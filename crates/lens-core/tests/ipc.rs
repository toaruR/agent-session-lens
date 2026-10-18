//! IPC read commands (architecture-v5 section 8): F25a, F25c. No Tauri; Aggregator start state is a fake.

use lens_core::ipc::{
    ev_health_changed, ev_project_detail_changed, ev_projects_changed, get_health, get_project_detail,
    list_projects, set_distro_enabled, set_source_root, IpcRead, PathProbe, Phase,
};
use lens_core::model::v5::{Ev, IpcError};
use lens_core::settings::{RealFs, SettingsFs, SettingsStore};
use std::path::Path;
use lens_core::model::v5::{
    Detail, IpcErrorCode, Origin, OriginKind, ProjectActivity, SourceHealth, SourceState,
};
use lens_core::model::{Agent, TimeSource};
use std::cell::Cell;

struct Fake {
    phase: Cell<Phase>,
}

fn row(key: &str, at: Option<&str>) -> ProjectActivity {
    ProjectActivity {
        key: key.into(),
        name: key.into(),
        origin: Origin { kind: OriginKind::Windows, distro: None },
        last_updated_at: at.map(String::from),
        last_updated_source: TimeSource::Event,
        updating: false,
        updating_confidence: None,
        origin_available: true,
        agents: vec![Agent::ClaudeCode],
        session_uids: vec![],
    }
}

impl IpcRead for Fake {
    fn phase(&self) -> Phase {
        self.phase.get()
    }
    fn projects(&self) -> Vec<ProjectActivity> {
        vec![
            row("new", Some("2026-10-17T01:00:00Z")),
            row("old", Some("2026-10-17T00:00:00Z")),
        ]
    }
    fn detail(&self, key: &str) -> Option<Detail> {
        (key == "old").then(|| Detail { project: row("old", None), sessions: vec![] })
    }
    fn health(&self) -> Vec<SourceHealth> {
        vec![SourceHealth {
            source_id: "claude-local".into(),
            agent: Some(Agent::ClaudeCode),
            origin: Origin { kind: OriginKind::Windows, distro: None },
            root_path: "C:/x".into(),
            state: SourceState::Ok,
            last_scan_at: None,
            files_tracked: 0,
            sessions_total: 0,
            partial_sessions: 0,
            skipped_old_files: 0,
            top_diagnostics: vec![],
        }]
    }
}

fn fake(p: Phase) -> Fake {
    Fake { phase: Cell::new(p) }
}

#[test]
fn f25a_unknown_key_project_not_found() {
    let f = fake(Phase::Ready);
    assert_eq!(get_project_detail(&f, "nope").unwrap_err().code, IpcErrorCode::ProjectNotFound);
    assert!(get_project_detail(&f, "old").is_ok());
}

#[test]
fn f25a_empty_key_invalid_argument() {
    let f = fake(Phase::Ready);
    assert_eq!(get_project_detail(&f, "").unwrap_err().code, IpcErrorCode::InvalidArgument);
}

#[test]
fn f25c_get_health_not_ready_then_partial() {
    let f = fake(Phase::NotStarted);
    assert_eq!(get_health(&f).unwrap_err().code, IpcErrorCode::NotReady);
    f.phase.set(Phase::FirstScan);
    assert!(!get_health(&f).unwrap().is_empty());
    f.phase.set(Phase::Ready);
    assert!(get_health(&f).is_ok());
}

#[test]
fn list_projects_not_ready_during_first_scan() {
    let f = fake(Phase::FirstScan);
    assert_eq!(list_projects(&f).unwrap_err().code, IpcErrorCode::NotReady);
    f.phase.set(Phase::Ready);
    let v = list_projects(&f).unwrap();
    let keys: Vec<&str> = v.iter().map(|r| r.key.as_str()).collect();
    assert_eq!(keys, vec!["new", "old"]);
}

// ---- write side ----

struct Probe(bool);
impl PathProbe for Probe {
    fn is_readable(&self, _o: &Origin, _p: &str) -> bool {
        self.0
    }
}

