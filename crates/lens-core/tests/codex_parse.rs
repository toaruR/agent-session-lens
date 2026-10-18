//! Codex rollout parsing, first half (architecture-v5 section 5). Synthetic fixtures only.

use chrono::{DateTime, Duration, Utc};
use lens_core::engine::{Engine, SourceConfig};
use lens_core::model::*;
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

struct Env {
    _tmp: tempfile::TempDir,
    root: PathBuf,
}

fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("sessions");
    fs::create_dir_all(&root).unwrap();
    Env { root, _tmp: tmp }
}

fn engine(e: &Env) -> Engine {
    Engine::new(vec![SourceConfig { id: "codex-local".into(), agent: Agent::Codex, root: e.root.clone() }])
}

fn snap(eng: &mut Engine) -> Snapshot {
    eng.rescan();
    eng.process_all();
    eng.snapshot(Utc::now())
}

fn write_lines(path: &Path, lines: &[Value]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = fs::File::create(path).unwrap();
    for l in lines {
        writeln!(f, "{l}").unwrap();
    }
}

fn rollout(e: &Env, id: &str) -> PathBuf {
    e.root.join("2026").join("10").join("17").join(format!("rollout-2026-10-17T00-00-00-{id}.jsonl"))
}

fn meta(id: &str, cwd: &str, ts: DateTime<Utc>, extra: Value) -> Value {
    let mut payload = json!({"id": id, "cwd": cwd});
    for (k, v) in extra.as_object().unwrap() {
        payload[k] = v.clone();
    }
    json!({"type":"session_meta","timestamp":ts.to_rfc3339(),"payload":payload})
}

fn ev(kind: &str, ts: DateTime<Utc>) -> Value {
    json!({"type":"event_msg","timestamp":ts.to_rfc3339(),"payload":{"type":kind}})
}

fn find<'a>(s: &'a Snapshot, uid: &str) -> &'a UnifiedSession {
    s.sessions.iter().find(|x| x.session_uid == uid).unwrap()
}

#[test]
fn f1a_fork_ordinal_excludes_inherited() {
    let e = env();
    let f0 = Utc::now() - Duration::seconds(3000);
    let f5 = f0 + Duration::seconds(5);
    write_lines(
        &rollout(&e, "k1"),
        &[
            meta("k1", "/w/repo", f0, json!({"subagent_history_start_ordinal": 3})),
            ev("task_started", f0),
            ev("task_complete", f0),
            ev("task_started", f5),
        ],
    );
    let mut eng = engine(&e);
    let s = snap(&mut eng);
    let x = find(&s, "codex-local:k1");
    assert_eq!(x.timestamps.last_active_at.as_deref(), Some(f5.to_rfc3339().as_str()));
    assert_eq!(x.timestamps.last_active_source, TimeSource::Event);
}

#[test]
fn f1b_fork_boundary_unknown() {
    let e = env();
    let t = Utc::now() - Duration::seconds(5);
    write_lines(
        &rollout(&e, "k2"),
        &[
            meta("k2", "/w/repo", t, json!({})),
            ev("task_started", t),
            meta("other", "/w/other", t + Duration::seconds(1), json!({})),
            ev("task_complete", t + Duration::seconds(2)),
            meta("third", "/w/third", t + Duration::seconds(3), json!({})),
        ],
    );
    let mut eng = engine(&e);
    let s = snap(&mut eng);
    let x = find(&s, "codex-local:k2");
    let d = x.health.diagnostics.iter().find(|d| d.code == "fork_boundary_unknown").unwrap();
    assert_eq!(d.count, 1);
    assert_eq!(x.status.confidence, Confidence::Low);
    assert_eq!(x.timestamps.last_active_at.as_deref(), Some((t + Duration::seconds(2)).to_rfc3339().as_str()));
}

#[test]
fn first_session_meta_wins() {
    let e = env();
    let t = Utc::now() - Duration::seconds(5);
    write_lines(
        &rollout(&e, "a"),
        &[meta("a", "C:\\x", t, json!({})), meta("b", "D:\\y", t, json!({"subagent_history_start_ordinal": 0})), ev("task_started", t)],
    );
    let mut eng = engine(&e);
    let s = snap(&mut eng);
    assert_eq!(s.sessions.len(), 1);
    let x = find(&s, "codex-local:a");
    assert_eq!(x.projects.len(), 1);
    assert_eq!(x.projects[0].key, "c:\\x");
}

#[test]
fn session_meta_only_file_no_activity() {
    let e = env();
    let t = Utc::now() - Duration::seconds(5);
    let path = rollout(&e, "m1");
    write_lines(&path, &[meta("m1", "/w/repo", t, json!({}))]);
    let mtime: DateTime<Utc> = fs::metadata(&path).unwrap().modified().unwrap().into();
    let mut eng = engine(&e);
    let s = snap(&mut eng);
    let x = find(&s, "codex-local:m1");
    assert_eq!(x.timestamps.last_active_source, TimeSource::FileMtime);
    assert_eq!(x.timestamps.last_active_at.as_deref(), Some(mtime.to_rfc3339().as_str()));
    assert_ne!(x.status.reason.as_str(), "");
    assert!(!x.status.reason.starts_with("turn open"), "no turn open: {}", x.status.reason);
}

