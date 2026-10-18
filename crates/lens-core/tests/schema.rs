use jsonschema::Validator;
use lens_core::model::v5::*;
use lens_core::model::{
    Agent, Attribution, Completeness, Confidence, Diagnostic, SessionKind, StatusValue, TimeSource,
};
use serde_json::{json, Value};

fn schema() -> Value {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/schema/lens.schema.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn validator(def: &str) -> Validator {
    let mut s = schema();
    s["$ref"] = json!(format!("#/definitions/{def}"));
    jsonschema::draft7::new(&s).unwrap()
}

fn check<T: serde::Serialize>(def: &str, v: &T) {
    let val = serde_json::to_value(v).unwrap();
    let r = validator(def).validate(&val);
    assert!(r.is_ok(), "{def} invalid: {val} => {:?}", r.err().map(|e| e.to_string()));
}

const TS: &str = "2026-01-01T00:00:00Z";

fn origin() -> Origin {
    Origin { kind: OriginKind::Wsl, distro: Some("Ubuntu".into()) }
}
fn project() -> ProjectActivity {
    ProjectActivity {
        key: "c:/work/a".into(),
        name: "a".into(),
        origin: Origin { kind: OriginKind::Windows, distro: None },
        last_updated_at: Some(TS.into()),
        last_updated_source: TimeSource::Event,
        updating: true,
        updating_confidence: Some(Confidence::High),
        origin_available: true,
        agents: vec![Agent::ClaudeCode, Agent::Codex],
        session_uids: vec!["claude-local:s1".into()],
    }
}
fn session() -> UnifiedSession {
    UnifiedSession {
        session_uid: session_uid("claude-local", "s1"),
        native_id: "s1".into(),
        agent: Agent::ClaudeCode,
        source_id: "claude-local".into(),
        origin: origin(),
        last_activity_at: Some(TS.into()),
        last_activity_source: TimeSource::FileMtime,
        projects: vec![SessionProject {
            key: "k".into(),
            name: "n".into(),
            display_path: "/p".into(),
            attribution: Attribution::GitRoot,
        }],
        status: Status {
            value: StatusValue::WaitingInput,
            reason: "r".into(),
            confidence: Confidence::Low,
            stale: false,
            auto_review: false,
        },
        current_task: CurrentTask {
            title: Some("t".into()),
            active_tool: Some(ActiveTool {
                name: "Bash".into(),
                input: "ls".into(),
                call_id: None,
                started_at: None,
            }),
            ..Default::default()
        },
        hierarchy: Hierarchy { parent_uid: None, child_uids: vec!["x".into()], kind: SessionKind::TopLevel },
        health: Health {
            completeness: Completeness::Partial,
            diagnostics: vec![Diagnostic { code: "c".into(), count: 1 }],
        },
    }
}
fn health(state: SourceState) -> SourceHealth {
    SourceHealth {
        source_id: "wsl:Ubuntu:codex".into(),
        agent: Some(Agent::Codex),
        origin: origin(),
        root_path: "/home/u/.codex".into(),
        state,
        last_scan_at: Some(TS.into()),
        files_tracked: 1,
        sessions_total: 2,
        partial_sessions: 0,
        skipped_old_files: 3,
        top_diagnostics: vec![],
    }
}

#[test]
fn schema_file_equals_design_block() {
    let doc = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/architecture-v5.md"))
        .unwrap()
        .replace("\r\n", "\n");
    let s7 = doc.find("## 7.").unwrap();
    let fence = format!("{}json\n", "`".repeat(3));
    let a = s7 + doc[s7..].find(&fence).unwrap() + fence.len();
    let b = a + doc[a..].find(&"`".repeat(3)).unwrap();
    let from_doc: Value = serde_json::from_str(&doc[a..b]).unwrap();
    assert_eq!(from_doc, schema());
    let mut keys: Vec<_> = schema()["definitions"].as_object().unwrap().keys().cloned().collect();
    keys.sort();
    let mut want: Vec<String> = [
        "ts", "N", "Agent", "Src", "Conf", "Diag", "Origin", "ProjectActivity", "UnifiedSession",
        "SourceHealth", "Settings", "IpcError", "In", "Detail", "Ev", "Msg",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    want.sort();
    assert_eq!(keys, want);
}

#[test]
fn samples_validate_all_types() {
    check("ProjectActivity", &project());
    check("UnifiedSession", &session());
    check("SourceHealth", &health(SourceState::Ok));
    check(
        "Settings",
        &Settings {
            version: 1,
            helper_path: Some("/usr/bin/h".into()),
            disabled_distros: Some(vec!["Debian".into()]),
            source_roots: Some([("claude-local".to_string(), "D:/x".to_string())].into()),
        },
    );
    check("IpcError", &IpcError { code: IpcErrorCode::PathInvalid, message: "m".into() });
    check("Detail", &Detail { project: project(), sessions: vec![session()] });
    check("In", &In::Empty {});
    check("In", &In::Key { key: "k".into() });
    check("In", &In::Distro { distro: "d".into(), enabled: true });
    check("In", &In::SourceRoot { source_id: "s".into(), path: None });
    check(
        "Ev",
        &Ev::ProjectsChanged { upserts: vec![project()], removes: vec!["a".into()], order: vec!["b".into()] },
    );
    check("Ev", &Ev::ProjectDetailChanged { key: "k".into() });
    check("Ev", &Ev::HealthChanged { sources: vec![health(SourceState::DistroStopped)] });
    check(
        "Msg",
        &Msg::Hello {
            protocol: 1,
            helper_version: "0.1".into(),
            distro: "Ubuntu".into(),
            home: "/home/u".into(),
            now: TS.into(),
        },
    );
    check("Msg", &Msg::SourceHealth { source_id: "wsl:Ubuntu:codex".into(), health: health(SourceState::Ok) });
    check("Msg", &Msg::SessionUpsert { session: session() });
    check("Msg", &Msg::SessionRemove { session_uid: "u".into() });
    check("Msg", &Msg::Heartbeat { now: TS.into() });
    check("Msg", &Msg::Error { code: HelperErrorCode::RootUnreadable });
}

#[test]
fn invalid_hello_missing_home_rejected() {
    let v = json!({"type":"hello","protocol":1,"helperVersion":"0.1","distro":"Ubuntu","now":TS});
    assert!(validator("Msg").validate(&v).is_err());
}

#[test]
fn enum_sizes_source_health_and_ipc_error() {
    let s = schema();
    let st = s["definitions"]["SourceHealth"]["properties"]["state"]["enum"].as_array().unwrap();
    assert_eq!(st.len(), 10);
    let er = s["definitions"]["IpcError"]["properties"]["code"]["enum"].as_array().unwrap();
    assert_eq!(er.len(), 7);
    for v in st {
        let s: SourceState = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(&serde_json::to_value(s).unwrap(), v);
    }
    for v in er {
        let c: IpcErrorCode = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(&serde_json::to_value(c).unwrap(), v);
    }
}

#[test]
fn msg_unknown_fields_ignored() {
    let m: Msg = serde_json::from_str(r#"{"type":"heartbeat","now":"2026-01-01T00:00:00Z","extra":1}"#).unwrap();
    assert_eq!(m, Msg::Heartbeat { now: TS.into() });
}

#[test]
fn session_uid_and_source_id_forms() {
    assert_eq!(session_uid("claude-local", "s1"), "claude-local:s1");
    assert_eq!(wsl_source_id("Ubuntu", Agent::Codex), "wsl:Ubuntu:codex");
    assert_eq!(wsl_source_id("Ubuntu", Agent::ClaudeCode), "wsl:Ubuntu:claude");
    assert_eq!(UNATTRIBUTED_KEY, "unattributed");
}
