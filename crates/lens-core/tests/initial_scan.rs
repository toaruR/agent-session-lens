//! Architecture-v5 initial scan policy (section 2 INITIAL_SCAN_DAYS / HEAD_SCAN_BYTES / TAIL_SCAN_BYTES),
//! section 3.1 rule 1 and SourceHealth states. Time is injected through `FakeClock`.

use chrono::{DateTime, Utc};
use lens_core::clock::FakeClock;
use lens_core::consts::{HEAD_SCAN_BYTES, INITIAL_SCAN_DAYS, TAIL_SCAN_BYTES};
use lens_core::engine::{Engine, SourceConfig};
use lens_core::model::*;
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

fn now_fixed() -> SystemTime {
    // Whole seconds so the boundary arithmetic is exact.
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)
}

fn rfc(t: SystemTime, off_secs: i64) -> String {
    (DateTime::<Utc>::from(t) + chrono::Duration::seconds(off_secs)).to_rfc3339()
}

fn user(sid: &str, text: &str, off: i64) -> Value {
    json!({"type":"user","sessionId":sid,"timestamp":rfc(now_fixed(), off),"message":{"role":"user","content":text}})
}

fn asst(sid: &str, blocks: Value, stop: &str, off: i64) -> Value {
    json!({"type":"assistant","sessionId":sid,"timestamp":rfc(now_fixed(), off),"message":{"role":"assistant","content":blocks,"stop_reason":stop}})
}

fn write_lines(path: &Path, lines: &[Value]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = OpenOptions::new().create(true).append(true).open(path).unwrap();
    for l in lines {
        writeln!(f, "{l}").unwrap();
    }
}

fn set_mtime(path: &Path, t: SystemTime) {
    OpenOptions::new().write(true).open(path).unwrap().set_modified(t).unwrap();
}

fn engine(root: &Path) -> Engine {
    Engine::new(vec![SourceConfig { id: "claude-local".into(), agent: Agent::ClaudeCode, root: root.to_path_buf() }])
        .with_clock(Arc::new(FakeClock::new(now_fixed())))
}

fn snap(eng: &mut Engine) -> Snapshot {
    eng.rescan();
    eng.process_all();
    eng.snapshot(DateTime::<Utc>::from(now_fixed()))
}

fn root() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let r = tmp.path().join("projects");
    (tmp, r)
}

#[test]
fn f21_seven_day_boundary() {
    let (_t, r) = root();
    let seven = Duration::from_secs(INITIAL_SCAN_DAYS * 86400);
    let exact = r.join("p").join("exact.jsonl");
    let older = r.join("p").join("older.jsonl");
    write_lines(&exact, &[user("exact", "hello", -1)]);
    write_lines(&older, &[user("older", "hello", -1)]);
    set_mtime(&exact, now_fixed() - seven);
    set_mtime(&older, now_fixed() - seven - Duration::from_secs(1));

    let mut eng = engine(&r);
    let s = snap(&mut eng);
    assert_eq!(s.sessions.len(), 1);
    assert_eq!(s.sessions[0].native_id, "exact");
    assert_eq!(s.sources[0].files_skipped_old, 1);
    assert_eq!(s.sources[0].files_tracked, 1);
}

