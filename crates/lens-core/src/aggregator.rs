//! Aggregator, first half (architecture-v5 sections 3.1, 4, 8): one session table keyed by
//! `sessionUid`, project rows recomputed only for affected projects, and a full re-evaluation
//! every `REEVAL_SECS` through the injected `Clock`. Every outgoing string goes through
//! `redact::sanitize`, and `projects_changed` is queued only when row content or order changes.
//! Lens's own repository is deliberately not excluded (F15, documented unprotectable limit).

use crate::clock::Clock;
use crate::consts::{HIDE_NON_REPO_SECS, HIDE_REPO_SECS, REEVAL_SECS};
use crate::model::v5::{
    self, ActiveTool, CurrentTask, Detail, Health, Hierarchy, Origin, OriginKind, ProjectActivity, SessionProject,
    UnifiedSession, UNATTRIBUTED_KEY,
};
use crate::model::{Agent, Attribution, Completeness, Confidence, Diagnostic, SessionKind, Status, StatusValue, TimeSource};
use crate::redact::{sanitize, Field};
use crate::session::{aggregate_updating, SessionState};
use chrono::{DateTime, Utc};
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

pub const PARENT_ENDED_REASON: &str = "parent turn ended after last activity";

/// Origin of a source id: `wsl:<distro>:claude|codex` is WSL, everything else is Windows.
pub fn origin_for_source(source_id: &str) -> Origin {
    match source_id.strip_prefix("wsl:") {
        Some(rest) => {
            let distro = rest.rsplit_once(':').map_or(rest, |(d, _)| d);
            Origin { kind: OriginKind::Wsl, distro: Some(distro.to_string()) }
        }
        None => Origin { kind: OriginKind::Windows, distro: None },
    }
}

/// One session handed to the aggregator.
#[derive(Debug, Clone)]
pub struct SessionInput {
    pub source_id: String,
    pub agent: Agent,
    pub origin: Origin,
    pub state: SessionState,
    pub file_mtime: Option<DateTime<Utc>>,
    pub projects: Vec<SessionProject>,
}

impl SessionInput {
    pub fn uid(&self) -> String {
        v5::session_uid(&self.source_id, &self.state.native_id)
    }

    fn parent_uid(&self) -> Option<String> {
        self.state.parent_native.as_ref().map(|p| v5::session_uid(&self.source_id, p))
    }

    fn last_activity(&self) -> Option<DateTime<Utc>> {
        self.state.last_active(self.file_mtime)
    }

    fn fingerprint(&self) -> u64 {
        let mut h = DefaultHasher::new();
        format!("{:?}", self).hash(&mut h);
        h.finish()
    }
}

struct Entry {
    input: SessionInput,
    fp: u64,
    status: Status,
}

pub struct Aggregator {
    clock: Arc<dyn Clock>,
    table: BTreeMap<String, Entry>,
    /// project key -> member session uids (automated sessions are never members).
    by_project: BTreeMap<String, BTreeSet<String>>,
    rows: BTreeMap<String, ProjectActivity>,
    last_reeval: Option<SystemTime>,
    recomputes: u64,
    unavailable_distros: BTreeSet<String>,
    /// Last emitted (sanitized) rows and order, for change detection.
    emitted: BTreeMap<String, ProjectActivity>,
    emitted_order: Vec<String>,
    events: Vec<v5::Ev>,
}

fn san(s: &str) -> String {
    sanitize(s, Field::Text)
}

fn san_opt(s: &Option<String>) -> Option<String> {
    s.as_deref().map(san)
}

fn sanitize_origin(o: &Origin) -> Origin {
    Origin { kind: o.kind, distro: san_opt(&o.distro) }
}

fn sanitize_row(r: &ProjectActivity) -> ProjectActivity {
    ProjectActivity { key: san(&r.key), name: san(&r.name), origin: sanitize_origin(&r.origin), ..r.clone() }
}

