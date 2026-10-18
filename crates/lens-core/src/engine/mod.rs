//! Source discovery, ingestion scheduling and snapshot aggregation (architecture-v3 §5.2, §5.4).

use crate::aggregator::{origin_for_source, Aggregator, SessionInput};
use crate::clock::{Clock, SystemClock};
use crate::model::*;
use crate::project::ProjectResolver;
use crate::reader::{Cursor, Limits};
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

pub const HOT_WINDOW: Duration = Duration::from_secs(crate::consts::HOT_WINDOW_SECS);
pub const DEFAULT_MAX_AGE: Duration = Duration::from_secs(crate::consts::INITIAL_SCAN_DAYS * 24 * 3600);

mod ingest;
mod parser;
mod scan;
mod snapshot;
mod sources;
mod thread_names;

pub use sources::{default_sources, SourceConfig};
use sources::matches_source;
use parser::Parser;
use thread_names::ThreadNameCache;

struct TrackedFile {
    source: usize,
    cursor: Cursor,
    parser: Parser,
    read_error: Option<io::ErrorKind>,
}

#[derive(Default, Clone)]
struct ScanInfo {
    state: Option<SourceState>,
    detail: Option<String>,
    last_scan_at: Option<DateTime<Utc>>,
    skipped_old: u64,
}

/// Checks that a source root can be listed (injectable so access denial can be simulated on any OS).
pub type DirOpener = Arc<dyn Fn(&Path) -> io::Result<()> + Send + Sync>;

pub struct Engine {
    clock: Arc<dyn Clock>,
    opener: DirOpener,
    sources: Vec<SourceConfig>,
    files: HashMap<PathBuf, TrackedFile>,
    dirty: BTreeSet<PathBuf>,
    scans: Vec<ScanInfo>,
    resolver: ProjectResolver,
    limits: Limits,
    max_age: Duration,
    full_parses: BTreeMap<String, u64>,
    /// Codex `session_index.jsonl` cache per source index: (mtime, len, id -> thread_name).
    thread_names: HashMap<usize, ThreadNameCache>,
}


impl Engine {
    pub fn new(sources: Vec<SourceConfig>) -> Self {
        let scans = vec![ScanInfo::default(); sources.len()];
        Self {
            clock: Arc::new(SystemClock),
            opener: Arc::new(|p: &Path| std::fs::read_dir(p).map(|_| ())),
            sources,
            files: HashMap::new(),
            dirty: BTreeSet::new(),
            scans,
            resolver: ProjectResolver::default(),
            limits: Limits::default(),
            max_age: DEFAULT_MAX_AGE,
            full_parses: BTreeMap::new(),
            thread_names: HashMap::new(),
        }
    }


    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    pub fn with_dir_opener(mut self, opener: DirOpener) -> Self {
        self.opener = opener;
        self
    }

    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    pub fn with_max_age(mut self, max_age: Duration) -> Self {
        self.max_age = max_age;
        self
    }

    /// Number of full `Value` parses per record `type` (`""` = no readable type). Test hook for the probe rule.
    pub fn full_parse_count(&self, ty: &str) -> u64 {
        self.full_parses.get(ty).copied().unwrap_or(0)
    }

    pub fn sources(&self) -> &[SourceConfig] {
        &self.sources
    }

    pub fn has_dirty(&self) -> bool {
        !self.dirty.is_empty()
    }


    /// Marks a path reported by the change detector.
    pub fn mark_dirty(&mut self, path: &Path) {
        if self.files.contains_key(path) {
            self.dirty.insert(path.to_path_buf());
            return;
        }
        for (idx, src) in self.sources.iter().enumerate() {
            if path.starts_with(&src.root) && matches_source(src.agent, path) && path.is_file() {
                self.files.insert(
                    path.to_path_buf(),
                    TrackedFile { source: idx, cursor: Cursor::default(), parser: Parser::new(src.agent, path), read_error: None },
                );
                self.dirty.insert(path.to_path_buf());
                return;
            }
        }
    }

    /// Stats recently-active files; covers writers whose appends do not raise change events.
    pub fn poll_hot(&mut self) {
        let now = self.clock.now();
        for (path, tf) in &self.files {
            let Some(mtime) = tf.cursor.mtime else { continue };
            if now.duration_since(mtime).unwrap_or_default() > HOT_WINDOW {
                continue;
            }
            if let Ok(meta) = std::fs::metadata(path) {
                if meta.len() != tf.cursor.size || meta.modified().ok() != Some(mtime) {
                    self.dirty.insert(path.clone());
                }
            }
        }
    }

    /// Processes up to `budget` dirty files, most recently modified first. Returns true if anything was read.
    pub fn process_dirty(&mut self, budget: usize) -> bool {
        let mut queue: Vec<(Option<SystemTime>, PathBuf)> = self
            .dirty
            .iter()
            .map(|p| (std::fs::metadata(p).and_then(|m| m.modified()).ok(), p.clone()))
            .collect();
        queue.sort_by_key(|b| std::cmp::Reverse(b.0));
        let mut changed = false;
        for (_, path) in queue.into_iter().take(budget) {
            self.dirty.remove(&path);
            let more = self.process_file(&path);
            changed = true;
            if more {
                self.dirty.insert(path);
            }
        }
        changed
    }

    pub fn process_all(&mut self) {
        while self.has_dirty() {
            self.process_dirty(usize::MAX);
        }
    }


    /// A fresh aggregator driven by this engine's clock.
    pub fn new_aggregator(&self) -> Aggregator {
        Aggregator::new(self.clock.clone())
    }

    /// Feeds every parsed session into the aggregator and drops the ones that vanished.
    pub fn sync_aggregator(&mut self, agg: &mut Aggregator) {
        self.refresh_thread_names();
        let mut inputs: BTreeMap<String, (SessionInput, Option<DateTime<Utc>>)> = BTreeMap::new();
        for tf in self.files.values() {
            let src = &self.sources[tf.source];
            let mtime = tf.cursor.mtime.map(DateTime::<Utc>::from);
            for s in tf.parser.sessions() {
                let mut state = s.clone();
                if let Some(t) = self.thread_names.get(&tf.source).and_then(|c| c.2.get(&s.native_id)) {
                    state.title = Some(t.clone());
                }
                let projects = s
                    .cwd
                    .as_deref()
                    .and_then(|c| self.resolver.resolve(c))
                    .into_iter()
                    .map(|p| crate::model::v5::SessionProject {
                        key: p.key,
                        name: p.name,
                        display_path: p.display_path,
                        attribution: p.attribution,
                    })
                    .collect();
                let input = SessionInput {
                    source_id: src.id.clone(),
                    agent: src.agent,
                    origin: origin_for_source(&src.id),
                    state,
                    file_mtime: mtime,
                    projects,
                };
                let last = s.last_active(mtime);
                let uid = input.uid();
                // The same id in two files (e.g. resumed sessions): keep the most recent one.
                if inputs.get(&uid).is_none_or(|(_, l)| *l < last) {
                    inputs.insert(uid, (input, last));
                }
            }
        }
        for src in &self.sources {
            for uid in agg.uids_of_source(&src.id) {
                if !inputs.contains_key(&uid) {
                    agg.remove(&uid);
                }
            }
        }
        for (_, (input, _)) in inputs {
            agg.upsert(input);
        }
    }
}
