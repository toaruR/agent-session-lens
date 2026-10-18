//! Claude Code adapter (jsonl-v2, architecture-v3 §2.1).

use crate::model::SessionKind;
use crate::session::{SessionState, TurnEnd};
use crate::util::{parse_ts, str_at, tool_input_summary};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

pub const AGENT_PREFIX: &str = "::agent:";

/// Strips leading auto-inserted `<tag>…</tag>` blocks. Returns `None` when nothing human remains.
pub fn strip_auto_tags(text: &str) -> Option<String> {
    let mut s = text.trim_start();
    while s.starts_with('<') {
        let name: String = s[1..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
        if name.is_empty() {
            break;
        }
        let close = format!("</{name}>");
        match s.find(&close) {
            Some(pos) => s = s[pos + close.len()..].trim_start(),
            None => return None,
        }
    }
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

fn between<'a>(s: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = s.find(open)? + open.len();
    let end = s[start..].find(close)? + start;
    Some(s[start..end].trim())
}

/// Extracts the human-authored part of a user text block.
pub fn human_text(text: &str) -> Option<String> {
    if let Some(name) = between(text, "<command-name>", "</command-name>") {
        let args = between(text, "<command-args>", "</command-args>").unwrap_or("");
        let joined = format!("{name} {args}");
        return Some(joined.trim().to_string());
    }
    strip_auto_tags(text)
}

/// `…/subagents/agent-<agentId>.jsonl` -> `<agentId>`.
pub fn path_agent_id(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let aid = name.strip_prefix("agent-")?.strip_suffix(".jsonl")?;
    let in_subagents = path.parent()?.file_name()?.to_str()? == "subagents";
    (in_subagents && !aid.is_empty()).then(|| aid.to_string())
}

pub fn record_session_id(v: &Value) -> Option<&str> {
    str_at(v, &["sessionId"])
}

fn session_identity(v: &Value, path_agent: Option<&str>) -> Option<(String, SessionKind, Option<String>)> {
    let sid = str_at(v, &["sessionId"])?;
    if let Some(aid) = path_agent {
        return Some((format!("{sid}{AGENT_PREFIX}{aid}"), SessionKind::Subagent, Some(sid.to_string())));
    }
    let sidechain = v.get("isSidechain").and_then(Value::as_bool).unwrap_or(false);
    match (sidechain, str_at(v, &["agentId"])) {
        (true, Some(aid)) => Some((format!("{sid}{AGENT_PREFIX}{aid}"), SessionKind::Subagent, Some(sid.to_string()))),
        _ => Some((sid.to_string(), SessionKind::TopLevel, None)),
    }
}

/// Applies one decoded record. Returns the native id of the session it touched.
pub fn apply(sessions: &mut BTreeMap<String, SessionState>, v: &Value, path_agent: Option<&str>) -> Option<String> {
    let ty = str_at(v, &["type"])?;
    if !wants_type(ty) {
        return None;
    }
    let (native, kind, parent) = session_identity(v, path_agent)?;
    let s = sessions.entry(native.clone()).or_insert_with(|| {
        let mut s = SessionState::new(native.clone(), kind);
        s.parent_native = parent;
        s
    });
    let ts = parse_ts(v.get("timestamp"));
    if let Some(cwd) = str_at(v, &["cwd"]) {
        s.cwd = Some(cwd.to_string());
    }
    if let Some(ver) = str_at(v, &["version"]) {
        s.writer_version = Some(ver.to_string());
    }

    // Only activity events move lastActivityAt (architecture-v5 section 4).
    match ty {
        "custom-title" => set_title(s, str_at(v, &["customTitle"]), 3),
        "ai-title" => set_title(s, str_at(v, &["aiTitle"]), 2),
        "user" => {
            if !is_meta(v) {
                s.touch(ts);
                apply_user(s, v, ts);
            }
        }
        "assistant" => {
            s.touch(ts);
            apply_assistant(s, v, ts);
        }
        _ => {
            if str_at(v, &["subtype"]) == Some("api_error") {
                s.touch(ts);
                s.end_turn(TurnEnd::Error, ts, "api_error");
            }
        }
    }
    Some(native)
}

/// Record types the engine must fully parse (everything else is skipped after the `type` probe).
pub fn wants_type(ty: &str) -> bool {
    matches!(ty, "user" | "assistant" | "system" | "custom-title" | "ai-title")
}

fn is_meta(v: &Value) -> bool {
    v.get("isMeta").and_then(Value::as_bool).unwrap_or(false)
}

/// Title order: custom-title (3) > ai-title (2) > first human message (rank 0, set by `human_message`).
fn set_title(s: &mut SessionState, title: Option<&str>, rank: u8) {
    let Some(t) = title.map(str::trim).filter(|t| !t.is_empty()) else { return };
    if s.title_rank <= rank {
        s.title = Some(crate::redact::shape(t, crate::redact::MAX_MESSAGE_CHARS));
        s.title_rank = rank;
    }
}

fn apply_user(s: &mut SessionState, v: &Value, ts: Option<chrono::DateTime<chrono::Utc>>) {    let content = &v["message"]["content"];
    let mut texts: Vec<&str> = Vec::new();
    // tool_result payload text: checked for the interruption marker only, never human text.
    let mut result_texts: Vec<&str> = Vec::new();
    match content {
        Value::String(t) => texts.push(t),
        Value::Array(blocks) => {
            for b in blocks {
                match str_at(b, &["type"]) {
                    Some("tool_result") => {
                        if let Some(id) = str_at(b, &["tool_use_id"]) {
                            s.tool_end(id);
                        }
                        match &b["content"] {
                            Value::String(t) => result_texts.push(t),
                            Value::Array(inner) => {
                                result_texts.extend(inner.iter().filter(|x| str_at(x, &["type"]) == Some("text")).filter_map(|x| str_at(x, &["text"])));
                            }
                            _ => {}
                        }
                        if b.get("is_error").and_then(Value::as_bool).unwrap_or(false) {
                            s.diag("tool_error");
                        }
                    }
                    Some("text") => {
                        if let Some(t) = str_at(b, &["text"]) {
                            texts.push(t);
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => s.diag("unknown_user_content"),
    }
    if result_texts.iter().any(|t| t.trim_start().starts_with("[Request interrupted by user")) {
        s.end_turn(TurnEnd::Aborted, ts, "interrupted by user");
        return;
    }
    let mut human: Vec<String> = Vec::new();
    for t in texts {
        if t.trim_start().starts_with("[Request interrupted by user") {
            s.end_turn(TurnEnd::Aborted, ts, "interrupted by user");
            return;
        }
        if let Some(h) = human_text(t) {
            human.push(h);
        }
    }
    if !human.is_empty() {
        s.human_message(&human.join("\n"), ts);
    }
}

fn apply_assistant(s: &mut SessionState, v: &Value, ts: Option<chrono::DateTime<chrono::Utc>>) {
    let msg = &v["message"];
    if let Some(blocks) = msg["content"].as_array() {
        for b in blocks {
            match str_at(b, &["type"]) {
                Some("text") => {
                    if let Some(t) = str_at(b, &["text"]) {
                        if !t.trim().is_empty() {
                            s.assistant_text(t);
                        }
                    }
                }
                Some("tool_use") => {
                    let id = str_at(b, &["id"]).unwrap_or_default().to_string();
                    let name = str_at(b, &["name"]).unwrap_or("tool");
                    s.tool_start(id, name, &tool_input_summary(&b["input"]), ts);
                }
                // thinking / redacted_thinking / server tool blocks are not display text.
                _ => {}
            }
        }
    }
    if str_at(msg, &["stop_reason"]) == Some("end_turn") {
        s.end_turn(TurnEnd::Complete, ts, "stop_reason=end_turn");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_leading_auto_tags() {
        assert_eq!(strip_auto_tags("<ide_opened_file>x</ide_opened_file>\n do it").as_deref(), Some("do it"));
        assert_eq!(strip_auto_tags("<system-reminder>a</system-reminder>"), None);
        assert_eq!(strip_auto_tags("<local-command-caveat>unterminated"), None);
        assert_eq!(strip_auto_tags("plain <b>text</b>").as_deref(), Some("plain <b>text</b>"));
    }

    #[test]
    fn slash_command_becomes_human_text() {
        let t = "<command-message>gougi</command-message>\n<command-name>/gougi</command-name>\n<command-args>docs\\a.md</command-args>";
        assert_eq!(human_text(t).as_deref(), Some("/gougi docs\\a.md"));
    }
}
