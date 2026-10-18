//! Per-session turn state machine and status evaluation (architecture-v3 §5.3).

use crate::consts::{RECENT_WRITE_SECS, STALE_SECS};
use crate::model::{Confidence, SessionKind, Status, StatusValue};
use crate::redact;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;

pub const IDLE_SECS: i64 = STALE_SECS as i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnEnd {
    Complete,
    Aborted,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenTool {
    pub call_id: String,
    pub name: String,
    pub input: String,
    pub started_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct SessionState {
    pub native_id: String,
    pub kind: SessionKind,
    pub parent_native: Option<String>,
    pub writer_version: Option<String>,
    pub cwd: Option<String>,

    pub title: Option<String>,
    /// Source rank of `title`: 0 = first human message, 2 = ai-title, 3 = custom-title.
    pub title_rank: u8,
    pub last_user: Option<String>,
    pub last_assistant: Option<String>,

    /// `Some(None)` = a turn is open; `Some(Some(end))` = last turn ended; `None` = no boundary seen.
    pub turn: Option<Option<TurnEnd>>,
    pub turn_changed_at: Option<DateTime<Utc>>,
    pub turn_reason: String,
    pub open_tools: Vec<OpenTool>,
    /// A session-level error (e.g. API error) not followed by later activity.
    pub pending_error: bool,
    /// The newest record could not be decoded.
    pub last_record_unreadable: bool,
    /// No complete line was found within the tail scan window (architecture-v5 §3.1 rule 1).
    pub tail_empty: bool,
    /// Lowers confidence (e.g. lines were lost, fork boundary unknown).
    pub degraded: bool,
    pub truncated_scan: bool,

    pub step_count: u64,
    pub tool_calls: u64,
    pub started_at: Option<DateTime<Utc>>,
    pub last_event_at: Option<DateTime<Utc>>,
    pub diagnostics: BTreeMap<String, u64>,
}

impl SessionState {
    pub fn new(native_id: impl Into<String>, kind: SessionKind) -> Self {
        Self {
            native_id: native_id.into(),
            kind,
            parent_native: None,
            writer_version: None,
            cwd: None,
            title: None,
            title_rank: 0,
            last_user: None,
            last_assistant: None,
            turn: None,
            turn_changed_at: None,
            turn_reason: String::new(),
            open_tools: Vec::new(),
            pending_error: false,
            last_record_unreadable: false,
            tail_empty: false,
            degraded: false,
            truncated_scan: false,
            step_count: 0,
            tool_calls: 0,
            started_at: None,
            last_event_at: None,
            diagnostics: BTreeMap::new(),
        }
    }

    pub fn diag(&mut self, code: &str) {
        *self.diagnostics.entry(code.to_string()).or_default() += 1;
    }

    pub fn touch(&mut self, ts: Option<DateTime<Utc>>) {
        if let Some(ts) = ts {
            if self.started_at.is_none_or(|s| ts < s) {
                self.started_at = Some(ts);
            }
            if self.last_event_at.is_none_or(|l| ts > l) {
                self.last_event_at = Some(ts);
            }
        }
        self.last_record_unreadable = false;
    }

    /// Any decoded activity after an error clears it.
    pub fn activity(&mut self) {
        self.pending_error = false;
    }

    pub fn human_message(&mut self, text: &str, ts: Option<DateTime<Utc>>) {
        let shaped = redact::shape(text, redact::MAX_MESSAGE_CHARS);
        if self.title.is_none() {
            self.title = Some(shaped.clone());
        }
        self.last_user = Some(shaped);
        self.step_count += 1;
        self.activity();
        self.start_turn(ts, "human message");
    }

    pub fn assistant_text(&mut self, text: &str) {
        self.last_assistant = Some(redact::shape(text, redact::MAX_MESSAGE_CHARS));
        self.step_count += 1;
        self.activity();
    }

    pub fn start_turn(&mut self, ts: Option<DateTime<Utc>>, reason: &str) {
        if self.turn != Some(None) {
            self.turn = Some(None);
            self.turn_changed_at = ts;
            self.turn_reason = reason.to_string();
        }
    }

    pub fn end_turn(&mut self, end: TurnEnd, ts: Option<DateTime<Utc>>, reason: &str) {
        self.turn = Some(Some(end));
        self.turn_changed_at = ts;
        self.turn_reason = reason.to_string();
        self.open_tools.clear();
        self.pending_error = end == TurnEnd::Error;
    }

    pub fn tool_start(&mut self, call_id: String, name: &str, input: &str, ts: Option<DateTime<Utc>>) {
        self.tool_calls += 1;
        self.activity();
        self.open_tools.retain(|t| t.call_id != call_id);
        self.open_tools.push(OpenTool {
            call_id,
            name: name.to_string(),
            input: redact::shape(input, redact::MAX_TOOL_INPUT_CHARS),
            started_at: ts,
        });
        // A tool call proves a turn is in progress even if its start was not read.
        self.start_turn(ts, "tool call");
    }

    pub fn tool_end(&mut self, call_id: &str) {
        self.activity();
        self.open_tools.retain(|t| t.call_id != call_id);
    }

    pub fn last_active(&self, file_mtime: Option<DateTime<Utc>>) -> Option<DateTime<Utc>> {
        self.last_event_at.or(file_mtime)
    }

    /// Evaluates the §3.1 rule table; falls back to the file mtime for the last activity.
    pub fn evaluate(&self, now: DateTime<Utc>, file_mtime: Option<DateTime<Utc>>) -> Status {
        let last = self.last_active(file_mtime);
        evaluate_at(now.timestamp_millis(), last.map(|l| l.timestamp_millis()), self)
    }
}

/// Pure §3.1 decision: `now_ms` is a parameter, no clock is read.
pub fn evaluate(now_ms: i64, session: &SessionState) -> Status {
    evaluate_at(now_ms, session.last_event_at.map(|l| l.timestamp_millis()), session)
}

/// `age_s = floor((now_ms - t)/1000)`; unknown activity time counts as infinitely old.
fn evaluate_at(now_ms: i64, last_ms: Option<i64>, s: &SessionState) -> Status {
    let age = last_ms.map(|t| (now_ms - t).div_euclid(1000)).unwrap_or(i64::MAX);
    let stale = age >= STALE_SECS as i64;
    let since = s
        .turn_changed_at
        .map(|t| t.to_rfc3339())
        .or_else(|| last_ms.and_then(DateTime::<Utc>::from_timestamp_millis).map(|t| t.to_rfc3339()));
    let degrade = |c: Confidence| if s.degraded { Confidence::Low } else { c };
    let mk = |value, reason: String, confidence, stale| Status {
        value,
        reason,
        confidence,
        stale,
        auto_review: false,
        since: since.clone(),
    };

    // Rule 1
    if s.last_record_unreadable || s.tail_empty {
        return mk(StatusValue::Unknown, "latest record unreadable".into(), Confidence::Low, false);
    }
    // Rule 2
    if s.pending_error {
        let value = if stale { StatusValue::Idle } else { StatusValue::Error };
        return mk(value, format!("error: {}", s.turn_reason), degrade(Confidence::High), false);
    }
    match s.turn {
        // Rule 3
        Some(None) => {
            let conf = if stale {
                Confidence::Low
            } else if !s.open_tools.is_empty() || age < RECENT_WRITE_SECS as i64 {
                Confidence::High
            } else {
                Confidence::Low
            };
            let reason = match s.open_tools.last() {
                Some(t) => format!("unresolved tool call: {}", t.name),
                None => format!("turn open ({})", s.turn_reason),
            };
            mk(StatusValue::Running, reason, degrade(conf), stale)
        }
        // Rule 4
        Some(Some(end)) => {
            let value = if stale { StatusValue::Idle } else { StatusValue::WaitingInput };
            let reason = match end {
                TurnEnd::Complete => format!("turn complete ({})", s.turn_reason),
                TurnEnd::Aborted => format!("turn aborted ({})", s.turn_reason),
                TurnEnd::Error => format!("turn error ({})", s.turn_reason),
            };
            mk(value, reason, degrade(Confidence::High), false)
        }
        // Rule 5
        None => {
            let value = if stale { StatusValue::Idle } else { StatusValue::Running };
            mk(value, "no turn boundary observed".into(), Confidence::Low, false)
        }
    }
}

/// `updating` = any status is `running` with `stale=false`; low-only => confidence low.
pub fn aggregate_updating<'a>(statuses: impl IntoIterator<Item = &'a Status>) -> (bool, Confidence) {
    let mut any = false;
    let mut high = false;
    for st in statuses {
        if st.value == StatusValue::Running && !st.stale {
            any = true;
            high |= st.confidence == Confidence::High;
        }
    }
    (any, if any && !high { Confidence::Low } else { Confidence::High })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn t0() -> DateTime<Utc> {
        "2026-10-17T00:00:00Z".parse().unwrap()
    }

    fn fresh() -> SessionState {
        let mut s = SessionState::new("a", SessionKind::TopLevel);
        s.touch(Some(t0()));
        s
    }

    #[test]
    fn unreadable_latest_record_is_unknown() {
        let mut s = fresh();
        s.human_message("hi", Some(t0()));
        s.last_record_unreadable = true;
        assert_eq!(s.evaluate(t0(), None).value, StatusValue::Unknown);
    }

    #[test]
    fn error_outranks_open_turn() {
        let mut s = fresh();
        s.human_message("hi", Some(t0()));
        s.end_turn(TurnEnd::Error, Some(t0()), "api_error");
        assert_eq!(s.evaluate(t0(), None).value, StatusValue::Error);
    }

    #[test]
    fn error_cleared_by_later_activity() {
        let mut s = fresh();
        s.human_message("hi", Some(t0()));
        s.pending_error = true;
        s.assistant_text("retry ok");
        assert_ne!(s.evaluate(t0(), None).value, StatusValue::Error);
    }

    #[test]
    fn long_running_tool_goes_stale_not_idle() {
        let mut s = fresh();
        s.human_message("build", Some(t0()));
        s.tool_start("c1".into(), "Bash", "cargo build", Some(t0()));
        let st = s.evaluate(t0() + Duration::seconds(IDLE_SECS + 5), None);
        assert_eq!(st.value, StatusValue::Running);
        assert!(st.stale);
        assert_eq!(st.confidence, Confidence::Low);
    }

    #[test]
    fn open_turn_with_tool_is_high_confidence() {
        let mut s = fresh();
        s.tool_start("c1".into(), "Bash", "ls", Some(t0()));
        let st = s.evaluate(t0() + Duration::seconds(120), None);
        assert_eq!((st.value, st.confidence), (StatusValue::Running, Confidence::High));
    }

    #[test]
    fn open_turn_without_tool_and_quiet_is_low_confidence() {
        let mut s = fresh();
        s.human_message("hi", Some(t0()));
        let st = s.evaluate(t0() + Duration::seconds(120), None);
        assert_eq!((st.value, st.confidence), (StatusValue::Running, Confidence::Low));
    }

    #[test]
    fn completed_turn_waits_then_idles() {
        let mut s = fresh();
        s.human_message("hi", Some(t0()));
        s.end_turn(TurnEnd::Complete, Some(t0()), "end_turn");
        assert_eq!(s.evaluate(t0() + Duration::seconds(10), None).value, StatusValue::WaitingInput);
        assert_eq!(s.evaluate(t0() + Duration::seconds(IDLE_SECS), None).value, StatusValue::Idle);
    }

    #[test]
    fn no_boundary_is_low_confidence() {
        let mut s = fresh();
        s.touch(Some(t0()));
        let st = s.evaluate(t0() + Duration::seconds(5), None);
        assert_eq!((st.value, st.confidence), (StatusValue::Running, Confidence::Low));
        assert_eq!(s.evaluate(t0() + Duration::seconds(IDLE_SECS), None).value, StatusValue::Idle);
    }

    #[test]
    fn degraded_lowers_confidence() {
        let mut s = fresh();
        s.human_message("hi", Some(t0()));
        s.end_turn(TurnEnd::Complete, Some(t0()), "end_turn");
        s.degraded = true;
        assert_eq!(s.evaluate(t0(), None).confidence, Confidence::Low);
    }
}
