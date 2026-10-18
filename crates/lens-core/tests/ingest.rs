//! Synthetic-fixture regression tests (architecture-v3 §8). No real log content is used.

use lens_core::engine::{Engine, SourceConfig};
use lens_core::model::*;
use lens_core::reader::{self, Cursor, Limits};
use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

fn now() -> DateTime<Utc> {
    Utc::now()
}

fn ts(offset_secs: i64) -> String {
    (now() + Duration::seconds(offset_secs)).to_rfc3339()
}

fn write_lines(path: &Path, lines: &[Value]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = OpenOptions::new().create(true).append(true).open(path).unwrap();
    for l in lines {
        writeln!(f, "{}", l).unwrap();
    }
}

struct Env {
    _tmp: tempfile::TempDir,
    claude: PathBuf,
    codex: PathBuf,
    cwd: PathBuf,
}

fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let claude = tmp.path().join("claude").join("projects");
    let codex = tmp.path().join("codex").join("sessions");
    let cwd = tmp.path().join("work").join("repo");
    fs::create_dir_all(cwd.join(".git")).unwrap();
    fs::create_dir_all(cwd.join("sub")).unwrap();
    Env { claude, codex, cwd, _tmp: tmp }
}

fn engine(e: &Env) -> Engine {
    Engine::new(vec![
        SourceConfig { id: "claude-local".into(), agent: Agent::ClaudeCode, root: e.claude.clone() },
        SourceConfig { id: "codex-local".into(), agent: Agent::Codex, root: e.codex.clone() },
    ])
}

fn snap(eng: &mut Engine) -> Snapshot {
    eng.rescan();
    eng.process_all();
    eng.snapshot(now())
}

fn find<'a>(s: &'a Snapshot, uid: &str) -> &'a UnifiedSession {
    s.sessions.iter().find(|x| x.session_uid == uid).unwrap_or_else(|| panic!("missing {uid}"))
}

fn c_user(e: &Env, sid: &str, content: Value, t: i64) -> Value {
    json!({"type":"user","sessionId":sid,"cwd":e.cwd.join("sub"),"timestamp":ts(t),"version":"9.9.9","message":{"role":"user","content":content}})
}

fn c_asst(e: &Env, sid: &str, blocks: Value, stop: &str, t: i64) -> Value {
    json!({"type":"assistant","sessionId":sid,"cwd":e.cwd,"timestamp":ts(t),"message":{"role":"assistant","content":blocks,"stop_reason":stop}})
}

// ---------- Claude ----------

#[test]
fn claude_running_tool_then_waiting_input() {
    let e = env();
    let f = e.claude.join("D--work-repo").join("s1.jsonl");
    write_lines(&f, &[
        c_user(&e, "s1", json!([{"type":"text","text":"<ide_opened_file>x</ide_opened_file>"},{"type":"text","text":"run the tests"}]), -20),
        c_asst(&e, "s1", json!([{"type":"thinking","thinking":"secret"}]), "tool_use", -19),
        c_asst(&e, "s1", json!([{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"npm test"}}]), "tool_use", -19),
    ]);
    let mut eng = engine(&e);
    let s = snap(&mut eng);
    let x = find(&s, "claude-local:s1");
    assert_eq!(x.status.value, StatusValue::Running);
    assert_eq!(x.status.confidence, Confidence::High);
    assert_eq!(x.current_task.title.as_deref(), Some("run the tests"));
    assert_eq!(x.current_task.active_tool.as_ref().unwrap().input, "npm test");
    assert_eq!(x.projects[0].attribution, Attribution::GitRoot);
    assert_eq!(x.projects[0].name, "repo");
    assert_eq!(x.writer_version.as_deref(), Some("9.9.9"));

    write_lines(&f, &[
        c_user(&e, "s1", json!([{"type":"tool_result","tool_use_id":"t1","content":"ok"}]), -10),
        c_asst(&e, "s1", json!([{"type":"text","text":"All tests pass."}]), "end_turn", -9),
    ]);
    let s = snap(&mut eng);
    let x = find(&s, "claude-local:s1");
    assert_eq!(x.status.value, StatusValue::WaitingInput);
    assert!(x.current_task.active_tool.is_none());
    assert_eq!(x.current_task.last_assistant_message.as_deref(), Some("All tests pass."));
    // tool_result envelope is not a human message
    assert_eq!(x.current_task.last_user_message.as_deref(), Some("run the tests"));
}

