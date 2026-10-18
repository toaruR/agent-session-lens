//! Change detector loop: notify + hot-file polling + periodic reconcile (architecture-v3 §5.2).

use crate::engine::{Engine, SourceConfig};
use crate::model::Snapshot;
use chrono::Utc;
use notify::{RecursiveMode, Watcher};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const TICK: Duration = Duration::from_millis(500);
pub const HOT_POLL_EVERY: Duration = Duration::from_secs(2);
pub const RESCAN_EVERY: Duration = Duration::from_secs(60);
pub const EMIT_AT_LEAST_EVERY: Duration = Duration::from_secs(5);
pub const FILES_PER_TICK: usize = 64;

/// Runs until `stop` is set, calling `on_snapshot` after changes and periodically
/// (status depends on elapsed time, so snapshots are re-evaluated even without new data).
pub fn run(sources: Vec<SourceConfig>, stop: Arc<AtomicBool>, mut on_snapshot: impl FnMut(Snapshot)) {
    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })
    .ok();
    let mut watched: BTreeSet<PathBuf> = BTreeSet::new();
    let mut engine = Engine::new(sources);

    let mut watch_roots = |engine: &Engine, watcher: &mut Option<notify::RecommendedWatcher>| {
        let Some(w) = watcher.as_mut() else { return };
        for src in engine.sources() {
            if !watched.contains(&src.root) && src.root.is_dir() && w.watch(&src.root, RecursiveMode::Recursive).is_ok() {
                watched.insert(src.root.clone());
            }
        }
    };

    engine.rescan();
    watch_roots(&engine, &mut watcher);
    engine.process_all();
    on_snapshot(engine.snapshot(Utc::now()));

    let mut last_hot = Instant::now();
    let mut last_rescan = Instant::now();
    let mut last_emit = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        let mut force_rescan = false;
        let mut handle = |res: notify::Result<notify::Event>, engine: &mut Engine| match res {
            Ok(ev) => {
                if ev.need_rescan() {
                    force_rescan = true;
                }
                for p in &ev.paths {
                    engine.mark_dirty(p);
                }
            }
            // Includes buffer overflow: reconcile immediately.
            Err(_) => force_rescan = true,
        };
        match rx.recv_timeout(TICK) {
            Ok(res) => handle(res, &mut engine),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => std::thread::sleep(TICK),
        }
        while let Ok(res) = rx.try_recv() {
            handle(res, &mut engine);
        }
        if last_hot.elapsed() >= HOT_POLL_EVERY {
            engine.poll_hot();
            last_hot = Instant::now();
        }
        if force_rescan || last_rescan.elapsed() >= RESCAN_EVERY {
            engine.rescan();
            watch_roots(&engine, &mut watcher);
            last_rescan = Instant::now();
        }
        let changed = engine.process_dirty(FILES_PER_TICK);
        if changed || last_emit.elapsed() >= EMIT_AT_LEAST_EVERY {
            on_snapshot(engine.snapshot(Utc::now()));
            last_emit = Instant::now();
        }
    }
}
