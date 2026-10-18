//! Legacy snapshot assembly (sessions + source health).

use super::Engine;
use crate::model::*;
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Per-source stats tuple used while building a snapshot.
type SourceStats = (u64, u64, BTreeSet<String>, BTreeMap<String, u64>);

impl Engine {

    /// Legacy flat snapshot (`sessions` + `sources`), built directly from the parsed files.
    ///
    /// Kept for backward compatibility: the v5 IPC path uses `sync_aggregator` + `Aggregator`
    /// instead, but existing ingest tests (`tests/ingest.rs`) and the `smoke` example still read
    /// `Snapshot::sessions` from here. Not used by the Tauri app. Do not remove without migrating them.
    pub fn snapshot(&mut self, now: DateTime<Utc>) -> Snapshot {
        self.refresh_thread_names();
        let mut sessions: BTreeMap<String, UnifiedSession> = BTreeMap::new();
        let mut per_source: Vec<SourceStats> =
            vec![(0, 0, BTreeSet::new(), BTreeMap::new()); self.sources.len()];

        for tf in self.files.values() {
            let src = &self.sources[tf.source];
            let mtime = tf.cursor.mtime.map(DateTime::<Utc>::from);
            for s in tf.parser.sessions() {
                let uid = format!("{}:{}", src.id, s.native_id);
                let projects: Vec<ProjectRef> = s.cwd.as_deref().and_then(|c| self.resolver.resolve(c)).into_iter().collect();
                let diagnostics: Vec<Diagnostic> =
                    s.diagnostics.iter().map(|(code, count)| Diagnostic { code: code.clone(), count: *count }).collect();
                let partial = s.truncated_scan || s.degraded || s.diagnostics.contains_key("unknown_thread_source");
                let title = self.thread_names.get(&tf.source).and_then(|c| c.2.get(&s.native_id)).cloned().or_else(|| s.title.clone());
                let last_active = s.last_active(mtime);
                let open_tool = s.open_tools.last().map(|t| ActiveTool {
                    name: t.name.clone(),
                    input: t.input.clone(),
                    call_id: Some(t.call_id.clone()).filter(|c| !c.is_empty()),
                    started_at: t.started_at.map(|d| d.to_rfc3339()),
                });
                let us = UnifiedSession {
                    session_uid: uid.clone(),
                    native_id: s.native_id.clone(),
                    agent: src.agent,
                    source_id: src.id.clone(),
                    writer_version: s.writer_version.clone(),
                    projects,
                    raw_paths: s.cwd.iter().cloned().collect(),
                    status: s.evaluate(now, mtime),
                    current_task: CurrentTask {
                        title,
                        last_user_message: s.last_user.clone(),
                        last_assistant_message: s.last_assistant.clone(),
                        active_tool: open_tool,
                    },
                    metrics: Metrics { step_count: s.step_count, tool_calls: s.tool_calls },
                    hierarchy: Hierarchy {
                        parent_uid: s.parent_native.as_ref().map(|p| format!("{}:{}", src.id, p)),
                        child_uids: Vec::new(),
                        kind: s.kind,
                    },
                    timestamps: Timestamps {
                        started_at: s.started_at.map(|d| d.to_rfc3339()),
                        last_active_at: last_active.map(|d| d.to_rfc3339()),
                        last_active_source: if s.last_event_at.is_some() { TimeSource::Event } else { TimeSource::FileMtime },
                    },
                    health: Health {
                        completeness: if partial { Completeness::Partial } else { Completeness::Complete },
                        diagnostics,
                    },
                };
                let acc = &mut per_source[tf.source];
                // The same id in two files (e.g. resumed sessions): keep the most recent one.
                let keep = match sessions.get(&uid) {
                    Some(existing) => existing.timestamps.last_active_at < us.timestamps.last_active_at,
                    None => {
                        acc.0 += 1;
                        if partial {
                            acc.1 += 1;
                        }
                        true
                    }
                };
                if let Some(v) = &s.writer_version {
                    acc.2.insert(v.clone());
                }
                for (code, count) in &s.diagnostics {
                    *acc.3.entry(code.clone()).or_default() += count;
                }
                if keep {
                    sessions.insert(uid, us);
                }
            }
        }

        // Link children to parents; flag parents whose automated reviewer is running.
        let links: Vec<(String, String, bool)> = sessions
            .values()
            .filter_map(|s| {
                let running = s.hierarchy.kind == SessionKind::Automated && s.status.value == StatusValue::Running;
                s.hierarchy.parent_uid.clone().map(|p| (p, s.session_uid.clone(), running))
            })
            .collect();
        for (parent, child, review_running) in links {
            if let Some(p) = sessions.get_mut(&parent) {
                p.hierarchy.child_uids.push(child);
                if review_running {
                    p.status.auto_review = true;
                    p.status.reason = format!("auto_review; {}", p.status.reason);
                }
            }
        }

        // A child cannot still be running if its parent's turn ended after the child's last activity
        // (e.g. a subagent whose final record was never written).
        let parse = |s: &Option<String>| s.as_deref().and_then(|t| DateTime::parse_from_rfc3339(t).ok());
        let ended_parents: HashMap<String, DateTime<chrono::FixedOffset>> = sessions
            .values()
            .filter(|s| matches!(s.status.value, StatusValue::WaitingInput | StatusValue::Idle))
            .filter_map(|s| parse(&s.status.since).map(|t| (s.session_uid.clone(), t)))
            .collect();
        for s in sessions.values_mut() {
            let Some(parent_end) = s.hierarchy.parent_uid.as_ref().and_then(|p| ended_parents.get(p)) else { continue };
            if s.status.value == StatusValue::Running && parse(&s.timestamps.last_active_at).is_some_and(|l| l < *parent_end) {
                s.status.value = StatusValue::Idle;
                s.status.confidence = Confidence::Low;
                s.status.stale = false;
                s.status.reason = format!("parent turn ended after last activity; {}", s.status.reason);
            }
        }

        let mut list: Vec<UnifiedSession> = sessions.into_values().collect();
        list.sort_by(|a, b| b.timestamps.last_active_at.cmp(&a.timestamps.last_active_at));

        let mut files_per_source = vec![0u64; self.sources.len()];
        let mut read_errors: Vec<BTreeSet<String>> = vec![BTreeSet::new(); self.sources.len()];
        for tf in self.files.values() {
            files_per_source[tf.source] += 1;
            if let Some(k) = tf.read_error {
                read_errors[tf.source].insert(format!("{k:?}"));
            }
        }
        let sources = self
            .sources
            .iter()
            .enumerate()
            .map(|(i, src)| {
                let info = &self.scans[i];
                let (total, partial, versions, diags) = &per_source[i];
                let mut top: Vec<Diagnostic> = diags.iter().map(|(c, n)| Diagnostic { code: c.clone(), count: *n }).collect();
                top.sort_by_key(|b| std::cmp::Reverse(b.count));
                top.truncate(5);
                let mut detail = info.detail.clone();
                if !read_errors[i].is_empty() {
                    let errs = read_errors[i].iter().cloned().collect::<Vec<_>>().join(", ");
                    detail = Some(format!("read errors: {errs}"));
                }
                SourceHealth {
                    source_id: src.id.clone(),
                    agent: src.agent,
                    root_path: src.root.to_string_lossy().into_owned(),
                    state: info.state.unwrap_or(SourceState::Empty),
                    detail,
                    last_scan_at: info.last_scan_at.map(|d| d.to_rfc3339()),
                    files_tracked: files_per_source[i],
                    files_skipped_old: info.skipped_old,
                    sessions_total: *total,
                    partial_sessions: *partial,
                    writer_versions: versions.iter().cloned().collect(),
                    top_diagnostics: top,
                }
            })
            .collect();

        Snapshot { generated_at: now.to_rfc3339(), sessions: list, sources }
    }
}