#[test]
fn claude_interrupt_and_meta_and_command() {
    let e = env();
    let f = e.claude.join("p").join("s2.jsonl");
    let mut meta = c_user(&e, "s2", json!("caveat text"), -30);
    meta["isMeta"] = json!(true);
    write_lines(&f, &[
        meta,
        c_user(&e, "s2", json!("<command-name>/review</command-name>\n<command-args>main</command-args>"), -20),
        c_asst(&e, "s2", json!([{"type":"tool_use","id":"t9","name":"Read","input":{"file_path":"a.rs"}}]), "tool_use", -19),
        c_user(&e, "s2", json!([{"type":"text","text":"[Request interrupted by user for tool use]"}]), -18),
    ]);
    let s = snap(&mut engine(&e));
    let x = find(&s, "claude-local:s2");
    assert_eq!(x.current_task.title.as_deref(), Some("/review main"));
    assert_eq!(x.status.value, StatusValue::WaitingInput);
    assert!(x.status.reason.contains("interrupted"));
}

#[test]
fn claude_subagent_file_links_to_parent() {
    let e = env();
    let parent = e.claude.join("p").join("s3.jsonl");
    let child = e.claude.join("p").join("s3").join("subagents").join("agent-abc.jsonl");
    write_lines(&parent, &[
        c_user(&e, "s3", json!("delegate"), -10),
        c_asst(&e, "s3", json!([{"type":"tool_use","id":"t1","name":"Agent","input":{"description":"explore"}}]), "tool_use", -9),
    ]);
    let mut cu = c_user(&e, "s3", json!("explore the repo"), -8);
    cu["isSidechain"] = json!(true);
    cu["agentId"] = json!("abc");
    write_lines(&child, &[cu]);
    fs::write(child.with_extension("meta.json"), "{}").unwrap();

    let s = snap(&mut engine(&e));
    let c = find(&s, "claude-local:s3::agent:abc");
    assert_eq!(c.hierarchy.kind, SessionKind::Subagent);
    assert_eq!(c.hierarchy.parent_uid.as_deref(), Some("claude-local:s3"));
    let p = find(&s, "claude-local:s3");
    assert_eq!(p.hierarchy.child_uids, vec!["claude-local:s3::agent:abc".to_string()]);
    assert_eq!(s.sessions.len(), 2);
}

#[test]
fn claude_api_error_then_recovery() {
    let e = env();
    let f = e.claude.join("p").join("s4.jsonl");
    write_lines(&f, &[
        c_user(&e, "s4", json!("go"), -10),
        json!({"type":"system","subtype":"api_error","sessionId":"s4","timestamp":ts(-9)}),
    ]);
    let mut eng = engine(&e);
    assert_eq!(find(&snap(&mut eng), "claude-local:s4").status.value, StatusValue::Error);
    write_lines(&f, &[c_asst(&e, "s4", json!([{"type":"text","text":"done"}]), "end_turn", -5)]);
    assert_eq!(find(&snap(&mut eng), "claude-local:s4").status.value, StatusValue::WaitingInput);
}

// ---------- Codex ----------

fn meta(id: &str, cwd: &Path, extra: Value) -> Value {
    let mut p = json!({"id":id,"cwd":cwd,"cli_version":"0.1.0","thread_source":"user","timestamp":ts(-100)});
    if let (Some(p), Some(x)) = (p.as_object_mut(), extra.as_object()) {
        for (k, v) in x {
            p.insert(k.clone(), v.clone());
        }
    }
    json!({"type":"session_meta","timestamp":ts(-100),"payload":p})
}

fn ev(kind: &str, t: i64) -> Value {
    json!({"type":"event_msg","timestamp":ts(t),"payload":{"type":kind,"turn_id":"u1"}})
}

fn msg(role: &str, text: &str, t: i64) -> Value {
    let block = if role == "assistant" { "output_text" } else { "input_text" };
    json!({"type":"response_item","timestamp":ts(t),"payload":{"type":"message","role":role,"content":[{"type":block,"text":text}]}})
}

fn rollout(e: &Env, id: &str) -> PathBuf {
    e.codex.join("2026").join("10").join("17").join(format!("rollout-2026-10-17T00-00-00-{id}.jsonl"))
}

