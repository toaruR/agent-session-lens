//! `serve`: streams the normalized NDJSON contract (architecture-v5 §6.2, §6.3) to stdout.
//!
//! `Server` is driven by an injected `Clock`; `run_stdio` supplies the Linux-only inputs
//! (HOME, distro name) from the environment so everything else is testable on any host.

use chrono::{DateTime, SecondsFormat, Utc};
use lens_core::aggregator::{origin_for_source, Aggregator};
use lens_core::clock::{Clock, SystemClock};
use lens_core::consts::{HB_INTERVAL_SECS, HOT_POLL_SECS, MAX_LINE_BYTES, PROTOCOL_VERSION, RECONCILE_SECS};
use lens_core::model::v5::{self, HelperErrorCode, Msg, SourceHealth, SourceState, UnifiedSession};
use lens_core::model::{Agent, SourceState as CoreState};
use lens_core::redact::{sanitize, Field};
use lens_core::{Engine, SourceConfig};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

const POLL_SLEEP: Duration = Duration::from_secs(1);
const DIRTY_BUDGET: usize = 32;

/// Injected inputs. `roots` empty means the defaults under `home`.
#[derive(Debug, Clone)]
pub struct Config {
    pub roots: Vec<PathBuf>,
    pub home: PathBuf,
    pub distro: String,
}

/// Roots given after `--roots` (already validated by `cli`).
pub fn parse_roots(args: &[String]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut in_roots = false;
    for a in args {
        if a == "--roots" {
            in_roots = true;
        } else if a.starts_with("--") {
            in_roots = false;
        } else if in_roots {
            roots.push(PathBuf::from(a));
        }
    }
    roots
}

/// HOME / WSL_DISTRO_NAME resolution (Linux side); both can be injected via `Config` instead.
pub fn config_from_env(args: &[String]) -> Config {
    let home = std::env::var("HOME").ok().filter(|h| h.starts_with('/')).unwrap_or_else(|| "/root".into());
    let distro = std::env::var("WSL_DISTRO_NAME").ok().filter(|d| !d.is_empty()).unwrap_or_else(|| "unknown".into());
    Config { roots: parse_roots(args), home: PathBuf::from(home), distro }
}