fn sanitize_session(u: UnifiedSession) -> UnifiedSession {
    UnifiedSession {
        origin: sanitize_origin(&u.origin),
        projects: u
            .projects
            .iter()
            .map(|p| SessionProject { key: san(&p.key), name: san(&p.name), display_path: san(&p.display_path), attribution: p.attribution })
            .collect(),
        status: v5::Status { reason: san(&u.status.reason), ..u.status },
        current_task: CurrentTask {
            title: san_opt(&u.current_task.title),
            last_user_message: san_opt(&u.current_task.last_user_message),
            last_assistant_message: san_opt(&u.current_task.last_assistant_message),
            active_tool: u.current_task.active_tool.map(|t| ActiveTool { name: san(&t.name), input: san(&t.input), ..t }),
        },
        health: Health {
            completeness: u.health.completeness,
            diagnostics: u.health.diagnostics.iter().map(|d| Diagnostic { code: san(&d.code), count: d.count }).collect(),
        },
        ..u
    }
}

fn project_keys(input: &SessionInput) -> Vec<String> {
    if input.state.kind == SessionKind::Automated {
        return Vec::new();
    }
    if input.projects.is_empty() {
        return vec![UNATTRIBUTED_KEY.to_string()];
    }
    let mut keys: Vec<String> = input.projects.iter().map(|p| p.key.clone()).collect();
    keys.sort();
    keys.dedup();
    keys
}

fn parse_ts(s: &Option<String>) -> Option<DateTime<chrono::FixedOffset>> {
    s.as_deref().and_then(|t| DateTime::parse_from_rfc3339(t).ok())
}

