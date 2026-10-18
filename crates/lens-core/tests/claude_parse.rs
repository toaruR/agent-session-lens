//! Claude Code parsing (architecture-v5 section 5). Synthetic fixtures only.

use chrono::{DateTime, Duration, Utc};
use lens_core::consts::MAX_LINE_BYTES;
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
    let root = tmp.path().join("projects");
    fs::create_dir_all(&root).unwrap();
    Env { root, _tmp: tmp }
}

fn engine(e: &Env) -> Engine {
    Engine::new(vec![SourceConfig { id: "claude-local".into(), agent: Agent::ClaudeCode, root: e.root.clone() }])
}

fn snap(eng: &mut Engine) -> Snapshot {
    eng.rescan();
    eng.process_all();
    eng.snapshot(Utc::now())
}

fn write_raw(path: &Path, lines: &[String]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = fs::File::create(path).unwrap();
    for l in lines {
        writeln!(f, "{l}").unwrap();
    }
}

fn user(sid: &str, ts: DateTime<Utc>, text: &str) -> Value {
    json!({"type":"user","sessionId":sid,"timestamp":ts.to_rfc3339(),"cwd":"/w/repo","message":{"role":"user","content":text}})
}

fn assistant(sid: &str, ts: DateTime<Utc>) -> Value {
    json!({"type":"assistant","sessionId":sid,"timestamp":ts.to_rfc3339(),"message":{"role":"assistant","content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn"}})
}

#[test]
fn f3_api_request_blob_not_fully_parsed() {
    let e = env();
    let t = Utc::now() - Duration::seconds(100);
    let last = t + Duration::seconds(10);
    let blob = json!({"type":"api-request-blob","sessionId":"s1","timestamp":(t + Duration::seconds(1000)).to_rfc3339(),"blob":"x".repeat(3 * 1024 * 1024)});
    let lines = vec![user("s1", t, "hello").to_string(), blob.to_string(), assistant("s1", last).to_string()];
    assert!(lines[1].len() > 3 * 1024 * 1024 && lines[1].len() <= MAX_LINE_BYTES);
    write_raw(&e.root.join("enc").join("s1.jsonl"), &lines);
    let mut eng = engine(&e);
    let s = snap(&mut eng);
    assert_eq!(eng.full_parse_count("api-request-blob"), 0);
    assert_eq!(eng.full_parse_count("user"), 1);
    assert_eq!(eng.full_parse_count("assistant"), 1);
    let ses = s.sessions.iter().find(|x| x.session_uid == "claude-local:s1").unwrap();
    assert_eq!(ses.timestamps.last_active_at.as_deref(), Some(last.to_rfc3339().as_str()));
}

fn typeless_line(total: usize) -> String {
    let base = json!({"pad": ""}).to_string();
    let line = format!("{}{}{}", &base[..base.len() - 2], "a".repeat(total - base.len()), "\"}");
    assert_eq!(line.len(), total);
    serde_json::from_str::<Value>(&line).unwrap();
    line
}

#[test]
fn typeless_line_parsed_only_up_to_max_line_bytes() {
    let t = Utc::now() - Duration::seconds(100);
    // Exactly MAX_LINE_BYTES: fully parsed once.
    let e = env();
    write_raw(&e.root.join("enc").join("s1.jsonl"), &[user("s1", t, "hi").to_string(), typeless_line(MAX_LINE_BYTES)]);
    let mut eng = engine(&e);
    let s = snap(&mut eng);
    assert_eq!(eng.full_parse_count(""), 1);
    let ses = s.sessions.iter().find(|x| x.session_uid == "claude-local:s1").unwrap();
    assert!(ses.health.diagnostics.iter().all(|d| d.code != "oversized_line"));

    // One byte more: dropped with oversized_line, never parsed.
    let e = env();
    write_raw(&e.root.join("enc").join("s1.jsonl"), &[user("s1", t, "hi").to_string(), typeless_line(MAX_LINE_BYTES + 1)]);
    let mut eng = engine(&e);
    let s = snap(&mut eng);
    assert_eq!(eng.full_parse_count(""), 0);
    let ses = s.sessions.iter().find(|x| x.session_uid == "claude-local:s1").unwrap();
    let d = ses.health.diagnostics.iter().find(|d| d.code == "oversized_line").expect("oversized_line");
    assert_eq!(d.count, 1);
}

