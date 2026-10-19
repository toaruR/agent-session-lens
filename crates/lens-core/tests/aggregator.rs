//! Aggregator first half (architecture-v5 sections 3.1, 3.3, 4): F2, F11c, F19, F25b. Time is a FakeClock.

use chrono::{DateTime, Duration, Utc};
use lens_core::aggregator::{origin_for_source, Aggregator, SessionInput};
use lens_core::clock::FakeClock;
use lens_core::model::v5::{SessionProject, UNATTRIBUTED_KEY};
use lens_core::model::{Agent, Attribution, Confidence, SessionKind, StatusValue};
use lens_core::session::{SessionState, TurnEnd};
use std::sync::Arc;
use std::time::SystemTime;

fn t0() -> DateTime<Utc> {
    "2026-10-17T00:00:00Z".parse().unwrap()
}

fn at(secs: i64) -> DateTime<Utc> {
    t0() + Duration::seconds(secs)
}

fn clock_at(secs: i64) -> Arc<FakeClock> {
    Arc::new(FakeClock::new(SystemTime::from(at(secs))))
}

fn proj(name: &str) -> Vec<SessionProject> {
    vec![SessionProject {
        key: format!("D:/work/{name}"),
        name: name.to_string(),
        display_path: format!("D:\\work\\{name}"),
        attribution: Attribution::GitRoot,
    }]
}

fn input(source: &str, agent: Agent, state: SessionState, projects: Vec<SessionProject>) -> SessionInput {
    SessionInput { source_id: source.into(), agent, origin: origin_for_source(source), state, file_mtime: None, projects }
}

fn claude(state: SessionState, projects: Vec<SessionProject>) -> SessionInput {
    input("claude-local", Agent::ClaudeCode, state, projects)
}

/// A session with a human message at `start` and an open tool call at `tool`.
fn running(id: &str, kind: SessionKind, parent: Option<&str>, start: i64, tool: i64) -> SessionState {
    let mut s = SessionState::new(id, kind);
    s.parent_native = parent.map(String::from);
    s.touch(Some(at(start)));
    s.human_message("go", Some(at(start)));
    s.tool_start("c1".into(), "Bash", "ls", Some(at(tool)));
    s.touch(Some(at(tool)));
    s
}

#[test]
fn f2_child_idle_after_parent_end_turn() {
    let mut agg = Aggregator::new(clock_at(110));
    let mut parent = SessionState::new("p", SessionKind::TopLevel);
    parent.touch(Some(at(0)));
    parent.human_message("go", Some(at(0)));
    parent.end_turn(TurnEnd::Complete, Some(at(100)), "end_turn");
    parent.touch(Some(at(100)));
    let child = running("c", SessionKind::Subagent, Some("p"), 10, 50);
    agg.upsert(claude(child, proj("repo")));
    agg.upsert(claude(parent, proj("repo")));

    let p = agg.session("claude-local:p").unwrap();
    assert_eq!((p.status.value, p.status.confidence, p.status.stale), (StatusValue::WaitingInput, Confidence::High, false));
    let c = agg.session("claude-local:c").unwrap();
    assert_eq!((c.status.value, c.status.confidence, c.status.stale), (StatusValue::Idle, Confidence::Low, false));
    assert_eq!(c.status.reason, "parent turn ended after last activity");
    assert_eq!(p.hierarchy.child_uids, vec!["claude-local:c".to_string()]);
    let rows = agg.list_projects();
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].updating);
    assert_eq!(rows[0].updating_confidence, None);
    // lastUpdatedAt includes the subagent and is the max of the attributed sessions.
    assert_eq!(rows[0].last_updated_at.as_deref().map(|s| s.parse::<DateTime<Utc>>().unwrap()), Some(at(100)));
}

#[test]
fn f19_guardian_review_not_activity() {
    let clock = clock_at(100);
    let mut agg = Aggregator::new(clock);
    let parent = running("p", SessionKind::TopLevel, None, 0, 10);
    let mut cx = input("codex-local", Agent::Codex, parent, proj("repo"));
    cx.state.cwd = Some("D:/work/repo".into());
    agg.upsert(cx.clone());
    let before = agg.list_projects()[0].clone();
    let before_status = agg.session("codex-local:p").unwrap().status;
    assert!(!before_status.auto_review);

    let g = running("g", SessionKind::Automated, Some("p"), 40, 60);
    agg.upsert(input("codex-local", Agent::Codex, g, proj("repo")));

    let rows = agg.list_projects();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].last_updated_at, before.last_updated_at);
    assert_eq!(rows[0].session_uids, vec!["codex-local:p".to_string()]);
    assert!(!rows[0].session_uids.iter().any(|u| u.ends_with(":g")));
    let detail = agg.project_detail(&rows[0].key).unwrap();
    assert_eq!(detail.sessions.len(), 1);
    let p = agg.session("codex-local:p").unwrap();
    assert!(p.status.auto_review);
    assert_eq!(p.status.value, before_status.value);
    assert_eq!(p.hierarchy.child_uids, Vec::<String>::new());

    // Removing the reviewer clears the flag again.
    assert!(agg.remove("codex-local:g"));
    assert!(!agg.session("codex-local:p").unwrap().status.auto_review);
}