fn rfc(t: SystemTime) -> String {
    DateTime::<Utc>::from(t).to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn elapsed(now: SystemTime, since: SystemTime) -> Duration {
    now.duration_since(since).unwrap_or_default()
}

fn map_state(s: CoreState) -> SourceState {
    match s {
        CoreState::Ok => SourceState::Ok,
        CoreState::NotInstalled => SourceState::NotInstalled,
        CoreState::Empty => SourceState::Empty,
        CoreState::AccessDenied => SourceState::AccessDenied,
        CoreState::UnsupportedSchema => SourceState::UnsupportedSchema,
        CoreState::Error => SourceState::Error,
    }
}

/// Redaction and truncation before anything leaves the process (§10).
fn sanitize_session(s: &mut UnifiedSession) {
    let t = &mut s.current_task;
    for x in [&mut t.title, &mut t.last_user_message, &mut t.last_assistant_message].into_iter().flatten() {
        *x = sanitize(x, Field::Text);
    }
    if let Some(a) = &mut t.active_tool {
        a.input = sanitize(&a.input, Field::Text);
    }
}

pub struct Server {
    clock: Arc<dyn Clock>,
    cfg: Config,
    engine: Engine,
    agg: Aggregator,
    source_ids: Vec<String>,
    sent_sessions: BTreeMap<String, String>,
    sent_health: BTreeMap<String, String>,
    unreadable: BTreeSet<usize>,
    last_hb: SystemTime,
    last_rescan: SystemTime,
    last_poll: SystemTime,
}

impl Server {
    pub fn new(cfg: Config, clock: Arc<dyn Clock>) -> Self {
        let sources = build_sources(&cfg);
        let mut source_ids: Vec<String> = sources.iter().map(|s| s.id.clone()).collect();
        source_ids.dedup();
        let engine = Engine::new(sources).with_clock(clock.clone());
        let agg = engine.new_aggregator();
        let now = clock.now();
        Self {
            clock,
            cfg,
            engine,
            agg,
            source_ids,
            sent_sessions: BTreeMap::new(),
            sent_health: BTreeMap::new(),
            unreadable: BTreeSet::new(),
            last_hb: now,
            last_rescan: now,
            last_poll: now,
        }
    }

    fn emit(out: &mut dyn Write, msg: &Msg) -> io::Result<bool> {
        let mut line = serde_json::to_string(msg).map_err(io::Error::other)?;
        if line.len() + 1 > MAX_LINE_BYTES {
            return Ok(false);
        }
        line.push('\n');
        out.write_all(line.as_bytes())?;
        out.flush()?;
        Ok(true)
    }

    /// hello, source_health for every source, root errors, then every session.
    pub fn start(&mut self, out: &mut dyn Write) -> io::Result<()> {
        let now = self.clock.now();
        self.last_hb = now;
        self.last_rescan = now;
        self.last_poll = now;
        Self::emit(
            out,
            &Msg::Hello {
                protocol: i64::from(PROTOCOL_VERSION),
                helper_version: env!("CARGO_PKG_VERSION").to_string(),
                distro: self.cfg.distro.clone(),
                home: self.cfg.home.to_string_lossy().into_owned(),
                now: rfc(now),
            },
        )?;
        self.engine.rescan();
        self.engine.process_all();
        self.engine.sync_aggregator(&mut self.agg);
        self.agg.tick();
        self.emit_health(out)?;
        self.check_roots(out)?;
        self.emit_sessions(out)
    }

    /// One scheduling step: hot polling, periodic reconcile, diffs and the heartbeat.
    pub fn tick(&mut self, out: &mut dyn Write) -> io::Result<()> {
        let now = self.clock.now();
        let mut changed = false;
        if elapsed(now, self.last_rescan) >= Duration::from_secs(RECONCILE_SECS) {
            self.engine.rescan();
            self.last_rescan = now;
            self.last_poll = now;
            changed = true;
            self.check_roots(out)?;
        } else if elapsed(now, self.last_poll) >= Duration::from_secs(HOT_POLL_SECS) {
            self.engine.poll_hot();
            self.last_poll = now;
        }
        if self.engine.has_dirty() {
            changed |= self.engine.process_dirty(DIRTY_BUDGET);
        }
        if changed {
            self.engine.sync_aggregator(&mut self.agg);
        }
        let due = self.agg.tick();
        if changed {
            self.emit_health(out)?;
        }
        if changed || due {
            self.emit_sessions(out)?;
        }
        if elapsed(now, self.last_hb) >= Duration::from_secs(HB_INTERVAL_SECS) {
            self.last_hb = now;
            Self::emit(out, &Msg::Heartbeat { now: rfc(now) })?;
        }
        Ok(())
    }

    /// Explicit roots only: a missing default root is a normal `not_installed`.
    fn check_roots(&mut self, out: &mut dyn Write) -> io::Result<()> {
        for (i, root) in self.cfg.roots.iter().enumerate() {
            if std::fs::read_dir(root).is_err() {
                if self.unreadable.insert(i) {
                    Self::emit(out, &Msg::Error { code: HelperErrorCode::RootUnreadable })?;
                }
            } else {
                self.unreadable.remove(&i);
            }
        }
        Ok(())
    }

    fn emit_health(&mut self, out: &mut dyn Write) -> io::Result<()> {
        let snap = self.engine.snapshot(DateTime::<Utc>::from(self.clock.now()));
        let mut seen = BTreeSet::new();
        for h in snap.sources {
            if !seen.insert(h.source_id.clone()) {
                continue;
            }
            let health = SourceHealth {
                source_id: h.source_id.clone(),
                agent: Some(h.agent),
                origin: origin_for_source(&h.source_id),
                root_path: h.root_path,
                state: map_state(h.state),
                last_scan_at: h.last_scan_at,
                files_tracked: h.files_tracked,
                sessions_total: h.sessions_total,
                partial_sessions: h.partial_sessions,
                skipped_old_files: h.files_skipped_old,
                top_diagnostics: h.top_diagnostics,
            };
            let mut cmp = health.clone();
            cmp.last_scan_at = None;
            let key = serde_json::to_string(&cmp).map_err(io::Error::other)?;
            if self.sent_health.get(&h.source_id) == Some(&key) {
                continue;
            }
            let msg = Msg::SourceHealth { source_id: h.source_id.clone(), health };
            if Self::emit(out, &msg)? {
                self.sent_health.insert(h.source_id, key);
            }
        }
        Ok(())
    }

    /// session_upsert for new/changed sessions, session_remove for vanished ones.
    fn emit_sessions(&mut self, out: &mut dyn Write) -> io::Result<()> {
        let mut current: BTreeSet<String> = BTreeSet::new();
        for id in &self.source_ids {
            current.extend(self.agg.uids_of_source(id));
        }
        let gone: Vec<String> = self.sent_sessions.keys().filter(|u| !current.contains(*u)).cloned().collect();
        for uid in gone {
            self.sent_sessions.remove(&uid);
            Self::emit(out, &Msg::SessionRemove { session_uid: uid })?;
        }
        for uid in current {
            let Some(mut s) = self.agg.session(&uid) else { continue };
            sanitize_session(&mut s);
            let mut key = serde_json::to_string(&s).map_err(io::Error::other)?;
            if self.sent_sessions.get(&uid) == Some(&key) {
                continue;
            }
            let mut sent = Self::emit(out, &Msg::SessionUpsert { session: s.clone() })?;
            if !sent {
                // Still oversized: drop the free-text fields and retry once.
                s.current_task = v5::CurrentTask::default();
                key = serde_json::to_string(&s).map_err(io::Error::other)?;
                sent = Self::emit(out, &Msg::SessionUpsert { session: s })?;
            }
            if sent {
                self.sent_sessions.insert(uid, key);
            }
        }
        Ok(())
    }
}

fn build_sources(cfg: &Config) -> Vec<SourceConfig> {
    let id = |a| v5::wsl_source_id(&cfg.distro, a);
    if cfg.roots.is_empty() {
        return vec![
            SourceConfig { id: id(Agent::ClaudeCode), agent: Agent::ClaudeCode, root: cfg.home.join(".claude").join("projects") },
            SourceConfig { id: id(Agent::Codex), agent: Agent::Codex, root: cfg.home.join(".codex").join("sessions") },
        ];
    }
    cfg.roots
        .iter()
        .map(|r| {
            let agent = if r.file_name().and_then(|n| n.to_str()) == Some("sessions") { Agent::Codex } else { Agent::ClaudeCode };
            SourceConfig { id: id(agent), agent, root: r.clone() }
        })
        .collect()
}

/// Production entry: real clock, env-resolved HOME and distro. Returns the exit code.
pub fn run_stdio(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut server = Server::new(config_from_env(args), Arc::new(SystemClock));
    let fail = |err: &mut dyn Write| {
        let _ = err.write_all(b"stdout_failed");
        let _ = err.flush();
        5
    };
    if server.start(out).is_err() {
        return fail(err);
    }
    loop {
        std::thread::sleep(POLL_SLEEP);
        if server.tick(out).is_err() {
            return fail(err);
        }
    }
}
