//! Architecture-v5 section 3.1 decision table as tests. Time is a parameter; no clock.

use chrono::{DateTime, Utc};
use lens_core::model::{Confidence, SessionKind, Status, StatusValue};
use lens_core::session::{aggregate_updating, evaluate, SessionState, TurnEnd};

fn t() -> DateTime<Utc> {
    "2026-10-17T00:00:00Z".parse().unwrap()
}

fn t_ms() -> i64 {
    t().timestamp_millis()
}

fn base() -> SessionState {
    let mut s = SessionState::new("a", SessionKind::TopLevel);
    s.touch(Some(t()));
    s
}

fn open_turn(tool: bool) -> SessionState {
    let mut s = base();
    s.human_message("go", Some(t()));
    if tool {
        s.tool_start("c1".into(), "Bash", "sleep", Some(t()));
    }
    s
}

fn ended(end: TurnEnd) -> SessionState {
    let mut s = base();
    s.human_message("go", Some(t()));
    s.end_turn(end, Some(t()), "x");
    s
}

fn at(s: &SessionState, age_s: i64) -> Status {
    evaluate(t_ms() + age_s * 1000, s)
}

fn tuple(st: &Status) -> (StatusValue, Confidence, bool) {
    (st.value, st.confidence, st.stale)
}

#[test]
fn t31_rule3_open_turn_recent_write_window() {
    let s = open_turn(false);
    let a = at(&s, 29);
    assert_eq!(tuple(&a), (StatusValue::Running, Confidence::High, false));
    assert_eq!(aggregate_updating([&a]), (true, Confidence::High));
    let b = at(&s, 30);
    assert_eq!(tuple(&b), (StatusValue::Running, Confidence::Low, false));
    assert_eq!(aggregate_updating([&b]), (true, Confidence::Low));
}

#[test]
fn t31_rule3_tool_open_599_to_601() {
    let s = open_turn(true);
    let a = at(&s, 599);
    assert_eq!(tuple(&a), (StatusValue::Running, Confidence::High, false));
    assert!(aggregate_updating([&a]).0);
    for age in [600, 601] {
        let b = at(&s, age);
        assert_eq!(tuple(&b), (StatusValue::Running, Confidence::Low, true));
        assert!(!aggregate_updating([&b]).0);
    }
}

#[test]
fn t31_rule2_rule4_boundaries_599_600() {
    let c = ended(TurnEnd::Complete);
    assert_eq!(tuple(&at(&c, 599)), (StatusValue::WaitingInput, Confidence::High, false));
    assert_eq!(tuple(&at(&c, 600)), (StatusValue::Idle, Confidence::High, false));
    let e = ended(TurnEnd::Error);
    assert_eq!(tuple(&at(&e, 599)), (StatusValue::Error, Confidence::High, false));
    assert_eq!(tuple(&at(&e, 600)), (StatusValue::Idle, Confidence::High, false));
}

#[test]
fn f14_unresolved_tool_use_700_is_stale_running() {
    let s = open_turn(true);
    let st = at(&s, 700);
    assert_eq!((st.value, st.stale), (StatusValue::Running, true));
    assert!(!aggregate_updating([&st]).0);
}

#[test]
fn f20_updating_599_600_601() {
    let s = open_turn(false);
    let u = |age| aggregate_updating([&at(&s, age)]).0;
    assert_eq!((u(599), u(600), u(601)), (true, false, false));
}

#[test]
fn t31_rule1_precedes_rule5() {
    let mut a = open_turn(false);
    a.last_record_unreadable = true; // invalid_utf8 last line
    let mut b = open_turn(false);
    b.last_record_unreadable = true; // oversized_line last line
    let mut c = SessionState::new("c", SessionKind::TopLevel);
    c.tail_empty = true; // no complete line within TAIL_SCAN_BYTES
    for s in [&a, &b, &c] {
        for age in [0, 100, 700] {
            assert_eq!(tuple(&at(s, age)), (StatusValue::Unknown, Confidence::Low, false));
        }
    }
}

#[test]
fn t31_rule5_no_turn_boundary() {
    let s = base();
    assert_eq!(tuple(&at(&s, 100)), (StatusValue::Running, Confidence::Low, false));
    assert_eq!(tuple(&at(&s, 600)), (StatusValue::Idle, Confidence::Low, false));
}