#[test]
fn f25b_equal_time_name_ascending() {
    let mut agg = Aggregator::new(clock_at(10));
    for name in ["beta", "alpha"] {
        let mut s = SessionState::new(format!("s-{name}"), SessionKind::TopLevel);
        s.touch(Some(at(5)));
        agg.upsert(claude(s, proj(name)));
    }
    let names: Vec<String> = agg.list_projects().into_iter().map(|p| p.name).collect();
    assert_eq!(names, ["alpha", "beta"]);

    // A newer project goes first; the unattributed session is a single row of its own.
    let mut newer = SessionState::new("s-new", SessionKind::TopLevel);
    newer.touch(Some(at(8)));
    agg.upsert(claude(newer, proj("zeta")));
    let mut lost = SessionState::new("s-lost", SessionKind::TopLevel);
    lost.touch(Some(at(1)));
    agg.upsert(claude(lost, Vec::new()));
    let mut lost2 = SessionState::new("s-lost2", SessionKind::TopLevel);
    lost2.touch(Some(at(2)));
    agg.upsert(claude(lost2, Vec::new()));
    let keys: Vec<String> = agg.list_projects().into_iter().map(|p| p.name).collect();
    assert_eq!(keys.len(), 4);
    assert_eq!(&keys[..3], ["zeta", "alpha", "beta"]);
    let rows = agg.list_projects();
    assert_eq!(rows[3].key, UNATTRIBUTED_KEY);
    assert_eq!(rows[3].session_uids.len(), 2);
}

#[test]
fn f11c_stale_observation_delay_all_phases() {
    for phase in 0..5i64 {
        let start = 599 - phase;
        let clock = clock_at(start);
        let mut agg = Aggregator::new(clock.clone());
        agg.upsert(claude(running("s", SessionKind::TopLevel, None, 0, 0), proj("repo")));
        let mut first_stale = None;
        for step in 0..12 {
            let age = start + step;
            agg.tick();
            let st = agg.session("claude-local:s").unwrap().status;
            if st.stale && first_stale.is_none() {
                first_stale = Some(age);
            }
            clock.advance(std::time::Duration::from_secs(1));
        }
        let age = first_stale.unwrap_or_else(|| panic!("never stale in phase {phase}"));
        assert!((600..=604).contains(&age), "phase {phase}: first observed stale at age {age}");
    }
}

#[test]
fn only_affected_projects_are_recomputed() {
    let mut agg = Aggregator::new(clock_at(10));
    for name in ["a", "b", "c"] {
        let mut s = SessionState::new(format!("s-{name}"), SessionKind::TopLevel);
        s.touch(Some(at(1)));
        agg.upsert(claude(s, proj(name)));
    }
    let n = agg.project_recompute_count();
    let mut s = SessionState::new("s-b", SessionKind::TopLevel);
    s.touch(Some(at(2)));
    assert!(agg.upsert(claude(s.clone(), proj("b"))));
    assert_eq!(agg.project_recompute_count(), n + 1);
    // An identical session is not an update.
    assert!(!agg.upsert(claude(s, proj("b"))));
    assert_eq!(agg.project_recompute_count(), n + 1);
}

fn count_projects_changed(agg: &mut Aggregator) -> usize {
    agg.take_events().iter().filter(|e| matches!(e, lens_core::model::v5::Ev::ProjectsChanged { .. })).count()
}

#[test]
fn f15_own_repo_session_is_listed() {
    // Documented unprotectable limit: Lens's own repository is not excluded.
    let mut agg = Aggregator::new(clock_at(10));
    let own = env!("CARGO_MANIFEST_DIR").replace('\\', "/");
    let mut s = running("own", SessionKind::TopLevel, None, 0, 5);
    s.cwd = Some(own.clone());
    let projects = vec![SessionProject {
        key: own.clone(),
        name: "agent-session-lens".into(),
        display_path: own.clone(),
        attribution: Attribution::GitRoot,
    }];
    agg.upsert(claude(s, projects));
    let rows = agg.list_projects();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].key, own);
    assert_eq!(rows[0].session_uids, vec!["claude-local:own".to_string()]);
}