#[test]
fn codex_turn_lifecycle_and_no_double_ingestion() {
    let e = env();
    let f = rollout(&e, "c1");
    write_lines(&f, &[
        meta("c1", &e.cwd, json!({})),
        ev("task_started", -50),
        msg("user", "# AGENTS.md instructions for x", -49),
        msg("user", "<environment_context>…</environment_context>", -49),
        msg("user", "fix the bug", -48),
        json!({"type":"event_msg","timestamp":ts(-48),"payload":{"type":"user_message","message":"fix the bug"}}),
        json!({"type":"response_item","timestamp":ts(-40),"payload":{"type":"function_call","call_id":"k1","name":"shell","arguments":"{\"cmd\":[\"cargo\",\"test\"]}"}}),
    ]);
    let mut eng = engine(&e);
    let s = snap(&mut eng);
    let x = find(&s, "codex-local:c1");
    assert_eq!(x.status.value, StatusValue::Running);
    assert_eq!(x.current_task.title.as_deref(), Some("fix the bug"));
    assert_eq!(x.metrics.step_count, 1, "event_msg copy must not be ingested");
    assert_eq!(x.current_task.active_tool.as_ref().unwrap().input, "cargo test");

    write_lines(&f, &[
        json!({"type":"response_item","timestamp":ts(-30),"payload":{"type":"function_call_output","call_id":"k1","output":"ok"}}),
        msg("assistant", "Fixed.", -20),
        ev("task_complete", -19),
    ]);
    let x = find(&snap(&mut eng), "codex-local:c1").clone();
    assert_eq!(x.status.value, StatusValue::WaitingInput);
    assert_eq!(x.status.confidence, Confidence::High);
}

#[test]
fn codex_task_complete_error_and_abort() {
    let e = env();
    write_lines(&rollout(&e, "c2"), &[
        meta("c2", &e.cwd, json!({})),
        ev("task_started", -10),
        json!({"type":"event_msg","timestamp":ts(-5),"payload":{"type":"task_complete","error":{"message":"boom"}}}),
    ]);
    write_lines(&rollout(&e, "c3"), &[
        meta("c3", &e.cwd, json!({})),
        ev("task_started", -10),
        json!({"type":"event_msg","timestamp":ts(-5),"payload":{"type":"turn_aborted","reason":"interrupted"}}),
    ]);
    let s = snap(&mut engine(&e));
    assert_eq!(find(&s, "codex-local:c2").status.value, StatusValue::Error);
    let c3 = find(&s, "codex-local:c3");
    assert_eq!(c3.status.value, StatusValue::WaitingInput);
    assert!(c3.status.reason.contains("interrupted"));
}

#[test]
fn codex_fork_skips_inherited_records_by_ordinal() {
    let e = env();
    // Lines 0..=3 are inherited (their timestamps are rewritten to the fork time in real logs).
    write_lines(&rollout(&e, "child"), &[
        meta("child", &e.cwd, json!({"thread_source":"subagent","parent_thread_id":"par","forked_from_id":"par","subagent_history_start_ordinal":4})),
        meta("par", &e.cwd, json!({})),
        ev("task_started", -10),
        msg("user", "parent request", -10),
        ev("task_started", -9),
        msg("user", "child task", -9),
    ]);
    write_lines(&rollout(&e, "par"), &[meta("par", &e.cwd, json!({})), msg("user", "parent request", -60), ev("task_complete", -59)]);
    let s = snap(&mut engine(&e));
    let c = find(&s, "codex-local:child");
    assert_eq!(c.current_task.title.as_deref(), Some("child task"));
    assert_eq!(c.metrics.step_count, 1);
    assert_eq!(c.hierarchy.kind, SessionKind::Subagent);
    assert_eq!(find(&s, "codex-local:par").hierarchy.child_uids, vec!["codex-local:child".to_string()]);
}

#[test]
fn codex_fork_without_ordinal_is_degraded() {
    let e = env();
    write_lines(&rollout(&e, "f1"), &[
        meta("f1", &e.cwd, json!({"forked_from_id":"p0"})),
        meta("p0", &e.cwd, json!({})),
        ev("task_started", -5),
        msg("user", "x", -5),
        ev("task_complete", -4),
    ]);
    let s = snap(&mut engine(&e));
    let f = find(&s, "codex-local:f1");
    assert_eq!(f.hierarchy.kind, SessionKind::Fork);
    assert_eq!(f.status.confidence, Confidence::Low);
    assert_eq!(f.health.completeness, Completeness::Partial);
    assert!(f.health.diagnostics.iter().any(|d| d.code == "fork_boundary_unknown"));
}

