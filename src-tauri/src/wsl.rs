//! Conversion of helper-evaluated WSL sessions back into aggregator inputs.

use chrono::{DateTime, Utc};
use lens_core::aggregator::SessionInput;
use lens_core::distro;
use lens_core::model::v5;
use lens_core::model::{Completeness, Confidence, StatusValue};
use lens_core::session::{OpenTool, SessionState, TurnEnd};

pub fn parse_ts(s: &Option<String>) -> Option<DateTime<Utc>> {
    s.as_deref().and_then(|t| DateTime::parse_from_rfc3339(t).ok()).map(|d| d.with_timezone(&Utc))
}

/// Rebuilds the aggregator input from a helper-evaluated session. The Aggregator re-evaluates
/// status from this state, so the turn boundary is inferred from the helper's status.
pub fn wsl_input(s: &v5::UnifiedSession) -> SessionInput {
    let mut st = SessionState::new(s.native_id.clone(), s.hierarchy.kind);
    let prefix = format!("{}:", s.source_id);
    st.parent_native = s.hierarchy.parent_uid.as_deref().map(|p| p.strip_prefix(&prefix).unwrap_or(p).to_string());
    st.title = s.current_task.title.clone();
    st.last_user = s.current_task.last_user_message.clone();
    st.last_assistant = s.current_task.last_assistant_message.clone();
    st.last_event_at = parse_ts(&s.last_activity_at);
    let r = &s.status.reason;
    st.turn_reason = match (r.find('('), r.rfind(')')) {
        (Some(a), Some(b)) if a < b => r[a + 1..b].to_string(),
        _ => r.clone(),
    };
    if let Some(t) = &s.current_task.active_tool {
        st.open_tools.push(OpenTool {
            call_id: t.call_id.clone().unwrap_or_default(),
            name: t.name.clone(),
            input: t.input.clone(),
            started_at: parse_ts(&t.started_at),
        });
    }
    match s.status.value {
        StatusValue::Running => st.turn = Some(None),
        StatusValue::WaitingInput => st.turn = Some(Some(TurnEnd::Complete)),
        StatusValue::Error => {
            st.turn = Some(Some(TurnEnd::Error));
            st.pending_error = true;
        }
        StatusValue::Unknown => st.last_record_unreadable = true,
        StatusValue::Idle => {}
    }
    st.degraded = s.status.confidence == Confidence::Low || s.health.completeness == Completeness::Partial;
    st.diagnostics = s.health.diagnostics.iter().map(|d| (d.code.clone(), d.count)).collect();
    SessionInput {
        source_id: s.source_id.clone(),
        agent: s.agent,
        origin: s.origin.clone(),
        state: st,
        file_mtime: None,
        projects: s.projects.clone(),
    }
}


/// `distro::Launcher` that does nothing: the per-distro `Supervisor` owns helper launches.
pub struct NoLaunch;
impl distro::Launcher for NoLaunch {
    fn launch(&self, _distro: &str) {}
}


#[cfg(test)]
mod tests {
    use super::*;

    fn session(status: &str, reason: &str, confidence: &str, completeness: &str, extra_task: &str) -> v5::UnifiedSession {
        let json = format!(
            r#"{{"sessionUid":"wsl:Ubuntu:claude:s1","nativeId":"s1","agent":"claude_code","sourceId":"wsl:Ubuntu:claude","origin":{{"kind":"wsl","distro":"Ubuntu"}},"lastActivityAt":"2026-01-02T03:04:05Z","lastActivitySource":"event","projects":[],"status":{{"value":"{status}","reason":"{reason}","confidence":"{confidence}","stale":false,"autoReview":false}},"currentTask":{{"title":"T","lastUserMessage":"u","lastAssistantMessage":"a"{extra_task}}},"hierarchy":{{"parentUid":"wsl:Ubuntu:claude:p0","childUids":[],"kind":"subagent"}},"health":{{"completeness":"{completeness}","diagnostics":[{{"code":"d1","count":2}}]}}}}"#
        );
        serde_json::from_str(&json).expect("valid session json")
    }

    #[test]
    pub fn parse_ts_accepts_rfc3339_and_rejects_garbage() {
        let t = parse_ts(&Some("2026-01-02T03:04:05+09:00".into())).expect("parsed");
        assert_eq!(t.to_rfc3339(), "2026-01-01T18:04:05+00:00");
        assert!(parse_ts(&Some("not a time".into())).is_none());
        assert!(parse_ts(&None).is_none());
    }

    #[test]
    pub fn wsl_input_running_maps_open_turn_and_tool() {
        let tool = r#","activeTool":{"name":"Bash","input":"ls","callId":"c1","startedAt":"2026-01-02T03:04:00Z"}"#;
        let s = session("running", "tool running (tool_open)", "high", "complete", tool);
        let i = wsl_input(&s);
        assert_eq!(i.state.native_id, "s1");
        assert_eq!(i.state.parent_native.as_deref(), Some("p0"));
        assert_eq!(i.state.title.as_deref(), Some("T"));
        assert_eq!(i.state.turn_reason, "tool_open");
        assert!(matches!(i.state.turn, Some(None)));
        assert_eq!(i.state.open_tools.len(), 1);
        assert_eq!(i.state.open_tools[0].call_id, "c1");
        assert!(i.state.open_tools[0].started_at.is_some());
        assert!(!i.state.degraded);
        assert_eq!(i.state.diagnostics.get("d1"), Some(&2));
        assert_eq!(i.source_id, "wsl:Ubuntu:claude");
    }

    #[test]
    pub fn wsl_input_status_to_turn_boundary() {
        let w = wsl_input(&session("waiting_input", "plain", "high", "complete", ""));
        assert!(matches!(w.state.turn, Some(Some(TurnEnd::Complete))));
        assert_eq!(w.state.turn_reason, "plain");
        assert!(w.state.open_tools.is_empty());

        let e = wsl_input(&session("error", "x", "high", "complete", ""));
        assert!(matches!(e.state.turn, Some(Some(TurnEnd::Error))));
        assert!(e.state.pending_error);

        let u = wsl_input(&session("unknown", "x", "high", "complete", ""));
        assert!(u.state.last_record_unreadable);

        let idle = wsl_input(&session("idle", "x", "high", "complete", ""));
        assert!(idle.state.turn.is_none());
    }

    #[test]
    pub fn wsl_input_degraded_on_low_confidence_or_partial() {
        assert!(wsl_input(&session("idle", "x", "low", "complete", "")).state.degraded);
        assert!(wsl_input(&session("idle", "x", "high", "partial", "")).state.degraded);
        assert!(!wsl_input(&session("idle", "x", "high", "complete", "")).state.degraded);
    }
}

