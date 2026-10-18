use crate::agy::AgyFile;
use crate::claude;
use crate::codex::CodexFile;
use crate::model::Agent;
use crate::session::SessionState;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(serde::Deserialize)]
pub(super) struct Probe<'a> {
    #[serde(rename = "type", borrow, default)]
    pub(super) ty: Option<std::borrow::Cow<'a, str>>,
}

pub(super) enum Parser {
    Claude {
        sessions: BTreeMap<String, SessionState>,
        primary: Option<String>,
        path_agent: Option<String>,
        file_sid: Option<String>,
        mixed: bool,
    },
    Codex(Box<CodexFile>),
    Antigravity(Box<AgyFile>),
}

impl Parser {
    pub(super) fn new(agent: Agent, path: &Path) -> Self {
        match agent {
            Agent::ClaudeCode => Parser::Claude {
                sessions: BTreeMap::new(),
                primary: None,
                path_agent: claude::path_agent_id(path),
                file_sid: None,
                mixed: false,
            },
            Agent::Codex => Parser::Codex(Box::default()),
            Agent::Antigravity => Parser::Antigravity(Box::default()),
        }
    }

    pub(super) fn sessions(&self) -> Vec<&SessionState> {
        match self {
            Parser::Claude { sessions, .. } => sessions.values().collect(),
            Parser::Codex(f) => f.session.iter().collect(),
            Parser::Antigravity(f) => f.session.iter().collect(),
        }
    }

    pub(super) fn sessions_mut(&mut self) -> Vec<&mut SessionState> {
        match self {
            Parser::Claude { sessions, .. } => sessions.values_mut().collect(),
            Parser::Codex(f) => f.session.iter_mut().collect(),
            Parser::Antigravity(f) => f.session.iter_mut().collect(),
        }
    }

    pub(super) fn wants(&self, ty: Option<&str>) -> bool {
        match self {
            // A line whose `type` cannot be read is fully parsed (the reader already capped its size).
            Parser::Claude { .. } => ty.is_none_or(claude::wants_type),
            Parser::Codex(_) => matches!(ty, Some("session_meta" | "event_msg" | "response_item" | "turn_context")),
            Parser::Antigravity(_) => false,
        }
    }

    /// The session that receives file-level diagnostics.
    pub(super) fn primary_mut(&mut self) -> Option<&mut SessionState> {
        match self {
            Parser::Claude { sessions, primary, .. } => primary.as_ref().and_then(|p| sessions.get_mut(p)),
            Parser::Codex(f) => f.session.as_mut(),
            Parser::Antigravity(f) => f.session.as_mut(),
        }
    }

    pub(super) fn apply(&mut self, line_no: Option<u64>, v: &Value) {
        match self {
            Parser::Claude { sessions, primary, path_agent, file_sid, mixed } => {
                if let Some(sid) = claude::record_session_id(v) {
                    match file_sid {
                        None => *file_sid = Some(sid.to_string()),
                        // A foreign sessionId: the record is excluded from the file's session.
                        Some(first) if first != sid => {
                            if !*mixed {
                                *mixed = true;
                                if let Some(p) = primary.as_ref().and_then(|p| sessions.get_mut(p)) {
                                    p.diag("mixed_session_ids");
                                    p.degraded = true;
                                }
                            }
                            return;
                        }
                        Some(_) => {}
                    }
                }
                if let Some(native) = claude::apply(sessions, v, path_agent.as_deref()) {
                    if primary.is_none() {
                        *primary = Some(native);
                        if *mixed {
                            if let Some(p) = primary.as_ref().and_then(|p| sessions.get_mut(p)) {
                                p.diag("mixed_session_ids");
                                p.degraded = true;
                            }
                        }
                    }
                }
            }
            Parser::Codex(f) => f.apply(line_no, v),
            Parser::Antigravity(_) => {}
        }
    }
}