struct FailFs;
impl SettingsFs for FailFs {
    fn read_to_string(&self, _p: &Path) -> std::io::Result<Option<String>> {
        Ok(None)
    }
    fn create_dir_all(&self, _p: &Path) -> std::io::Result<()> {
        Ok(())
    }
    fn write(&self, _p: &Path, _d: &[u8]) -> std::io::Result<()> {
        Err(std::io::Error::other("boom"))
    }
    fn rename(&self, _a: &Path, _b: &Path) -> std::io::Result<()> {
        Ok(())
    }
}

fn store(dir: &Path) -> SettingsStore {
    SettingsStore::load(RealFs, dir)
}

fn sources() -> Vec<String> {
    vec!["claude-local".into(), "codex-local".into(), "wsl:Ubuntu:claude".into()]
}

#[test]
fn f23_unc_wsl_localhost_path_invalid() {
    let d = tempfile::tempdir().unwrap();
    let mut s = store(d.path());
    let unc = r"\\wsl.localhost\Ubuntu\home\u";
    let e = set_source_root(&mut s, &sources(), &Probe(true), "claude-local", Some(unc)).unwrap_err();
    assert_eq!(e.code, IpcErrorCode::PathInvalid);
    assert!(s.settings().source_roots.is_empty());
}

#[test]
fn set_source_root_path_rules() {
    let d = tempfile::tempdir().unwrap();
    let mut s = store(d.path());
    let p = Probe(true);
    let src = sources();
    assert!(set_source_root(&mut s, &src, &p, "claude-local", Some(r"C:\x")).is_ok());
    assert_eq!(s.settings().source_roots.get("claude-local").unwrap(), r"C:\x");
    let code = |r: Result<(), IpcError>| r.unwrap_err().code;
    assert_eq!(code(set_source_root(&mut s, &src, &p, "claude-local", Some(r"x\y"))), IpcErrorCode::PathInvalid);
    assert!(set_source_root(&mut s, &src, &p, "wsl:Ubuntu:claude", Some("/home/u")).is_ok());
    assert_eq!(code(set_source_root(&mut s, &src, &p, "wsl:Ubuntu:claude", Some("home/u"))), IpcErrorCode::PathInvalid);
    assert_eq!(code(set_source_root(&mut s, &src, &p, "nope", Some(r"C:\x"))), IpcErrorCode::SourceNotFound);
    assert_eq!(
        code(set_source_root(&mut s, &src, &Probe(false), "claude-local", Some(r"D:\y"))),
        IpcErrorCode::PathInvalid
    );
    assert!(set_source_root(&mut s, &src, &p, "claude-local", None).is_ok());
    assert!(!s.settings().source_roots.contains_key("claude-local"));
}

#[test]
fn set_distro_enabled_errors() {
    let d = tempfile::tempdir().unwrap();
    let mut s = store(d.path());
    let known = vec!["Ubuntu".to_string()];
    assert_eq!(set_distro_enabled(&mut s, &known, "", false).unwrap_err().code, IpcErrorCode::InvalidArgument);
    assert_eq!(set_distro_enabled(&mut s, &known, "Debian", false).unwrap_err().code, IpcErrorCode::DistroNotFound);
    assert!(set_distro_enabled(&mut s, &known, "Ubuntu", false).is_ok());
    assert_eq!(s.settings().disabled_distros, vec!["Ubuntu".to_string()]);
    let mut bad = SettingsStore::load(FailFs, d.path());
    assert_eq!(
        set_distro_enabled(&mut bad, &known, "Ubuntu", false).unwrap_err().code,
        IpcErrorCode::SettingsSaveFailed
    );
    let e = set_source_root(&mut bad, &sources(), &Probe(true), "claude-local", Some(r"C:\x")).unwrap_err();
    assert_eq!(e.code, IpcErrorCode::SettingsSaveFailed);
}

fn schema_check(def: &str, v: &serde_json::Value) {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/schema/lens.schema.json");
    let mut s: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    s["$ref"] = serde_json::json!(format!("#/definitions/{def}"));
    let r = jsonschema::draft7::new(&s).unwrap().validate(v);
    assert!(r.is_ok(), "{def} invalid: {v}");
}