#[test]
fn mixed_session_ids_excluded() {
    let e = env();
    let t = Utc::now() - Duration::seconds(100);
    write_raw(
        &e.root.join("enc").join("a.jsonl"),
        &[user("a", t, "one").to_string(), user("b", t + Duration::seconds(5), "two").to_string(), assistant("a", t + Duration::seconds(6)).to_string()],
    );
    let mut eng = engine(&e);
    let s = snap(&mut eng);
    // The foreign session never appears.
    assert!(s.sessions.iter().all(|x| x.session_uid != "claude-local:b"));
    let ses = s.sessions.iter().find(|x| x.session_uid == "claude-local:a").unwrap();
    let d = ses.health.diagnostics.iter().find(|d| d.code == "mixed_session_ids").expect("mixed_session_ids");
    assert_eq!(d.count, 1);
    assert_eq!(ses.health.completeness, Completeness::Partial);
    // The foreign record is not counted as activity.
    assert_eq!(ses.timestamps.last_active_at.as_deref(), Some((t + Duration::seconds(6)).to_rfc3339().as_str()));
}

#[test]
fn subagent_uid_and_shared_session_id() {
    let e = env();
    let t = Utc::now() - Duration::seconds(100);
    write_raw(&e.root.join("enc").join("s1.jsonl"), &[user("s1", t, "parent").to_string()]);
    let mut sub = user("s1", t + Duration::seconds(1), "child task");
    sub["isSidechain"] = json!(true);
    sub["agentId"] = json!("x1");
    write_raw(&e.root.join("enc").join("s1").join("subagents").join("agent-x1.jsonl"), &[sub.to_string()]);
    let mut eng = engine(&e);
    let s = snap(&mut eng);
    let child = s.sessions.iter().find(|x| x.session_uid == "claude-local:s1::agent:x1").expect("child");
    assert_eq!(child.native_id, "s1::agent:x1");
    assert_eq!(child.hierarchy.parent_uid.as_deref(), Some("claude-local:s1"));
    assert_eq!(child.hierarchy.kind, SessionKind::Subagent);
    let parent = s.sessions.iter().find(|x| x.session_uid == "claude-local:s1").unwrap();
    assert_eq!(parent.hierarchy.child_uids, vec!["claude-local:s1::agent:x1".to_string()]);
}

fn one(sid: &str, lines: Vec<Value>) -> (UnifiedSession, ()) {
    let e = env();
    let strs: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    write_raw(&e.root.join("enc").join(format!("{sid}.jsonl")), &strs);
    let mut eng = engine(&e);
    let s = snap(&mut eng);
    let ses = s.sessions.into_iter().find(|x| x.session_uid == format!("claude-local:{sid}")).unwrap();
    (ses, ())
}

fn rec(ty: &str, sid: &str, ts: DateTime<Utc>) -> Value {
    json!({"type":ty,"sessionId":sid,"timestamp":ts.to_rfc3339(),"cwd":"/w/repo"})
}

fn custom(sid: &str, ts: DateTime<Utc>, t: &str) -> Value {
    let mut v = rec("custom-title", sid, ts);
    v["customTitle"] = json!(t);
    v
}

fn ai(sid: &str, ts: DateTime<Utc>, t: &str) -> Value {
    let mut v = rec("ai-title", sid, ts);
    v["aiTitle"] = json!(t);
    v
}

