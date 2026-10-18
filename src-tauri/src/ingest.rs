//! Background ingest thread: local engine, WSL supervisors, aggregator sync and event emission.

use crate::state::{lock, Shared};
use crate::wsl::{wsl_input, NoLaunch};
use chrono::Utc;
use lens_core::aggregator::{origin_for_source, SessionInput};
use lens_core::change_detector::ChangeDetector;
use lens_core::clock::{Clock, SystemClock};
use lens_core::distro::{DistroMonitor, SystemLxssRegistry, WslExeProbe};
use lens_core::engine::{default_sources, Engine};
use lens_core::ipc::{self, Phase};
use lens_core::model::v5::{self, Ev, SourceHealth};
use lens_core::model::Agent;
use lens_core::supervisor::{Supervisor, WslLauncher};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const TICK: Duration = Duration::from_millis(250);
const FILES_PER_TICK: usize = 64;
const LOCAL_HEALTH_EVERY: Duration = Duration::from_secs(5);

fn emit_ev(app: &AppHandle, ev: &Ev) {
    match ev {
        Ev::ProjectsChanged { upserts, .. } => {
            let _ = app.emit("projects_changed", ev);
            for r in upserts {
                let _ = app.emit("project_detail_changed", ipc::ev_project_detail_changed(&r.key));
            }
        }
        Ev::ProjectDetailChanged { .. } => {
            let _ = app.emit("project_detail_changed", ev);
        }
        Ev::HealthChanged { .. } => {
            let _ = app.emit("health_changed", ev);
        }
    }
}

fn local_health(h: &lens_core::model::SourceHealth) -> SourceHealth {
    use lens_core::model::SourceState as L;
    use v5::SourceState as S;
    SourceHealth {
        source_id: h.source_id.clone(),
        agent: Some(h.agent),
        origin: origin_for_source(&h.source_id),
        root_path: h.root_path.clone(),
        state: match h.state {
            L::Ok => S::Ok,
            L::NotInstalled => S::NotInstalled,
            L::Empty => S::Empty,
            L::AccessDenied => S::AccessDenied,
            L::UnsupportedSchema => S::UnsupportedSchema,
            L::Error => S::Error,
        },
        last_scan_at: h.last_scan_at.clone(),
        files_tracked: h.files_tracked,
        sessions_total: h.sessions_total,
        partial_sessions: h.partial_sessions,
        skipped_old_files: h.files_skipped_old,
        top_diagnostics: h.top_diagnostics.clone(),
    }
}


pub fn ingest(app: AppHandle, stop: Arc<AtomicBool>) {
    let shared = app.state::<Shared>();
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let sources = default_sources();
    let mut engine = Engine::new(sources.clone());
    let mut detectors: Vec<ChangeDetector> =
        sources.iter().map(|s| ChangeDetector::new(s.root.clone(), clock.clone())).collect();
    let registry = SystemLxssRegistry;
    let probe = WslExeProbe;
    let launcher = NoLaunch;
    let mut monitor = DistroMonitor::new(&registry, &probe, &launcher, clock.as_ref());
    let mut supervisors: BTreeMap<String, Supervisor> = BTreeMap::new();
    let wsl_launcher: Arc<dyn lens_core::supervisor::Launcher> = Arc::new(WslLauncher);

    *lock(&shared.phase) = Phase::FirstScan;
    *lock(&shared.known_sources) = sources.iter().map(|s| s.id.clone()).collect();
    engine.rescan();
    engine.process_all();

    let mut local_health_cache: Vec<SourceHealth> = Vec::new();
    let mut last_local_health: Option<Instant> = None;
    let mut first = true;
    let mut sv_sources: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    while !stop.load(Ordering::Relaxed) {
        let mut changed = first;
        for d in detectors.iter_mut() {
            let r = d.tick();
            let paths = r
                .notify_batches
                .into_iter()
                .flatten()
                .chain(r.hot_changed)
                .chain(r.new_files)
                .chain(r.reconcile_changed);
            for p in paths {
                engine.mark_dirty(&p);
            }
            if r.reconciled {
                engine.rescan();
                changed = true;
            }
        }
        changed |= engine.process_dirty(FILES_PER_TICK);

        // WSL: distro states and supervisors.
        let settings = lock(&shared.store).settings().clone();
        let statuses = monitor.poll(&settings.disabled_distros).to_vec();
        *lock(&shared.known_distros) = statuses.iter().map(|d| d.name.clone()).collect();
        let mut known: Vec<String> = sources.iter().map(|s| s.id.clone()).collect();
        for st in &statuses {
            for a in [Agent::ClaudeCode, Agent::Codex] {
                known.push(v5::wsl_source_id(&st.name, a));
            }
            let sv = supervisors
                .entry(st.name.clone())
                .or_insert_with(|| Supervisor::new(&st.name, &settings, wsl_launcher.clone(), clock.clone()));
            sv.update_settings(&settings);
            sv.tick_with(&statuses);
        }
        *lock(&shared.known_sources) = known;

        // Local health comes from a snapshot: refresh on change or periodically.
        if changed || last_local_health.is_none_or(|t| t.elapsed() >= LOCAL_HEALTH_EVERY) {
            local_health_cache = engine.snapshot(Utc::now()).sources.iter().map(local_health).collect();
            last_local_health = Some(Instant::now());
        }
        let mut health = local_health_cache.clone();
        for st in &statuses {
            let sv = supervisors.get(&st.name);
            if let Some(h) = st.source_health() {
                health.push(h);
            } else if let Some(h) = sv.and_then(|s| s.health()) {
                health.push(h);
            }
            if let Some(sv) = sv {
                health.extend(sv.receiver().source_health().values().cloned());
            }
        }

        let events = {
            let mut agg = lock(&shared.agg);
            if changed {
                engine.sync_aggregator(&mut agg);
                *lock(&shared.session_paths) = engine.session_paths().clone();
            }
            for (name, sv) in &supervisors {
                agg.set_distro_available(name, sv.origin_available());
                let inputs: Vec<SessionInput> = sv.sessions().iter().map(wsl_input).collect();
                let live: BTreeSet<String> = inputs.iter().map(SessionInput::uid).collect();
                let mut sids: BTreeSet<String> = sv_sources.remove(name).unwrap_or_default();
                sids.extend(inputs.iter().map(|i| i.source_id.clone()));
                // Stopped distros keep their last state; while the helper runs, vanished uids are dropped.
                if !sv.is_stopped() {
                    for sid in &sids {
                        for uid in agg.uids_of_source(sid) {
                            if !live.contains(&uid) {
                                agg.remove(&uid);
                            }
                        }
                    }
                }
                for i in inputs {
                    agg.upsert(i);
                }
                sv_sources.insert(name.clone(), sids);
            }
            agg.tick();
            agg.take_events()
        };
        if first {
            *lock(&shared.phase) = Phase::Ready;
            first = false;
        }
        for ev in &events {
            emit_ev(&app, ev);
        }
        let health_changed = {
            let mut cur = lock(&shared.health);
            if *cur != health {
                *cur = health.clone();
                true
            } else {
                false
            }
        };
        if health_changed {
            emit_ev(&app, &ipc::ev_health_changed(health));
        }

        std::thread::sleep(TICK);
    }

    // Shutdown: stop every helper.
    for sv in supervisors.values_mut() {
        sv.tick(false);
    }
}

