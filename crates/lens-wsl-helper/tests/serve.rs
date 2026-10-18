#![allow(dead_code)]
use lens_core::clock::FakeClock;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

// serve.rs is compiled into this test crate directly (binary-only crate).
#[path = "../src/serve.rs"]
mod serve;

fn write_session(root: &Path, file: &str, sid: &str, text: &str) -> PathBuf {
    let dir = root.join("proj");
    std::fs::create_dir_all(&dir).unwrap();
    let ts = chrono::Utc::now().to_rfc3339();
    let u = json!({"type":"user","sessionId":sid,"timestamp":ts,"message":{"role":"user","content":text}});
    let a = json!({"type":"assistant","sessionId":sid,"timestamp":ts,"message":{"role":"assistant","content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn"}});
    let p = dir.join(file);
    std::fs::write(&p, format!("{u}\n{a}\n")).unwrap();
    p
}

struct Fixture {
    _tmp: tempfile::TempDir,
    root: PathBuf,
}

fn fixture(text: &str) -> (Fixture, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("projects");
    let f = write_session(&root, "s1.jsonl", "sess-1", text);
    (Fixture { _tmp: tmp, root }, f)
}

fn cfg(roots: Vec<PathBuf>) -> serve::Config {
    serve::Config { roots, home: PathBuf::from("/home/u"), distro: "Ubuntu".into() }
}

fn lines(buf: &[u8]) -> Vec<Value> {
    String::from_utf8(buf.to_vec()).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

fn types(v: &[Value]) -> Vec<&str> {
    v.iter().map(|m| m["type"].as_str().unwrap()).collect()
}

fn started(roots: Vec<PathBuf>) -> (serve::Server, Arc<FakeClock>, Vec<u8>) {
    let clock = Arc::new(FakeClock::new(SystemTime::now()));
    let mut s = serve::Server::new(cfg(roots), clock.clone());
    let mut out = Vec::new();
    s.start(&mut out).unwrap();
    (s, clock, out)
}

#[test]
fn first_line_is_hello_protocol_1() {
    let (fx, _) = fixture("hi");
    let (_s, _c, out) = started(vec![fx.root.clone()]);
    let m = lines(&out);
    assert_eq!(m[0]["type"], "hello");
    assert_eq!(m[0]["protocol"], 1);
    assert_eq!(m[0]["distro"], "Ubuntu");
    assert_eq!(m[0]["home"], "/home/u");
    assert!(m[0]["helperVersion"].is_string() && m[0]["now"].is_string());
    let t = types(&m);
    let last_health = t.iter().rposition(|x| *x == "source_health").unwrap();
    let first_upsert = t.iter().position(|x| *x == "session_upsert").unwrap();
    assert!(last_health < first_upsert, "{t:?}");
    // default roots -> one source_health per source (claude and codex)
    let (_s2, _c2, out2) = started(vec![]);
    let h = lines(&out2);
    assert_eq!(types(&h).iter().filter(|x| **x == "source_health").count(), 2);
    assert_eq!(h[1]["sourceId"], "wsl:Ubuntu:claude");
    assert_eq!(h[2]["sourceId"], "wsl:Ubuntu:codex");
}

#[test]
fn binary_streams_hello_first() {
    let (fx, _) = fixture("hi");
    let mut child = Command::new(env!("CARGO_BIN_EXE_lens-wsl-helper"))
        .args(["serve", "--protocol", "1", "--roots"])
        .arg(&fx.root)
        .env("HOME", "/home/u")
        .env("WSL_DISTRO_NAME", "Ubuntu")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut first = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut first).unwrap();
    let _ = child.kill();
    let _ = child.wait();
    let v: Value = serde_json::from_str(&first).unwrap();
    assert_eq!(v["type"], "hello");
    assert_eq!(v["distro"], "Ubuntu");
}

#[test]
fn partial_unreadable_root_continues() {
    let (fx, _) = fixture("hi");
    let bad = fx.root.parent().unwrap().join("unreadable");
    let (_s, _c, out) = started(vec![bad, fx.root.clone()]);
    let m = lines(&out);
    let err = m.iter().find(|x| x["type"] == "error").expect("error line");
    assert_eq!(err["code"], "root_unreadable");
    assert!(m.iter().any(|x| x["type"] == "session_upsert"));
}

#[test]
fn no_raw_token_in_output() {
    let token = format!("ghp_{}", "a1B2c3D4e5".repeat(4).chars().take(36).collect::<String>());
    let (fx, _) = fixture(&format!("please use {token} now"));
    let (_s, _c, out) = started(vec![fx.root.clone()]);
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("[REDACTED]"), "{s}");
    assert!(!s.contains(&token));
}

#[test]
fn every_line_valid_and_bounded() {
    let (fx, _) = fixture(&"long text ".repeat(5000));
    let (mut s, c, mut out) = started(vec![fx.root.clone(), fx.root.parent().unwrap().join("missing")]);
    c.advance(Duration::from_secs(10));
    s.tick(&mut out).unwrap();
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../lens-core/schema/lens.schema.json");
    let mut schema: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    schema["$ref"] = json!("#/definitions/Msg");
    let v = jsonschema::draft7::new(&schema).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.lines().count() >= 4);
    for l in text.lines() {
        assert!(l.len() < 4194304);
        let val: Value = serde_json::from_str(l).unwrap();
        assert!(v.validate(&val).is_ok(), "invalid: {l}");
    }
    assert!(types(&lines(text.as_bytes())).contains(&"heartbeat"));
}

#[test]
fn heartbeat_every_10s_fake_clock() {
    let (fx, f) = fixture("hi");
    let (mut s, c, _) = started(vec![fx.root.clone()]);
    let count = |buf: &[u8]| lines(buf).iter().filter(|m| m["type"] == "heartbeat").count();
    let mut out = Vec::new();
    c.advance(Duration::from_secs(9));
    s.tick(&mut out).unwrap();
    assert_eq!(count(&out), 0);
    c.advance(Duration::from_secs(1));
    s.tick(&mut out).unwrap();
    assert_eq!(count(&out), 1);
    // unchanged session is not re-sent
    assert!(!types(&lines(&out)).contains(&"session_upsert"));

    std::fs::remove_file(&f).unwrap();
    let mut out2 = Vec::new();
    c.advance(Duration::from_secs(60));
    s.tick(&mut out2).unwrap();
    let m = lines(&out2);
    let rm = m.iter().find(|x| x["type"] == "session_remove").expect("session_remove");
    assert_eq!(rm["sessionUid"], "wsl:Ubuntu:claude:sess-1");
}

#[cfg(feature = "fault_inject")]
#[test]
fn f29e_panic_exits_101() {
    let o = Command::new(env!("CARGO_BIN_EXE_lens-wsl-helper"))
        .args(["serve", "--protocol", "1"])
        .env("LENS_FAULT", "panic")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(101));
    assert_eq!(String::from_utf8_lossy(&o.stderr), "internal");
}
