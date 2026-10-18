use lens_core::ndjson::{Accepted, Outcome, Receiver};

const HELLO: &str = r#"{"type":"hello","protocol":1,"helperVersion":"0.1","distro":"Ubuntu","home":"/home/u","now":"2026-01-01T00:00:00Z"}"#;

#[test]
fn f13a_protocol_mismatch_stops() {
    let mut r = Receiver::new();
    assert!(r.restart_allowed());
    let l = r#"{"type":"hello","protocol":2,"helperVersion":"0.1","distro":"Ubuntu","home":"/home/u","now":"2026-01-01T00:00:00Z"}"#;
    r.handle_line(l);
    assert_eq!(r.count("helper_version_mismatch"), 1);
    assert!(r.is_stopped());
    assert!(!r.restart_allowed());
    assert_eq!(r.handle_line(HELLO), Outcome::Stopped);
}

#[test]
fn f13b_missing_required_field() {
    let mut r = Receiver::new();
    assert_eq!(r.handle_line(HELLO), Outcome::Accepted(Accepted::Hello));
    r.handle_line(r#"{"type":"heartbeat"}"#);
    assert_eq!(r.count("invalid_message"), 1);
    assert_eq!(r.count("malformed_record"), 0);
    assert!(!r.is_stopped());
    assert_eq!(
        r.handle_line(r#"{"type":"heartbeat","now":"2026-01-01T00:00:10Z"}"#),
        Outcome::Accepted(Accepted::Heartbeat)
    );
}

#[test]
fn f13c_unknown_type_ignored() {
    let mut r = Receiver::new();
    r.handle_line(r#"{"type":"foo"}"#);
    assert_eq!(r.count("unknown_message"), 1);
    assert_eq!(r.diagnostics().len(), 1);
    assert!(r.hello().is_none() && r.sessions().is_empty() && !r.is_stopped());
    // unknown fields are ignored
    let l = r#"{"type":"hello","protocol":1,"helperVersion":"0.1","distro":"U","home":"/h","now":"2026-01-01T00:00:00Z","extra":1}"#;
    assert_eq!(r.handle_line(l), Outcome::Accepted(Accepted::Hello));
}

#[test]
fn f13f_upsert_before_hello() {
    for l in [
        r#"{"type":"session_upsert","session":{"sessionUid":"a:b"}}"#,
        r#"{"type":"source_health","sourceId":"wsl:U:claude","health":{}}"#,
    ] {
        let mut r = Receiver::new();
        r.handle_line(l);
        assert_eq!(r.count("invalid_message"), 1);
        assert!(r.sessions().is_empty());
        assert!(r.source_health().is_empty());
    }
}

fn hello_fed() -> Receiver {
    let mut r = Receiver::new();
    r.feed(format!("{HELLO}\n").as_bytes());
    assert!(r.hello().is_some());
    r
}

const HB: &str = r#"{"type":"heartbeat","now":"2026-01-01T00:00:10Z"}"#;

#[test]
fn f13d_oversized_line() {
    let mut r = hello_fed();
    let mut data = vec![b'a'; 5 * 1024 * 1024];
    data.push(b'\n');
    data.extend_from_slice(format!("{HB}\n").as_bytes());
    let out = r.feed(&data);
    assert_eq!(r.count("oversized_line"), 1);
    assert_eq!(out, vec![Outcome::Accepted(Accepted::Heartbeat)]);
    assert_eq!(r.last_heartbeat(), Some("2026-01-01T00:00:10Z"));
}

#[test]
fn f13e_malformed_json() {
    let mut r = hello_fed();
    r.feed(b"{\"type\":\n");
    assert_eq!(r.count("malformed_record"), 1);
    assert_eq!(r.count("invalid_message"), 0);
    assert_eq!(r.feed(format!("{HB}\n").as_bytes()), vec![Outcome::Accepted(Accepted::Heartbeat)]);
}

#[test]
fn invalid_utf8_line() {
    let mut r = hello_fed();
    let mut data = format!("{HB}\n").into_bytes();
    data.extend_from_slice(b"{\"type\":\"x\x82\xa0\"}\n");
    data.extend_from_slice(format!("{HB}\n").as_bytes());
    let out = r.feed(&data);
    assert_eq!(r.count("invalid_utf8"), 1);
    assert_eq!(r.count("malformed_record"), 0);
    assert_eq!(out.len(), 2);
}

#[test]
fn incoming_session_redacted_again() {
    let mut r = hello_fed();
    let tok = format!("ghp_{}", "a".repeat(36));
    let line = format!(
        r#"{{"type":"session_upsert","session":{{"sessionUid":"wsl:U:claude:s1","nativeId":"s1","agent":"claude_code","sourceId":"wsl:U:claude","origin":{{"kind":"wsl","distro":"U"}},"lastActivitySource":"event","projects":[],"status":{{"value":"idle","reason":"r","confidence":"high","stale":false,"autoReview":false}},"currentTask":{{"title":"t {tok}","lastUserMessage":"u {tok}","activeTool":{{"name":"Bash","input":"echo {tok}"}}}},"hierarchy":{{"childUids":[],"kind":"top_level"}},"health":{{"completeness":"complete","diagnostics":[]}}}}}}"#
    );
    // split across two chunks to exercise buffering
    let b = line.as_bytes();
    assert!(r.feed(&b[..50]).is_empty());
    let out = r.feed(format!("{}\n", &line[50..]).as_bytes());
    assert_eq!(out, vec![Outcome::Accepted(Accepted::SessionUpsert)]);
    let s = &r.sessions()["wsl:U:claude:s1"];
    let t = &s.current_task;
    for v in [t.title.as_deref().unwrap(), t.last_user_message.as_deref().unwrap(), &t.active_tool.as_ref().unwrap().input] {
        assert!(v.contains("[REDACTED]"), "{v}");
        assert!(!v.contains(&tok), "{v}");
    }
}