#[test]
fn large_file_head_tail_truncated() {
    let (_t, r) = root();
    let f = r.join("p").join("big.jsonl");
    let target = HEAD_SCAN_BYTES + TAIL_SCAN_BYTES + 1;
    let head = user("big", "original goal", -500).to_string() + "\n";
    let tail = [user("big", "latest ask", -3), asst("big", json!([{"type":"tool_use","id":"z","name":"Bash","input":{"command":"make"}}]), "tool_use", -2)];
    let tail_bytes: u64 = tail.iter().map(|l| l.to_string().len() as u64 + 1).sum();
    fs::create_dir_all(f.parent().unwrap()).unwrap();
    let mut w = fs::File::create(&f).unwrap();
    w.write_all(head.as_bytes()).unwrap();
    let filler = asst("big", json!([{"type":"text","text":"y".repeat(3000)}]), "end_turn", -400).to_string() + "\n";
    let mut written = head.len() as u64;
    while written + tail_bytes + (filler.len() as u64) < target {
        w.write_all(filler.as_bytes()).unwrap();
        written += filler.len() as u64;
    }
    // Final filler padded so the total is exactly `target`.
    let pad = (target - written - tail_bytes) as usize;
    let tiny = asst("big", json!([{"type":"text","text":"p"}]), "end_turn", -400).to_string();
    assert!(pad > tiny.len() + 1);
    let padded = asst("big", json!([{"type":"text","text":"p".repeat(1 + pad - tiny.len() - 1)}]), "end_turn", -400).to_string() + "\n";
    assert_eq!(padded.len(), pad);
    w.write_all(padded.as_bytes()).unwrap();
    for l in &tail {
        writeln!(w, "{l}").unwrap();
    }
    drop(w);
    set_mtime(&f, now_fixed());
    assert_eq!(fs::metadata(&f).unwrap().len(), target);

    let s = snap(&mut engine(&r));
    let x = s.sessions.iter().find(|x| x.native_id == "big").expect("session");
    assert_eq!(x.current_task.title.as_deref(), Some("original goal"));
    assert_eq!(x.current_task.last_user_message.as_deref(), Some("latest ask"));
    assert_eq!(x.status.value, StatusValue::Running);
    assert_eq!(x.health.completeness, Completeness::Partial);
    let d = x.health.diagnostics.iter().find(|d| d.code == "truncated_initial_scan").expect("diag");
    assert_eq!(d.count, 1);
    assert_eq!(s.sources[0].partial_sessions, 1);
}

#[test]
fn tail_without_complete_line_is_unknown() {
    let (_t, r) = root();
    let f = r.join("p").join("t.jsonl");
    let head = user("t", "goal", -5).to_string() + "\n";
    fs::create_dir_all(f.parent().unwrap()).unwrap();
    let mut w = fs::File::create(&f).unwrap();
    w.write_all(head.as_bytes()).unwrap();
    // One unterminated line that fills the whole tail window.
    w.write_all(&vec![b'x'; (HEAD_SCAN_BYTES + TAIL_SCAN_BYTES) as usize]).unwrap();
    drop(w);
    set_mtime(&f, now_fixed());

    let s = snap(&mut engine(&r));
    let x = s.sessions.iter().find(|x| x.native_id == "t").expect("session");
    assert_eq!((x.status.value, x.status.confidence, x.status.stale), (StatusValue::Unknown, Confidence::Low, false));
}

#[test]
fn source_states_not_installed_empty_access_denied_ok() {
    // not_installed
    let (_t, r) = root();
    let s = snap(&mut engine(&r));
    assert_eq!(s.sources[0].state, SourceState::NotInstalled);

    // empty
    fs::create_dir_all(&r).unwrap();
    let s = snap(&mut engine(&r));
    assert_eq!(s.sources[0].state, SourceState::Empty);

    // access_denied: the injected opener denies (chmod cannot create this on Windows)
    let denied: lens_core::engine::DirOpener =
        Arc::new(|_p: &Path| Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)));
    let s = snap(&mut engine(&r).with_dir_opener(denied));
    assert_eq!(s.sources[0].state, SourceState::AccessDenied);

    // ok
    let f = r.join("p").join("ok.jsonl");
    write_lines(&f, &[user("ok", "hi", -1)]);
    set_mtime(&f, now_fixed());
    let s = snap(&mut engine(&r));
    assert_eq!(s.sources[0].state, SourceState::Ok);
    assert_eq!(s.sources[0].files_tracked, 1);
    assert_eq!(s.sources[0].sessions_total, 1);
}
