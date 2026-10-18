//! Codex CLI adapter (rollout-v1, architecture-v3 §2.2).

use crate::model::SessionKind;
use crate::session::{SessionState, TurnEnd};
use crate::util::{parse_ts, str_at, tool_input_summary};
use serde_json::Value;

/// Per-file parsing context. One rollout file holds exactly one session.
#[derive(Debug, Default, Clone)]
pub struct CodexFile {
    pub session: Option<SessionState>,
    metas_seen: u32,
    history_start: Option<u64>,
}

fn is_auto_user_block(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with('<') || t.starts_with("# AGENTS.md instructions")
}

fn parent_of(p: &Value) -> Option<String> {
    str_at(p, &["parent_thread_id"])
        .or_else(|| str_at(p, &["source", "subagent", "thread_spawn", "parent_thread_id"]))
        .map(str::to_string)
}

/// Returns the kind and whether `thread_source` was a value outside the known set.
fn kind_of(p: &Value) -> (SessionKind, bool) {
    let forked = p.get("forked_from_id").is_some_and(|f| !f.is_null());
    match str_at(p, &["thread_source"]) {
        Some("guardian_review") => (SessionKind::Automated, false),
        Some("subagent") => (SessionKind::Subagent, false),
        Some("user") | None if forked => (SessionKind::Fork, false),
        Some("user") | None => (SessionKind::TopLevel, false),
        Some(_) => (SessionKind::TopLevel, true),
    }
}

impl CodexFile {
    /// Applies one decoded record. `line_no` is `None` for lines read from the tail of a large file.
    pub fn apply(&mut self, line_no: Option<u64>, v: &Value) {
        let ty = str_at(v, &["type"]).unwrap_or_default();
        let p = &v["payload"];
        let ts = parse_ts(v.get("timestamp"));

        if ty == "session_meta" {
            self.metas_seen += 1;
            if self.metas_seen == 1 {
                let id = str_at(p, &["id"]).or_else(|| str_at(p, &["session_id"])).unwrap_or("unknown");
                let (kind, unknown_source) = kind_of(p);
                let mut s = SessionState::new(id, kind);
                if unknown_source {
                    s.diag("unknown_thread_source");
                }
                s.parent_native = parent_of(p);
                s.cwd = str_at(p, &["cwd"]).map(str::to_string);
                s.writer_version = str_at(p, &["cli_version"]).map(str::to_string);
                // session_meta is not activity: it only anchors startedAt, so a meta-only file falls back to the file mtime.
                s.started_at = parse_ts(p.get("timestamp")).or(ts);
                self.history_start = p.get("subagent_history_start_ordinal").and_then(Value::as_u64);
                self.session = Some(s);
            } else if self.metas_seen == 2 && self.history_start.is_none() {
                if let Some(s) = self.session.as_mut() {
                    s.diag("fork_boundary_unknown");
                    s.degraded = true;
                }
            }
            return;
        }

        let Some(s) = self.session.as_mut() else {
            return;
        };
        // Records before the ordinal were inherited from the fork source.
        if let (Some(start), Some(n)) = (self.history_start, line_no) {
            if n < start {
                return;
            }
        }
        s.touch(ts);

        match ty {
            "event_msg" => match str_at(p, &["type"]) {
                Some("task_started") => s.start_turn(ts, "task_started"),
                Some("task_complete") => {
                    if p.get("error").is_some_and(|e| !e.is_null()) {
                        s.end_turn(TurnEnd::Error, ts, "task_complete with error");
                    } else {
                        s.end_turn(TurnEnd::Complete, ts, "task_complete");
                    }
                }
                Some("turn_aborted") => {
                    let reason = format!("turn_aborted: {}", str_at(p, &["reason"]).unwrap_or("unknown"));
                    s.end_turn(TurnEnd::Aborted, ts, &reason);
                }
                // Message bodies come from response_item only (no double ingestion).
                _ => {}
            },
            "response_item" => match str_at(p, &["type"]) {
                Some("message") => {
                    let texts: Vec<&str> = p["content"]
                        .as_array()
                        .map(|a| a.iter().filter_map(|b| str_at(b, &["text"])).collect())
                        .unwrap_or_default();
                    match str_at(p, &["role"]) {
                        Some("user") => {
                            let human: Vec<&str> = texts.into_iter().filter(|t| !is_auto_user_block(t) && !t.trim().is_empty()).collect();
                            if !human.is_empty() {
                                s.human_message(&human.join("\n"), ts);
                            }
                        }
                        Some("assistant") => {
                            let joined = texts.join("\n");
                            if !joined.trim().is_empty() {
                                s.assistant_text(&joined);
                            }
                        }
                        _ => {}
                    }
                }
                Some("function_call") | Some("custom_tool_call") | Some("local_shell_call") => {
                    let id = str_at(p, &["call_id"]).unwrap_or_default().to_string();
                    let name = str_at(p, &["name"]).unwrap_or("tool");
                    let input = p.get("arguments").or_else(|| p.get("input")).unwrap_or(&Value::Null);
                    s.tool_start(id, name, &tool_input_summary(input), ts);
                }
                Some("function_call_output") | Some("custom_tool_call_output") | Some("local_shell_call_output") => {
                    if let Some(id) = str_at(p, &["call_id"]) {
                        s.tool_end(id);
                    }
                }
                _ => {}
            },
            "turn_context" if s.cwd.is_none() => {
                s.cwd = str_at(p, &["cwd"]).map(str::to_string);
            }
            _ => {}
        }
    }
}