#[test]
fn outgoing_payload_sanitized() {
    let token = format!("ghp_{}", "a1B2".repeat(9));
    assert_eq!(token.len(), 40);
    let mut agg = Aggregator::new(clock_at(10));
    let mut s = SessionState::new("sec", SessionKind::TopLevel);
    s.touch(Some(at(1)));
    s.human_message(&format!("use {token} now"), Some(at(1)));
    s.title = Some(format!("title {token}"));
    s.last_assistant = Some(format!("reply {token}"));
    s.tool_start("c1".into(), "Bash", &format!("curl {token}"), Some(at(2)));
    let name = format!("proj-{token}");
    let projects = vec![SessionProject {
        key: format!("D:/work/{name}"),
        name: name.clone(),
        display_path: format!("D:\\work\\{name}"),
        attribution: Attribution::GitRoot,
    }];
    agg.upsert(claude(s, projects));

    let rows = agg.list_projects();
    let sess = agg.session("claude-local:sec").unwrap();
    let detail = agg.project_detail(&rows[0].key).unwrap();
    let events = agg.take_events();
    for json in [
        serde_json::to_string(&rows).unwrap(),
        serde_json::to_string(&sess).unwrap(),
        serde_json::to_string(&detail).unwrap(),
        serde_json::to_string(&events).unwrap(),
    ] {
        assert!(!json.contains(&token), "raw token leaked: {json}");
        assert!(json.contains("[REDACTED]"));
    }
    assert!(rows[0].name.contains("[REDACTED]"));
    let ct = &sess.current_task;
    for f in [&ct.title, &ct.last_user_message, &ct.last_assistant_message] {
        assert!(f.as_deref().unwrap().contains("[REDACTED]"));
    }
    assert!(ct.active_tool.as_ref().unwrap().input.contains("[REDACTED]"));
}

#[test]
fn projects_changed_only_on_change() {
    let clock = clock_at(10);
    let mut agg = Aggregator::new(clock.clone());
    let mut a = SessionState::new("a", SessionKind::TopLevel);
    a.touch(Some(at(5)));
    agg.upsert(claude(a, proj("a")));
    // Identical re-evaluations emit nothing.
    clock.advance(std::time::Duration::from_secs(1));
    agg.reeval();
    clock.advance(std::time::Duration::from_secs(1));
    agg.reeval();
    assert_eq!(count_projects_changed(&mut agg), 1);

    // A changed order is a second event.
    let mut b = SessionState::new("b", SessionKind::TopLevel);
    b.touch(Some(at(8)));
    agg.upsert(claude(b, proj("b")));
    let evs = agg.take_events();
    assert_eq!(evs.len(), 1);
    match &evs[0] {
        lens_core::model::v5::Ev::ProjectsChanged { order, .. } => {
            assert_eq!(order, &["D:/work/b".to_string(), "D:/work/a".to_string()])
        }
        _ => panic!("expected projects_changed"),
    }
    agg.reeval();
    assert_eq!(count_projects_changed(&mut agg), 0);
}

#[test]
fn project_row_includes_antigravity_agent() {
    let clock = clock_at(10);
    let mut agg = Aggregator::new(clock);
    let mut s1 = SessionState::new("claude_s", SessionKind::TopLevel);
    s1.touch(Some(at(1)));
    agg.upsert(input("claude-local", Agent::ClaudeCode, s1, proj("my-repo")));

    let mut s2 = SessionState::new("agy_s", SessionKind::TopLevel);
    s2.touch(Some(at(2)));
    agg.upsert(input("agy-local", Agent::Antigravity, s2, proj("my-repo")));

    let rows = agg.list_projects();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].agents, vec![Agent::ClaudeCode, Agent::Antigravity]);
}


fn raw(name: &str) -> Vec<SessionProject> {
    vec![SessionProject {
        key: format!("D:/tmp/{name}"),
        name: name.to_string(),
        display_path: format!("D:/tmp/{name}"),
        attribution: Attribution::RawPath,
    }]
}

fn idle_at(id: &str, secs: i64) -> SessionState {
    let mut s = SessionState::new(id, SessionKind::TopLevel);
    s.touch(Some(at(secs)));
    s
}

#[test]
fn non_repo_project_hidden_after_60_minutes() {
    let clock = clock_at(3599);
    let mut agg = Aggregator::new(clock.clone());
    agg.upsert(claude(idle_at("raw", 0), raw("scratch")));
    agg.upsert(claude(idle_at("none", 0), Vec::new()));
    assert_eq!(agg.list_projects().len(), 2);
    clock.advance(std::time::Duration::from_secs(1));
    agg.reeval();
    assert!(agg.list_projects().is_empty());
    // A new event brings the row back.
    agg.upsert(claude(idle_at("raw", 3600), raw("scratch")));
    assert_eq!(agg.list_projects().len(), 1);
}

#[test]
fn repo_project_hidden_after_one_month() {
    let month = 30 * 24 * 3600;
    let clock = clock_at(month - 1);
    let mut agg = Aggregator::new(clock.clone());
    agg.upsert(claude(idle_at("repo", 0), proj("repo")));
    assert_eq!(agg.list_projects().len(), 1);
    clock.advance(std::time::Duration::from_secs(1));
    agg.reeval();
    assert!(agg.list_projects().is_empty());
}