#[test]
fn ev_and_ipc_error_match_schema() {
    let f = fake(Phase::Ready);
    let evs: Vec<Ev> = vec![
        ev_projects_changed(f.projects(), vec!["gone".into()], vec!["new".into(), "old".into()]),
        ev_project_detail_changed("old"),
        ev_health_changed(f.health()),
    ];
    for e in &evs {
        schema_check("Ev", &serde_json::to_value(e).unwrap());
    }
    let d = tempfile::tempdir().unwrap();
    let mut s = store(d.path());
    let secret = r"C:\Users\secret\body-fragment";
    let mut errs = vec![
        list_projects(&fake(Phase::FirstScan)).unwrap_err(),
        get_project_detail(&f, "").unwrap_err(),
        get_project_detail(&f, secret).unwrap_err(),
        set_distro_enabled(&mut s, &[], secret, true).unwrap_err(),
        set_source_root(&mut s, &sources(), &Probe(true), secret, Some(secret)).unwrap_err(),
        set_source_root(&mut s, &sources(), &Probe(false), "claude-local", Some(secret)).unwrap_err(),
    ];
    errs.push(set_distro_enabled(&mut SettingsStore::load(FailFs, d.path()), &["U".into()], "U", true).unwrap_err());
    for e in &errs {
        schema_check("IpcError", &serde_json::to_value(e).unwrap());
        assert!(!e.message.contains("secret") && !e.message.contains("body-fragment"));
        assert!(!e.message.contains('\\') && !e.message.contains('/') && !e.message.contains(':'));
    }
}

#[test]
fn f30_ipc_payload_truncated_nothing_on_disk() {
    use lens_core::model::v5::{CurrentTask, Health, Hierarchy, Status, UnifiedSession};
    use lens_core::model::{Completeness, Confidence, SessionKind, StatusValue};
    let text = "a".repeat(2001);
    struct One(String);
    impl IpcRead for One {
        fn phase(&self) -> Phase {
            Phase::Ready
        }
        fn projects(&self) -> Vec<ProjectActivity> {
            vec![]
        }
        fn detail(&self, _k: &str) -> Option<Detail> {
            let s = UnifiedSession {
                session_uid: "s:1".into(),
                native_id: "1".into(),
                agent: Agent::ClaudeCode,
                source_id: "claude-local".into(),
                origin: Origin { kind: OriginKind::Windows, distro: None },
                last_activity_at: None,
                last_activity_source: TimeSource::Event,
                projects: vec![],
                status: Status {
                    value: StatusValue::Idle,
                    reason: "r".into(),
                    confidence: Confidence::High,
                    stale: false,
                    auto_review: false,
                },
                current_task: CurrentTask {
                    title: None,
                    last_user_message: Some(self.0.clone()),
                    last_assistant_message: None,
                    active_tool: None,
                },
                hierarchy: Hierarchy { parent_uid: None, child_uids: vec![], kind: SessionKind::TopLevel },
                health: Health { completeness: Completeness::Complete, diagnostics: vec![] },
            };
            Some(Detail { project: row("p", None), sessions: vec![s] })
        }
        fn health(&self) -> Vec<SourceHealth> {
            vec![]
        }
    }
    let d = tempfile::tempdir().unwrap();
    let mut st = store(d.path());
    set_distro_enabled(&mut st, &["Ubuntu".into()], "Ubuntu", false).unwrap();
    let det = get_project_detail(&One(text.clone()), "p").unwrap();
    let got = det.sessions[0].current_task.last_user_message.clone().unwrap();
    assert_eq!(got, text.chars().take(2000).collect::<String>());
    let mut stack = vec![d.path().to_path_buf()];
    while let Some(p) = stack.pop() {
        for e in std::fs::read_dir(&p).unwrap() {
            let e = e.unwrap().path();
            if e.is_dir() {
                stack.push(e);
            } else {
                assert!(!std::fs::read_to_string(&e).unwrap().contains(&text));
            }
        }
    }
}