#[test]
fn codex_guardian_review_marks_parent_auto_review() {
    let e = env();
    write_lines(&rollout(&e, "p1"), &[
        meta("p1", &e.cwd, json!({})),
        ev("task_started", -10),
        json!({"type":"response_item","timestamp":ts(-9),"payload":{"type":"function_call","call_id":"k","name":"shell","arguments":"{\"cmd\":[\"rm\",\"x\"]}"}}),
    ]);
    write_lines(&rollout(&e, "g1"), &[
        meta("g1", &e.cwd, json!({"thread_source":"guardian_review","parent_thread_id":"p1"})),
        ev("task_started", -8),
    ]);
    let s = snap(&mut engine(&e));
    let p = find(&s, "codex-local:p1");
    assert_eq!(p.status.value, StatusValue::Running);
    assert!(p.status.auto_review);
    assert_eq!(find(&s, "codex-local:g1").hierarchy.kind, SessionKind::Automated);
}

// ---------- Reader / health ----------

#[test]
fn incomplete_tail_line_is_buffered() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("a.jsonl");
    fs::write(&p, b"{\"a\":1}\n{\"b\":").unwrap();
    let mut cur = Cursor::default();
    let lim = Limits::default();
    let o = reader::read_new(&p, &mut cur, &lim).unwrap();
    assert_eq!(o.lines.len(), 1);
    OpenOptions::new().append(true).open(&p).unwrap().write_all(b"2}\n").unwrap();
    let o = reader::read_new(&p, &mut cur, &lim).unwrap();
    assert_eq!(o.lines.len(), 1);
    assert_eq!(o.lines[0].bytes, b"{\"b\":2}");
    assert_eq!(o.lines[0].line_no, Some(1));
    assert!(!o.replaced);
}

#[test]
fn small_file_append_is_not_a_replacement_but_shrink_is() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("a.jsonl");
    fs::write(&p, b"{}\n").unwrap();
    let mut cur = Cursor::default();
    let lim = Limits::default();
    reader::read_new(&p, &mut cur, &lim).unwrap();
    OpenOptions::new().append(true).open(&p).unwrap().write_all(b"{\"x\":1}\n").unwrap();
    assert!(!reader::read_new(&p, &mut cur, &lim).unwrap().replaced);
    fs::write(&p, b"{}\n").unwrap();
    let o = reader::read_new(&p, &mut cur, &lim).unwrap();
    assert!(o.replaced);
    assert_eq!(o.lines.len(), 1);
}

#[test]
fn replaced_file_with_new_id_is_detected() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("a.jsonl");
    fs::write(&p, b"{\"n\":1}\n").unwrap();
    let mut cur = Cursor::default();
    let lim = Limits::default();
    reader::read_new(&p, &mut cur, &lim).unwrap();
    let other = tmp.path().join("b.jsonl");
    fs::write(&other, b"{\"n\":1}\n{\"n\":2}\n").unwrap();
    fs::remove_file(&p).unwrap();
    fs::rename(&other, &p).unwrap();
    let o = reader::read_new(&p, &mut cur, &lim).unwrap();
    assert!(o.replaced);
    assert_eq!(o.lines.len(), 2);
}

#[test]
fn oversized_line_is_dropped() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("a.jsonl");
    let big = "x".repeat(100);
    fs::write(&p, format!("{{\"a\":1}}\n\"{big}\"\n{{\"b\":2}}\n")).unwrap();
    let lim = Limits { max_line: 50, ..Limits::default() };
    let o = reader::read_new(&p, &mut Cursor::default(), &lim).unwrap();
    assert_eq!(o.lines.len(), 2);
    assert_eq!(o.oversized_lines, 1);
    assert_eq!(o.lines[1].line_no, Some(2));
}

#[test]
fn invalid_utf8_line_is_diagnosed_and_lowers_confidence() {
    let e = env();
    let f = e.claude.join("p").join("s5.jsonl");
    write_lines(&f, &[c_user(&e, "s5", json!("hello"), -5)]);
    let mut bad = OpenOptions::new().append(true).open(&f).unwrap();
    bad.write_all(b"{\"type\":\"user\",\"x\":\"\xff\xfe\"}\n").unwrap();
    drop(bad);
    write_lines(&f, &[c_asst(&e, "s5", json!([{"type":"text","text":"hi"}]), "end_turn", -4)]);
    let s = snap(&mut engine(&e));
    let x = find(&s, "claude-local:s5");
    assert!(x.health.diagnostics.iter().any(|d| d.code == "invalid_utf8"));
    assert_eq!(x.health.completeness, Completeness::Partial);
    assert_eq!(x.status.value, StatusValue::WaitingInput);
    assert_eq!(x.status.confidence, Confidence::Low);
    assert!(!x.current_task.last_assistant_message.as_deref().unwrap().contains('\u{FFFD}'));
}

