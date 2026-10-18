//! Change detector: debounced notify events, hot-file polling, periodic
//! reconcile, overflow-triggered reconcile, and retry backoff.
//! All time comes from the injected `Clock`.

use crate::clock::Clock;
use crate::consts::{BACKOFF_MAX_SECS, DEBOUNCE_MS, HOT_POLL_SECS, HOT_WINDOW_SECS, RECONCILE_SECS};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use walkdir::WalkDir;

/// Wait before the n-th consecutive retry (n starts at 1): min(2^(n-1), BACKOFF_MAX_SECS).
pub fn backoff_secs(n: u32) -> u64 {
    let n = n.max(1);
    if n > 63 {
        return BACKOFF_MAX_SECS;
    }
    (1u64 << (n - 1)).min(BACKOFF_MAX_SECS)
}

/// Failure counter; reset on success.
#[derive(Debug)]
pub struct Backoff {
    n: u32,
}

impl Default for Backoff {
    fn default() -> Self {
        Self { n: 1 }
    }
}

impl Backoff {
    /// Record a failure and return the seconds to wait.
    pub fn on_failure(&mut self) -> u64 {
        let s = backoff_secs(self.n);
        self.n = self.n.saturating_add(1);
        s
    }

    pub fn on_success(&mut self) {
        self.n = 1;
    }
}

/// What one `tick` observed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// One entry per fired debounce window (paths collapsed, sorted).
    pub notify_batches: Vec<Vec<PathBuf>>,
    /// Hot files whose (size, mtime) changed.
    pub hot_changed: Vec<PathBuf>,
    /// Files first seen by a reconcile.
    pub new_files: Vec<PathBuf>,
    /// Known files found changed by a reconcile.
    pub reconcile_changed: Vec<PathBuf>,
    pub reconciled: bool,
}

type FileState = (u64, SystemTime);

pub struct ChangeDetector {
    root: PathBuf,
    clock: Arc<dyn Clock>,
    files: HashMap<PathBuf, FileState>,
    window_start: Option<SystemTime>,
    window_paths: Vec<PathBuf>,
    ready_batches: Vec<Vec<PathBuf>>,
    last_hot_poll: SystemTime,
    last_reconcile: SystemTime,
    overflow: bool,
    reconcile_count: u64,
}

fn stat(p: &Path) -> Option<FileState> {
    let m = std::fs::metadata(p).ok()?;
    Some((m.len(), m.modified().ok()?))
}

fn elapsed(now: SystemTime, earlier: SystemTime) -> Duration {
    now.duration_since(earlier).unwrap_or(Duration::ZERO)
}

impl ChangeDetector {
    /// Takes the baseline scan (not counted as a reconcile).
    pub fn new(root: impl Into<PathBuf>, clock: Arc<dyn Clock>) -> Self {
        let now = clock.now();
        let mut d = Self {
            root: root.into(),
            clock,
            files: HashMap::new(),
            window_start: None,
            window_paths: Vec::new(),
            ready_batches: Vec::new(),
            last_hot_poll: now,
            last_reconcile: now,
            overflow: false,
            reconcile_count: 0,
        };
        d.files = d.scan();
        d
    }

    pub fn reconcile_count(&self) -> u64 {
        self.reconcile_count
    }

    /// Feed one notify event (already reduced to a path).
    pub fn feed_event(&mut self, path: PathBuf) {
        let now = self.clock.now();
        self.close_window_if_due(now);
        if self.window_start.is_none() {
            self.window_start = Some(now);
        }
        if !self.window_paths.contains(&path) {
            self.window_paths.push(path);
        }
    }

    /// Feed a raw notify result. A rescan flag or any watcher error is treated
    /// as overflow (conservative; backend mapping is not verified by tests).
    pub fn feed_notify(&mut self, res: notify::Result<notify::Event>) {
        match res {
            Ok(ev) if ev.need_rescan() => self.feed_overflow(),
            Ok(ev) => {
                for p in ev.paths {
                    self.feed_event(p);
                }
            }
            Err(_) => self.feed_overflow(),
        }
    }

    /// Overflow (IN_Q_OVERFLOW, max_user_watches, RDCW overflow): reconcile on next tick.
    pub fn feed_overflow(&mut self) {
        self.overflow = true;
    }

    pub fn tick(&mut self) -> Report {
        let now = self.clock.now();
        let mut rep = Report::default();
        self.close_window_if_due(now);
        rep.notify_batches = std::mem::take(&mut self.ready_batches);

        let reconcile_due = self.overflow
            || elapsed(now, self.last_reconcile) >= Duration::from_secs(RECONCILE_SECS);
        if reconcile_due {
            self.overflow = false;
            self.last_reconcile = now;
            self.last_hot_poll = now;
            self.reconcile_count += 1;
            rep.reconciled = true;
            let scanned = self.scan();
            for (p, st) in &scanned {
                match self.files.get(p) {
                    None => rep.new_files.push(p.clone()),
                    Some(old) if old != st => rep.reconcile_changed.push(p.clone()),
                    _ => {}
                }
            }
            rep.new_files.sort();
            rep.reconcile_changed.sort();
            self.files = scanned;
        } else if elapsed(now, self.last_hot_poll) >= Duration::from_secs(HOT_POLL_SECS) {
            self.last_hot_poll = now;
            let window = Duration::from_secs(HOT_WINDOW_SECS);
            let hot: Vec<PathBuf> = self
                .files
                .iter()
                .filter(|(_, (_, mtime))| elapsed(now, *mtime) <= window)
                .map(|(p, _)| p.clone())
                .collect();
            for p in hot {
                if let Some(st) = stat(&p) {
                    if self.files.get(&p) != Some(&st) {
                        self.files.insert(p.clone(), st);
                        rep.hot_changed.push(p);
                    }
                }
            }
            rep.hot_changed.sort();
        }
        rep
    }

    fn close_window_if_due(&mut self, now: SystemTime) {
        if let Some(start) = self.window_start {
            if elapsed(now, start) >= Duration::from_millis(DEBOUNCE_MS) {
                let mut paths = std::mem::take(&mut self.window_paths);
                paths.sort();
                self.ready_batches.push(paths);
                self.window_start = None;
            }
        }
    }

    fn scan(&self) -> HashMap<PathBuf, FileState> {
        WalkDir::new(&self.root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| stat(e.path()).map(|s| (e.into_path(), s)))
            .collect()
    }
}