fn tool_result(sid: &str, ts: DateTime<Utc>, text: &str) -> Value {
    json!({"type":"user","sessionId":sid,"timestamp":ts.to_rfc3339(),"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"c1","content":text}]}})
}

#[test]
fn title_fallback_order() {
    let t = Utc::now() - Duration::seconds(100);
    let mut meta = user("s", t, "META");
    meta["isMeta"] = json!(true);
    let human = |ts| user("s", ts, "H");
    let base = |extra: Vec<Value>| {
        let mut v = vec![meta.clone(), tool_result("s", t, "TR"), human(t + Duration::seconds(1))];
        v.extend(extra);
        v
    };
    // custom-title wins regardless of order.
    let (s, _) = one("s", base(vec![custom("s", t, "C"), ai("s", t, "A")]));
    assert_eq!(s.current_task.title.as_deref(), Some("C"));
    let (s, _) = one("s", base(vec![ai("s", t, "A"), custom("s", t, "C")]));
    assert_eq!(s.current_task.title.as_deref(), Some("C"));
    // ai-title beats the first human message.
    let (s, _) = one("s", base(vec![ai("s", t, "A")]));
    assert_eq!(s.current_task.title.as_deref(), Some("A"));
    // neither: first human message; isMeta and tool_result-only records skipped.
    let (s, _) = one("s", base(vec![]));
    assert_eq!(s.current_task.title.as_deref(), Some("H"));
    // titles before the first human message still win.
    let (s, _) = one("s", vec![custom("s", t, "C"), human(t)]);
    assert_eq!(s.current_task.title.as_deref(), Some("C"));
}

#[test]
fn activity_events_exclusions() {
    let t = Utc::now() - Duration::seconds(300);
    let base = user("s", t, "hi");
    let later = t + Duration::seconds(50);
    let mut meta = user("s", later, "meta");
    meta["isMeta"] = json!(true);
    let blob = json!({"type":"api-request-blob","sessionId":"s","timestamp":later.to_rfc3339(),"blob":"x"});
    let mut sys_other = rec("system", "s", later);
    sys_other["subtype"] = json!("informational");
    let excluded = vec![
        custom("s", later, "C"),
        ai("s", later, "A"),
        rec("summary", "s", later),
        rec("attachment", "s", later),
        meta,
        blob,
        sys_other,
    ];
    let mut lines = vec![base.clone()];
    lines.extend(excluded);
    let (s, _) = one("s", lines);
    assert_eq!(s.timestamps.last_active_at.as_deref(), Some(t.to_rfc3339().as_str()));

    let mut api = rec("system", "s", later);
    api["subtype"] = json!("api_error");
    let cases: Vec<Value> = vec![user("s", later, "more"), assistant("s", later), tool_result("s", later, "ok"), api];
    for c in cases {
        let (s, _) = one("s", vec![base.clone(), c.clone()]);
        assert_eq!(s.timestamps.last_active_at.as_deref(), Some(later.to_rfc3339().as_str()), "{c}");
    }
}

#[test]
fn turn_signals() {
    let t = Utc::now() - Duration::seconds(20);
    let t1 = t + Duration::seconds(1);
    let t2 = t + Duration::seconds(2);
    // human opens a turn.
    let (s, _) = one("s", vec![user("s", t, "go")]);
    assert_eq!(s.status.value, StatusValue::Running);
    assert!(s.status.reason.starts_with("turn open"), "{}", s.status.reason);
    // end_turn closes it as completed.
    let (s, _) = one("s", vec![user("s", t, "go"), assistant("s", t1)]);
    assert_eq!(s.status.value, StatusValue::WaitingInput);
    assert!(s.status.reason.starts_with("turn complete"), "{}", s.status.reason);
    // tool_use stop_reason does not close it.
    let mut tu = assistant("s", t1);
    tu["message"]["stop_reason"] = json!("tool_use");
    let (s, _) = one("s", vec![user("s", t, "go"), tu]);
    assert_eq!(s.status.value, StatusValue::Running);
    // interruption text closes it as aborted.
    let (s, _) = one("s", vec![user("s", t, "go"), tool_result("s", t1, "[Request interrupted by user for tool use]")]);
    assert!(s.status.reason.starts_with("turn aborted"), "{}", s.status.reason);
    let (s, _) = one("s", vec![user("s", t, "go"), user("s", t1, "[Request interrupted by user]")]);
    assert!(s.status.reason.starts_with("turn aborted"), "{}", s.status.reason);
    // system/api_error ends it as error.
    let mut api = rec("system", "s", t2);
    api["subtype"] = json!("api_error");
    let (s, _) = one("s", vec![user("s", t, "go"), api]);
    assert_eq!(s.status.value, StatusValue::Error);
    assert!(s.status.reason.contains("api_error"), "{}", s.status.reason);
}