#[test]
fn large_file_head_and_tail_restore_title_and_latest_state() {
    let e = env();
    let f = e.claude.join("p").join("s6.jsonl");
    let mut lines = vec![c_user(&e, "s6", json!("original goal"), -500)];
    for i in 0..400 {
        lines.push(c_asst(&e, "s6", json!([{"type":"text","text":format!("step {i} {}", "y".repeat(200))}]), "end_turn", -400 + i));
    }
    lines.push(c_user(&e, "s6", json!("latest ask"), -3));
    lines.push(c_asst(&e, "s6", json!([{"type":"tool_use","id":"z","name":"Bash","input":{"command":"make"}}]), "tool_use", -2));
    write_lines(&f, &lines);
    let lim = Limits { initial_head: 2048, initial_tail: 4096, ..Limits::default() };
    let mut eng = engine(&e).with_limits(lim);
    let s = snap(&mut eng);
    let x = find(&s, "claude-local:s6");
    assert_eq!(x.current_task.title.as_deref(), Some("original goal"));
    assert_eq!(x.current_task.last_user_message.as_deref(), Some("latest ask"));
    assert_eq!(x.status.value, StatusValue::Running);
    assert_eq!(x.health.completeness, Completeness::Partial);
}

#[test]
fn source_health_distinguishes_missing_and_empty() {
    let e = env();
    fs::create_dir_all(&e.codex).unwrap();
    let s = snap(&mut engine(&e));
    let c = s.sources.iter().find(|x| x.source_id == "claude-local").unwrap();
    assert_eq!(c.state, SourceState::NotInstalled);
    let x = s.sources.iter().find(|x| x.source_id == "codex-local").unwrap();
    assert_eq!(x.state, SourceState::Empty);
}

#[test]
fn redaction_applies_before_snapshot() {
    let e = env();
    write_lines(&e.claude.join("p").join("s7.jsonl"), &[c_user(&e, "s7", json!("use key sk-ant-abcdefghijklmnopqrstuvwxyz"), -1)]);
    let s = snap(&mut engine(&e));
    let t = find(&s, "claude-local:s7").current_task.title.clone().unwrap();
    assert!(t.contains("[REDACTED]"));
    assert!(!t.contains("abcdefghij"));
}

#[test]
fn old_files_are_skipped_and_counted() {
    let e = env();
    write_lines(&e.claude.join("p").join("old.jsonl"), &[c_user(&e, "old", json!("x"), -1)]);
    let mut eng = engine(&e).with_max_age(std::time::Duration::from_secs(0));
    std::thread::sleep(std::time::Duration::from_millis(20));
    let s = snap(&mut eng);
    assert!(s.sessions.is_empty());
    assert_eq!(s.sources[0].files_skipped_old, 1);
}

#[test]
fn orphaned_subagent_idles_when_parent_turn_ended_later() {
    let e = env();
    let parent = e.claude.join("p").join("s8.jsonl");
    let child = e.claude.join("p").join("s8").join("subagents").join("agent-q.jsonl");
    let mut cu = c_asst(&e, "s8", json!([{"type":"tool_use","id":"x","name":"Grep","input":{"pattern":"a"}}]), "tool_use", -50);
    cu["isSidechain"] = json!(true);
    cu["agentId"] = json!("q");
    write_lines(&child, &[cu]);
    write_lines(&parent, &[
        c_user(&e, "s8", json!("go"), -60),
        c_asst(&e, "s8", json!([{"type":"text","text":"done"}]), "end_turn", -10),
    ]);
    let s = snap(&mut engine(&e));
    let c = find(&s, "claude-local:s8::agent:q");
    assert_eq!(c.status.value, StatusValue::Idle);
    assert_eq!(c.status.confidence, Confidence::Low);
}

#[test]
fn error_then_successful_turn_clears_error() {
    let e = env();
    write_lines(&rollout(&e, "c9"), &[
        meta("c9", &e.cwd, json!({})),
        ev("task_started", -10),
        json!({"type":"event_msg","timestamp":ts(-9),"payload":{"type":"task_complete","error":{"message":"boom"}}}),
        ev("task_started", -8),
        ev("task_complete", -7),
    ]);
    assert_eq!(find(&snap(&mut engine(&e)), "codex-local:c9").status.value, StatusValue::WaitingInput);
}