impl Aggregator {
    pub fn origin_available(&self, origin: &Origin) -> bool {
        match (&origin.kind, &origin.distro) {
            (OriginKind::Wsl, Some(d)) => !self.unavailable_distros.contains(d),
            _ => true,
        }
    }
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            table: BTreeMap::new(),
            by_project: BTreeMap::new(),
            rows: BTreeMap::new(),
            last_reeval: None,
            recomputes: 0,
            unavailable_distros: BTreeSet::new(),
            emitted: BTreeMap::new(),
            emitted_order: Vec::new(),
            events: Vec::new(),
        }
    }

    /// Drains queued events (`projects_changed` only when content or order changed).
    pub fn take_events(&mut self) -> Vec<v5::Ev> {
        std::mem::take(&mut self.events)
    }

    /// Compares the current sanitized rows and order with the last emitted ones.
    fn detect_changes(&mut self) {
        let list = self.list_projects();
        let order: Vec<String> = list.iter().map(|r| r.key.clone()).collect();
        let current: BTreeMap<String, ProjectActivity> = list.iter().map(|r| (r.key.clone(), r.clone())).collect();
        let upserts: Vec<ProjectActivity> = list.into_iter().filter(|r| self.emitted.get(&r.key) != Some(r)).collect();
        let removes: Vec<String> = self.emitted.keys().filter(|k| !current.contains_key(*k)).cloned().collect();
        if upserts.is_empty() && removes.is_empty() && order == self.emitted_order {
            return;
        }
        self.emitted = current;
        self.emitted_order = order.clone();
        self.events.push(v5::Ev::ProjectsChanged { upserts, removes, order });
    }

    fn now(&self) -> DateTime<Utc> {
        DateTime::<Utc>::from(self.clock.now())
    }

    /// Number of project-row recomputations so far (test hook for "only affected projects").
    pub fn project_recompute_count(&self) -> u64 {
        self.recomputes
    }

    pub fn session_count(&self) -> usize {
        self.table.len()
    }

    pub fn uids_of_source(&self, source_id: &str) -> Vec<String> {
        self.table.iter().filter(|(_, e)| e.input.source_id == source_id).map(|(u, _)| u.clone()).collect()
    }

    /// Inserts or replaces a session. Returns false when nothing changed.
    pub fn upsert(&mut self, input: SessionInput) -> bool {
        let uid = input.uid();
        let fp = input.fingerprint();
        if self.table.get(&uid).is_some_and(|e| e.fp == fp) {
            return false;
        }
        let mut dirty: BTreeSet<String> = BTreeSet::new();
        let old_parent = self.table.get(&uid).and_then(|e| e.input.parent_uid());
        if let Some(e) = self.table.get(&uid) {
            dirty.extend(project_keys(&e.input));
        }
        let keys = project_keys(&input);
        dirty.extend(keys.iter().cloned());
        for set in self.by_project.values_mut() {
            set.remove(&uid);
        }
        for k in &keys {
            self.by_project.entry(k.clone()).or_default().insert(uid.clone());
        }
        let parent = input.parent_uid();
        self.table.insert(uid.clone(), Entry { input, fp, status: Self::placeholder_status() });
        let mut related: BTreeSet<String> = BTreeSet::new();
        related.insert(uid.clone());
        related.extend(parent);
        related.extend(old_parent);
        related.extend(self.children_of(&uid));
        self.refresh_related(related, dirty);
        self.detect_changes();
        true
    }

    pub fn remove(&mut self, uid: &str) -> bool {
        let Some(e) = self.table.remove(uid) else { return false };
        let dirty: BTreeSet<String> = project_keys(&e.input).into_iter().collect();
        for set in self.by_project.values_mut() {
            set.remove(uid);
        }
        let mut related: BTreeSet<String> = BTreeSet::new();
        related.extend(e.input.parent_uid());
        related.extend(self.children_of(uid));
        self.refresh_related(related, dirty);
        self.detect_changes();
        true
    }

    /// Re-evaluates everything when `REEVAL_SECS` have passed since the last full evaluation.
    pub fn tick(&mut self) -> bool {
        let now = self.clock.now();
        let due = match self.last_reeval {
            None => true,
            Some(last) => now.duration_since(last).map_or(true, |d| d >= Duration::from_secs(REEVAL_SECS)),
        };
        if due {
            self.reeval();
        }
        due
    }

    /// Full re-evaluation of every session and every project row.
    pub fn reeval(&mut self) {
        let now = self.now();
        self.last_reeval = Some(self.clock.now());
        let updates: Vec<(String, Status)> = self.table.keys().map(|u| (u.clone(), self.full_status(u, now))).collect();
        for (u, st) in updates {
            if let Some(e) = self.table.get_mut(&u) {
                e.status = st;
            }
        }
        let keys: BTreeSet<String> = self.by_project.keys().cloned().chain(self.rows.keys().cloned()).collect();
        for k in keys {
            self.recompute_project(&k);
        }
        self.detect_changes();
    }

    /// Marks a WSL distro (un)available; affects `originAvailable` of its projects.
    pub fn set_distro_available(&mut self, distro: &str, available: bool) {
        let changed = if available {
            self.unavailable_distros.remove(distro)
        } else {
            self.unavailable_distros.insert(distro.to_string())
        };
        if changed {
            let keys: Vec<String> = self.rows.keys().cloned().collect();
            for k in keys {
                self.recompute_project(&k);
            }
            self.detect_changes();
        }
    }

    /// Project rows: `lastUpdatedAt` descending, equal times by name ascending (key as final tie-break).
    pub fn list_projects(&self) -> Vec<ProjectActivity> {
        let mut v: Vec<ProjectActivity> = self.rows.values().map(sanitize_row).collect();
        v.sort_by(|a, b| {
            let ta = a.last_updated_at.as_deref().and_then(|t| DateTime::parse_from_rfc3339(t).ok());
            let tb = b.last_updated_at.as_deref().and_then(|t| DateTime::parse_from_rfc3339(t).ok());
            tb.cmp(&ta).then_with(|| a.name.cmp(&b.name)).then_with(|| a.key.cmp(&b.key))
        });
        v
    }

    pub fn project_detail(&self, key: &str) -> Option<Detail> {
        let project = sanitize_row(self.rows.get(key).or_else(|| self.rows.values().find(|r| san(&r.key) == key))?);
        let sessions = project.session_uids.iter().filter_map(|u| self.session(u)).collect();
        Some(Detail { project, sessions })
    }

    /// Any tracked session, including automated ones (which never appear in project rows).
    pub fn session(&self, uid: &str) -> Option<UnifiedSession> {
        self.raw_session(uid).map(sanitize_session)
    }

    fn raw_session(&self, uid: &str) -> Option<UnifiedSession> {
        let e = self.table.get(uid)?;
        let s = &e.input.state;
        let last = e.input.last_activity();
        let partial = s.truncated_scan || s.degraded || s.diagnostics.contains_key("unknown_thread_source");
        Some(UnifiedSession {
            session_uid: uid.to_string(),
            native_id: s.native_id.clone(),
            agent: e.input.agent,
            source_id: e.input.source_id.clone(),
            origin: e.input.origin.clone(),
            last_activity_at: last.map(|d| d.to_rfc3339()),
            last_activity_source: if s.last_event_at.is_some() { TimeSource::Event } else { TimeSource::FileMtime },
            projects: e.input.projects.clone(),
            status: v5::Status {
                value: e.status.value,
                reason: e.status.reason.clone(),
                confidence: e.status.confidence,
                stale: e.status.stale,
                auto_review: e.status.auto_review,
            },
            current_task: CurrentTask {
                title: s.title.clone(),
                last_user_message: s.last_user.clone(),
                last_assistant_message: s.last_assistant.clone(),
                active_tool: s.open_tools.last().map(|t| ActiveTool {
                    name: t.name.clone(),
                    input: t.input.clone(),
                    call_id: Some(t.call_id.clone()).filter(|c| !c.is_empty()),
                    started_at: t.started_at.map(|d| d.to_rfc3339()),
                }),
            },
            hierarchy: Hierarchy {
                parent_uid: e.input.parent_uid(),
                child_uids: self
                    .children_of(uid)
                    .into_iter()
                    .filter(|c| self.table[c].input.state.kind != SessionKind::Automated)
                    .collect(),
                kind: s.kind,
            },
            health: Health {
                completeness: if partial { Completeness::Partial } else { Completeness::Complete },
                diagnostics: s.diagnostics.iter().map(|(code, count)| Diagnostic { code: code.clone(), count: *count }).collect(),
            },
        })
    }

    fn placeholder_status() -> Status {
        Status { value: StatusValue::Unknown, reason: String::new(), confidence: Confidence::Low, stale: false, auto_review: false, since: None }
    }

    fn children_of(&self, uid: &str) -> Vec<String> {
        self.table.iter().filter(|(_, e)| e.input.parent_uid().as_deref() == Some(uid)).map(|(u, _)| u.clone()).collect()
    }

    /// Evaluates one session and applies the parent-ended rule (no auto-review flag).
    fn hierarchy_status(&self, e: &Entry, now: DateTime<Utc>) -> Status {
        let mut st = e.input.state.evaluate(now, e.input.file_mtime);
        if st.value != StatusValue::Running {
            return st;
        }
        let Some(parent) = e.input.parent_uid().and_then(|p| self.table.get(&p)) else { return st };
        let pst = parent.input.state.evaluate(now, parent.input.file_mtime);
        if !matches!(pst.value, StatusValue::WaitingInput | StatusValue::Idle) {
            return st;
        }
        let Some(parent_end) = parse_ts(&pst.since) else { return st };
        if e.input.last_activity().is_some_and(|l| l.fixed_offset() < parent_end) {
            st.value = StatusValue::Idle;
            st.confidence = Confidence::Low;
            st.stale = false;
            st.reason = PARENT_ENDED_REASON.to_string();
        }
        st
    }

    fn full_status(&self, uid: &str, now: DateTime<Utc>) -> Status {
        let e = &self.table[uid];
        let mut st = self.hierarchy_status(e, now);
        if e.input.state.kind != SessionKind::Automated {
            // A running guardian_review child flags the parent; the parent's own value is unchanged.
            st.auto_review = self.children_of(uid).iter().any(|c| {
                let ce = &self.table[c];
                ce.input.state.kind == SessionKind::Automated && {
                    let cs = self.hierarchy_status(ce, now);
                    cs.value == StatusValue::Running && !cs.stale
                }
            });
        }
        st
    }

    fn refresh_related(&mut self, related: BTreeSet<String>, mut dirty: BTreeSet<String>) {
        let now = self.now();
        let updates: Vec<(String, Status)> =
            related.iter().filter(|u| self.table.contains_key(*u)).map(|u| (u.clone(), self.full_status(u, now))).collect();
        for (u, st) in updates {
            if let Some(e) = self.table.get_mut(&u) {
                dirty.extend(project_keys(&e.input));
                e.status = st;
            }
        }
        for k in dirty {
            self.recompute_project(&k);
        }
    }

    fn recompute_project(&mut self, key: &str) {
        self.recomputes += 1;
        let members: Vec<&Entry> = match self.by_project.get(key) {
            Some(set) => set.iter().filter_map(|u| self.table.get(u)).collect(),
            None => Vec::new(),
        };
        let Some(first) = members.first() else {
            self.by_project.remove(key);
            self.rows.remove(key);
            return;
        };
        let name = first.input.projects.iter().find(|p| p.key == key).map_or_else(|| key.to_string(), |p| p.name.clone());
        let origin = first.input.origin.clone();
        let mut last: Option<(DateTime<Utc>, TimeSource)> = None;
        for m in &members {
            if let Some(t) = m.input.last_activity() {
                if last.is_none_or(|(l, _)| t > l) {
                    let src = if m.input.state.last_event_at.is_some() { TimeSource::Event } else { TimeSource::FileMtime };
                    last = Some((t, src));
                }
            }
        }
        let (mut updating, conf) = aggregate_updating(members.iter().map(|m| &m.status));
        let agents: Vec<Agent> =
            Agent::ALL.into_iter().filter(|a| members.iter().any(|m| m.input.agent == *a)).collect();
        let mut ordered: Vec<(String, Option<DateTime<Utc>>)> =
            self.by_project[key].iter().map(|u| (u.clone(), self.table.get(u).and_then(|e| e.input.last_activity()))).collect();
        ordered.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let origin_available = match (&origin.kind, &origin.distro) {
            (OriginKind::Wsl, Some(d)) => !self.unavailable_distros.contains(d),
            _ => true,
        };
        // A stopped distro shows its last state; nothing there is being updated (section 6).
        updating &= origin_available;
        // Old rows are hidden but stay members, so a new event brings them back.
        let repo = members.iter().any(|m| m.input.projects.iter().any(|p| p.key == key && p.attribution == Attribution::GitRoot));
        let hide_secs = if repo { HIDE_REPO_SECS } else { HIDE_NON_REPO_SECS };
        if !updating && last.is_some_and(|(t, _)| (self.now() - t).num_seconds() >= hide_secs as i64) {
            self.rows.remove(key);
            return;
        }
        let row = ProjectActivity {
            key: key.to_string(),
            name,
            origin,
            last_updated_at: last.map(|(t, _)| t.to_rfc3339()),
            last_updated_source: last.map_or(TimeSource::FileMtime, |(_, s)| s),
            updating,
            updating_confidence: (updating && conf == Confidence::Low).then_some(Confidence::Low),
            origin_available,
            agents,
            session_uids: ordered.into_iter().map(|(u, _)| u).collect(),
        };
        self.rows.insert(key.to_string(), row);
    }
}
