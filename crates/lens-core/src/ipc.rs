//! IPC layer, read side (architecture-v5 section 8). No Tauri types: the shell adapts these functions.

use crate::aggregator::Aggregator;
use crate::model::v5::{
    Detail, Ev, IpcError, IpcErrorCode, Origin, OriginKind, ProjectActivity, SourceHealth,
    UnifiedSession,
};
use crate::redact::{sanitize, Field};
use crate::settings::{SettingsFs, SettingsStore};

/// Aggregator lifecycle as seen by the IPC layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Aggregator not started yet.
    NotStarted,
    /// First scan still running.
    FirstScan,
    /// First scan finished.
    Ready,
}

/// Read-only state behind the IPC read commands. Tests inject a fake.
pub trait IpcRead {
    fn phase(&self) -> Phase;
    fn projects(&self) -> Vec<ProjectActivity>;
    fn detail(&self, key: &str) -> Option<Detail>;
    fn health(&self) -> Vec<SourceHealth>;
}

/// `IpcRead` over a real `Aggregator` plus externally tracked phase and source health.
pub struct AggregatorView<'a> {
    pub aggregator: &'a Aggregator,
    pub phase: Phase,
    pub health: Vec<SourceHealth>,
}

impl IpcRead for AggregatorView<'_> {
    fn phase(&self) -> Phase {
        self.phase
    }
    fn projects(&self) -> Vec<ProjectActivity> {
        self.aggregator.list_projects()
    }
    fn detail(&self, key: &str) -> Option<Detail> {
        self.aggregator.project_detail(key)
    }
    fn health(&self) -> Vec<SourceHealth> {
        self.health.clone()
    }
}

fn err(code: IpcErrorCode, message: &str) -> IpcError {
    IpcError { code, message: message.to_string() }
}

/// `not_ready` while the first scan runs (or before start); otherwise rows ordered by lastUpdatedAt desc.
pub fn list_projects(s: &dyn IpcRead) -> Result<Vec<ProjectActivity>, IpcError> {
    if s.phase() != Phase::Ready {
        return Err(err(IpcErrorCode::NotReady, "first scan in progress"));
    }
    Ok(s.projects())
}

/// `invalid_argument` for an empty key, `project_not_found` for an unknown key.
pub fn get_project_detail(s: &dyn IpcRead, key: &str) -> Result<Detail, IpcError> {
    if key.is_empty() {
        return Err(err(IpcErrorCode::InvalidArgument, "key must not be empty"));
    }
    s.detail(key)
        .map(sanitize_detail)
        .ok_or_else(|| err(IpcErrorCode::ProjectNotFound, "project not found"))
}

/// `not_ready` only before the Aggregator has started; partial results during the first scan.
pub fn get_health(s: &dyn IpcRead) -> Result<Vec<SourceHealth>, IpcError> {
    if s.phase() == Phase::NotStarted {
        return Err(err(IpcErrorCode::NotReady, "aggregator not started"));
    }
    Ok(s.health())
}

// ---- write side ----

/// Filesystem check behind `set_source_root`. Injected so tests never touch the disk.
pub trait PathProbe {
    /// True when `path` can be read as a directory for a source of this origin.
    fn is_readable(&self, origin: &Origin, path: &str) -> bool;
}

/// Real probe: Windows paths via `read_dir`; WSL paths cannot be probed from the host
/// (the helper reports `root_unreadable`), so they pass.
pub struct RealPathProbe;

impl PathProbe for RealPathProbe {
    fn is_readable(&self, origin: &Origin, path: &str) -> bool {
        match origin.kind {
            OriginKind::Windows => std::fs::read_dir(path).is_ok(),
            OriginKind::Wsl => true,
        }
    }
}

/// `invalid_argument` for an empty distro, `distro_not_found` when not in `known_distros`,
/// `settings_save_failed` when the write fails.
pub fn set_distro_enabled<F: SettingsFs>(
    store: &mut SettingsStore<F>,
    known_distros: &[String],
    distro: &str,
    enabled: bool,
) -> Result<(), IpcError> {
    if distro.is_empty() {
        return Err(err(IpcErrorCode::InvalidArgument, "distro must not be empty"));
    }
    if !known_distros.iter().any(|d| d == distro) {
        return Err(err(IpcErrorCode::DistroNotFound, "distro not found"));
    }
    store
        .set_distro_enabled(distro, enabled)
        .map_err(|_| err(IpcErrorCode::SettingsSaveFailed, "settings could not be saved"))
}

fn is_drive_absolute(p: &str) -> bool {
    let b = p.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == 92 || b[2] == b'/')
}

fn source_origin(source_id: &str) -> Origin {
    match source_id.strip_prefix("wsl:").and_then(|r| r.rsplit_once(':')) {
        Some((distro, _)) => Origin { kind: OriginKind::Wsl, distro: Some(distro.to_string()) },
        None => Origin { kind: OriginKind::Windows, distro: None },
    }
}

/// `source_not_found` when `source_id` is not in `known_sources`; `path_invalid` unless the path
/// is drive-absolute (Windows) or starts with `/` (WSL), and for UNC or unreadable paths;
/// `None` resets to the default. `settings_save_failed` when the write fails.
pub fn set_source_root<F: SettingsFs>(
    store: &mut SettingsStore<F>,
    known_sources: &[String],
    probe: &dyn PathProbe,
    source_id: &str,
    path: Option<&str>,
) -> Result<(), IpcError> {
    if source_id.is_empty() {
        return Err(err(IpcErrorCode::InvalidArgument, "sourceId must not be empty"));
    }
    if !known_sources.iter().any(|s| s == source_id) {
        return Err(err(IpcErrorCode::SourceNotFound, "source not found"));
    }
    if let Some(p) = path {
        let origin = source_origin(source_id);
        let shape_ok = match origin.kind {
            OriginKind::Windows => is_drive_absolute(p),
            OriginKind::Wsl => p.starts_with('/'),
        };
        if !shape_ok || !probe.is_readable(&origin, p) {
            return Err(err(IpcErrorCode::PathInvalid, "path is not valid"));
        }
    }
    store
        .set_source_root(source_id, path)
        .map_err(|_| err(IpcErrorCode::SettingsSaveFailed, "settings could not be saved"))
}

// ---- events and sanitization ----

fn sanitize_session(mut s: UnifiedSession) -> UnifiedSession {
    let t = &mut s.current_task;
    for f in [&mut t.title, &mut t.last_user_message, &mut t.last_assistant_message] {
        if let Some(v) = f.as_mut() {
            *v = sanitize(v, Field::Text);
        }
    }
    if let Some(a) = t.active_tool.as_mut() {
        a.input = sanitize(&a.input, Field::Text);
    }
    s
}

/// Redacts and truncates every body field of a detail payload before it leaves the process.
pub fn sanitize_detail(mut d: Detail) -> Detail {
    d.sessions = d.sessions.into_iter().map(sanitize_session).collect();
    d
}

/// `projects_changed` event.
pub fn ev_projects_changed(upserts: Vec<ProjectActivity>, removes: Vec<String>, order: Vec<String>) -> Ev {
    Ev::ProjectsChanged { upserts, removes, order }
}

/// `project_detail_changed` event.
pub fn ev_project_detail_changed(key: &str) -> Ev {
    Ev::ProjectDetailChanged { key: key.to_string() }
}

/// `health_changed` event.
pub fn ev_health_changed(sources: Vec<SourceHealth>) -> Ev {
    Ev::HealthChanged { sources }
}
