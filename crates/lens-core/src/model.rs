//! Serializable snapshot model sent to the UI (architecture-v3 §4).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Agent {
    ClaudeCode,
    Codex,
    #[serde(rename = "agy")]
    Antigravity,
}

impl Agent {
    pub const ALL: [Agent; 3] = [Agent::ClaudeCode, Agent::Codex, Agent::Antigravity];
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Completeness {
    Complete,
    Partial,
    Unsupported,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusValue {
    Running,
    WaitingInput,
    Idle,
    Error,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    High,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    TopLevel,
    Subagent,
    Fork,
    Automated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Attribution {
    GitRoot,
    RawPath,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeSource {
    Event,
    FileMtime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceState {
    Ok,
    NotInstalled,
    Empty,
    AccessDenied,
    UnsupportedSchema,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRef {
    pub key: String,
    pub name: String,
    pub display_path: String,
    pub attribution: Attribution,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub value: StatusValue,
    pub reason: String,
    pub confidence: Confidence,
    pub stale: bool,
    pub auto_review: bool,
    pub since: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveTool {
    pub name: String,
    pub input: String,
    pub call_id: Option<String>,
    pub started_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentTask {
    pub title: Option<String>,
    pub last_user_message: Option<String>,
    pub last_assistant_message: Option<String>,
    pub active_tool: Option<ActiveTool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub step_count: u64,
    pub tool_calls: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hierarchy {
    pub parent_uid: Option<String>,
    pub child_uids: Vec<String>,
    pub kind: SessionKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timestamps {
    pub started_at: Option<String>,
    pub last_active_at: Option<String>,
    pub last_active_source: TimeSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub code: String,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    pub completeness: Completeness,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedSession {
    pub session_uid: String,
    pub native_id: String,
    pub agent: Agent,
    pub source_id: String,
    pub writer_version: Option<String>,
    pub projects: Vec<ProjectRef>,
    pub raw_paths: Vec<String>,
    pub status: Status,
    pub current_task: CurrentTask,
    pub metrics: Metrics,
    pub hierarchy: Hierarchy,
    pub timestamps: Timestamps,
    pub health: Health,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceHealth {
    pub source_id: String,
    pub agent: Agent,
    pub root_path: String,
    pub state: SourceState,
    pub detail: Option<String>,
    pub last_scan_at: Option<String>,
    pub files_tracked: u64,
    pub files_skipped_old: u64,
    pub sessions_total: u64,
    pub partial_sessions: u64,
    pub writer_versions: Vec<String>,
    pub top_diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub generated_at: String,
    pub sessions: Vec<UnifiedSession>,
    pub sources: Vec<SourceHealth>,
}

/// architecture-v5 section 7 data model (IPC and NDJSON). Coexists with the v3
/// snapshot types above until the aggregator and Tauri tasks replace them.
pub mod v5 {
    use super::{Agent, Attribution, Completeness, Confidence, Diagnostic, SessionKind, StatusValue, TimeSource};
    use serde::{Deserialize, Serialize};
    use std::collections::BTreeMap;

    pub const UNATTRIBUTED_KEY: &str = "unattributed";
    pub const CLAUDE_LOCAL: &str = "claude-local";
    pub const CODEX_LOCAL: &str = "codex-local";

    /// `${sourceId}:${nativeId}`
    pub fn session_uid(source_id: &str, native_id: &str) -> String {
        format!("{source_id}:{native_id}")
    }

    /// `wsl:<distro>:claude|codex`
    pub fn wsl_source_id(distro: &str, agent: Agent) -> String {
        let a = match agent {
            Agent::ClaudeCode => "claude",
            Agent::Codex => "codex",
            Agent::Antigravity => "agy",
        };
        format!("wsl:{distro}:{a}")
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum OriginKind {
        Windows,
        Wsl,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Origin {
        pub kind: OriginKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub distro: Option<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ProjectActivity {
        pub key: String,
        pub name: String,
        pub origin: Origin,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub last_updated_at: Option<String>,
        pub last_updated_source: TimeSource,
        pub updating: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub updating_confidence: Option<Confidence>,
        pub origin_available: bool,
        pub agents: Vec<Agent>,
        pub session_uids: Vec<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SessionProject {
        pub key: String,
        pub name: String,
        pub display_path: String,
        pub attribution: Attribution,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Status {
        pub value: StatusValue,
        pub reason: String,
        pub confidence: Confidence,
        pub stale: bool,
        pub auto_review: bool,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ActiveTool {
        pub name: String,
        pub input: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub call_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub started_at: Option<String>,
    }

    #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct CurrentTask {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub last_user_message: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub last_assistant_message: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub active_tool: Option<ActiveTool>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Hierarchy {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub parent_uid: Option<String>,
        pub child_uids: Vec<String>,
        pub kind: SessionKind,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Health {
        pub completeness: Completeness,
        pub diagnostics: Vec<Diagnostic>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct UnifiedSession {
        pub session_uid: String,
        pub native_id: String,
        pub agent: Agent,
        pub source_id: String,
        pub origin: Origin,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub last_activity_at: Option<String>,
        pub last_activity_source: TimeSource,
        pub projects: Vec<SessionProject>,
        pub status: Status,
        pub current_task: CurrentTask,
        pub hierarchy: Hierarchy,
        pub health: Health,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum SourceState {
        Ok,
        NotInstalled,
        Empty,
        AccessDenied,
        UnsupportedSchema,
        Error,
        DistroStopped,
        HelperUnavailable,
        HelperVersionMismatch,
        Unsupported,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SourceHealth {
        pub source_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub agent: Option<Agent>,
        pub origin: Origin,
        pub root_path: String,
        pub state: SourceState,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub last_scan_at: Option<String>,
        pub files_tracked: u64,
        pub sessions_total: u64,
        pub partial_sessions: u64,
        pub skipped_old_files: u64,
        pub top_diagnostics: Vec<Diagnostic>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Settings {
        pub version: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub helper_path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub disabled_distros: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub source_roots: Option<BTreeMap<String, String>>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum IpcErrorCode {
        NotReady,
        InvalidArgument,
        ProjectNotFound,
        DistroNotFound,
        SourceNotFound,
        PathInvalid,
        SettingsSaveFailed,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct IpcError {
        pub code: IpcErrorCode,
        pub message: String,
    }

    /// IPC command input. Variant order matters for untagged deserialization.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(untagged)]
    pub enum In {
        Key { key: String },
        Distro { distro: String, enabled: bool },
        #[serde(rename_all = "camelCase")]
        SourceRoot { source_id: String, path: Option<String> },
        Empty {},
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Detail {
        pub project: ProjectActivity,
        pub sessions: Vec<UnifiedSession>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(untagged)]
    pub enum Ev {
        ProjectsChanged {
            upserts: Vec<ProjectActivity>,
            removes: Vec<String>,
            order: Vec<String>,
        },
        ProjectDetailChanged {
            key: String,
        },
        HealthChanged {
            sources: Vec<SourceHealth>,
        },
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum HelperErrorCode {
        RootUnreadable,
        Internal,
    }

    /// NDJSON message from the WSL helper. Unknown fields are ignored.
    // Boxing SessionUpsert would change the public API used by tests; wire format is unaffected but keep as is.
    #[allow(clippy::large_enum_variant)]
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum Msg {
        #[serde(rename_all = "camelCase")]
        Hello {
            protocol: i64,
            helper_version: String,
            distro: String,
            home: String,
            now: String,
        },
        #[serde(rename_all = "camelCase")]
        SourceHealth { source_id: String, health: SourceHealth },
        SessionUpsert { session: UnifiedSession },
        #[serde(rename_all = "camelCase")]
        SessionRemove { session_uid: String },
        Heartbeat { now: String },
        Error { code: HelperErrorCode },
    }
}