fn msg(role: &str, text: &str, ts: DateTime<Utc>) -> Value {
    let kind = if role == "user" { "input_text" } else { "output_text" };
    json!({"type":"response_item","timestamp":ts.to_rfc3339(),"payload":{"type":"message","role":role,"content":[{"type":kind,"text":text}]}})
}

#[test]
fn f17_response_item_only_body() {
    let e = env();
    let t = Utc::now() - Duration::seconds(20);
    write_lines(
        &rollout(&e, "b1"),
        &[
            meta("b1", "/w/repo", t, json!({})),
            json!({"type":"event_msg","timestamp":t.to_rfc3339(),"payload":{"type":"user_message","message":"do it"}}),
            msg("user", "do it", t),
            json!({"type":"event_msg","timestamp":t.to_rfc3339(),"payload":{"type":"agent_message","message":"done"}}),
            msg("assistant", "done", t),
        ],
    );
    let mut eng = engine(&e);
    let s = snap(&mut eng);
    let x = find(&s, "codex-local:b1");
    assert_eq!(x.metrics.step_count, 2);
    assert_eq!(x.current_task.last_user_message.as_deref(), Some("do it"));
    assert_eq!(x.current_task.last_assistant_message.as_deref(), Some("done"));
}

#[test]
fn f24_unknown_thread_source() {
    let e = env();
    let t = Utc::now() - Duration::seconds(5);
    write_lines(&rollout(&e, "u1"), &[meta("u1", "/w/repo", t, json!({"thread_source":"foo"})), ev("task_started", t)]);
    write_lines(&rollout(&e, "u2"), &[meta("u2", "/w/repo", t, json!({"thread_source":"user"})), ev("task_started", t)]);
    let mut eng = engine(&e);
    let s = snap(&mut eng);
    let x = find(&s, "codex-local:u1");
    assert_eq!(x.hierarchy.kind, SessionKind::TopLevel);
    let d = x.health.diagnostics.iter().find(|d| d.code == "unknown_thread_source").unwrap();
    assert_eq!(d.count, 1);
    assert_eq!(x.health.completeness, Completeness::Partial);
    let u = find(&s, "codex-local:u2");
    assert_eq!(u.hierarchy.kind, SessionKind::TopLevel);
    assert_eq!(u.health.completeness, Completeness::Complete);
}

#[test]
fn f19_guardian_review_is_automated() {
    let e = env();
    let t = Utc::now() - Duration::seconds(100);
    let newer = Utc::now() - Duration::seconds(3);
    write_lines(&rollout(&e, "p1"), &[meta("p1", "/w/repo", t, json!({})), ev("task_started", t)]);
    write_lines(
        &rollout(&e, "g1"),
        &[meta("g1", "/w/repo", newer, json!({"thread_source":"guardian_review","parent_thread_id":"p1"})), ev("task_started", newer)],
    );
    let mut eng = engine(&e);
    let s = snap(&mut eng);
    let g = find(&s, "codex-local:g1");
    assert_eq!(g.hierarchy.kind, SessionKind::Automated);
    assert_eq!(g.health.completeness, Completeness::Complete);
    let p = find(&s, "codex-local:p1");
    assert!(p.status.auto_review);
    assert_eq!(p.timestamps.last_active_at.as_deref(), Some(t.to_rfc3339().as_str()));
}

#[test]
fn title_index_then_first_human() {
    let e = env();
    let t = Utc::now() - Duration::seconds(60);
    write_lines(&rollout(&e, "a"), &[meta("a", "/w/repo", t, json!({})), msg("user", "H-a", t)]);
    write_lines(&rollout(&e, "b"), &[meta("b", "/w/repo", t, json!({})), msg("user", "H", t)]);
    write_lines(&e.root.parent().unwrap().join("session_index.jsonl"), &[json!({"id":"a","thread_name":"N"})]);
    // turn signals
    let turn = |id: &str, last: Value| {
        write_lines(&rollout(&e, id), &[meta(id, "/w/repo", t, json!({})), ev("task_started", t), last]);
    };
    turn("c", ev("task_complete", t + Duration::seconds(1)));
    turn("d", json!({"type":"event_msg","timestamp":(t + Duration::seconds(1)).to_rfc3339(),"payload":{"type":"task_complete","error":"boom"}}));
    turn("f", ev("turn_aborted", t + Duration::seconds(1)));
    turn("g", ev("task_started", t + Duration::seconds(1)));
    let mut eng = engine(&e);
    let s = snap(&mut eng);
    assert_eq!(find(&s, "codex-local:a").current_task.title.as_deref(), Some("N"));
    assert_eq!(find(&s, "codex-local:b").current_task.title.as_deref(), Some("H"));
    assert!(find(&s, "codex-local:c").status.reason.starts_with("turn complete"));
    assert_eq!(find(&s, "codex-local:d").status.value, StatusValue::Error);
    assert!(find(&s, "codex-local:f").status.reason.starts_with("turn aborted"));
    assert!(find(&s, "codex-local:g").status.reason.starts_with("turn open"));
}
