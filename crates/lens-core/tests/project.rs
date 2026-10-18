//! Project Resolver tests (architecture-v5 sections 6 and 8).

use lens_core::engine::{Engine, SourceConfig};
use lens_core::model::v5::UNATTRIBUTED_KEY;
use lens_core::model::{Agent, Attribution};
use lens_core::project::{normalize, ProjectResolver};
use std::fs;

#[test]
fn f18_windows_key_case_insensitive() {
    let mut r = ProjectResolver::default();
    for raw in [r"C:\Work\Repo", r"c:\work\repo", r"C:\Work\Repo\"] {
        let n = normalize(raw, None).unwrap();
        assert_eq!(n.key, r"c:\work\repo", "{raw}");
        let p = r.resolve(raw).unwrap();
        assert_eq!(p.key, r"c:\work\repo", "{raw}");
    }
    assert_eq!(normalize(r"C:\Work\Repo", None).unwrap().display, r"C:\Work\Repo");
    assert_eq!(normalize(r"C:\Work\Repo\", None).unwrap().display, r"C:\Work\Repo");
    assert_eq!(r.resolve(r"C:\Work\Repo").unwrap().display_path, r"C:\Work\Repo");
}

#[test]
fn f23a_mnt_drive_maps_to_windows_key() {
    let a = normalize("/mnt/c/x", Some("Ubuntu")).unwrap();
    let b = normalize(r"C:\x", None).unwrap();
    assert_eq!(a.key, r"c:\x");
    assert_eq!(a.key, b.key);
    assert_eq!(a.display, r"C:\x");
    let mut r = ProjectResolver::default();
    let pa = r.resolve_in("/mnt/c/x", Some("Ubuntu")).unwrap();
    let pb = r.resolve(r"C:\x").unwrap();
    assert_eq!(pa.key, pb.key);
}

#[test]
fn f23b_wsl_localhost_converts_to_wsl_key() {
    let want = "wsl:Ubuntu:/home/u/p";
    for (raw, distro) in [
        (r"\\wsl.localhost\Ubuntu\home\u\p", None),
        (r"\\wsl$\Ubuntu\home\u\p", None),
        ("/home/u/p", Some("Ubuntu")),
    ] {
        let n = normalize(raw, distro).unwrap();
        assert_eq!(n.key, want, "{raw}");
        assert_eq!(n.display, "p (WSL: Ubuntu)", "{raw}");
    }
    let mut r = ProjectResolver::default();
    let p = r.resolve(r"\\wsl.localhost\Ubuntu\home\u\p").unwrap();
    assert_eq!(p.key, want);
    assert_eq!(p.display_path, "p (WSL: Ubuntu)");
    assert_eq!(p.attribution, Attribution::RawPath);
}

fn nested(levels: usize) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    fs::create_dir_all(root.join(".git")).unwrap();
    let mut cwd = root.clone();
    for _ in 0..levels {
        cwd = cwd.join("d");
    }
    fs::create_dir_all(&cwd).unwrap();
    (tmp, root, cwd)
}

#[test]
fn git_root_depth_32_found_33_not() {
    let (_t, root, cwd) = nested(32);
    let p = ProjectResolver::default().resolve(&cwd.to_string_lossy()).unwrap();
    assert_eq!(p.attribution, Attribution::GitRoot);
    assert_eq!(p.name, "root");
    assert_eq!(p.key, ProjectResolver::default().resolve(&root.to_string_lossy()).unwrap().key);

    let (_t2, _root2, cwd2) = nested(33);
    let q = ProjectResolver::default().resolve(&cwd2.to_string_lossy()).unwrap();
    assert_eq!(q.attribution, Attribution::RawPath);
    assert_eq!(q.name, "d");
}

#[test]
fn file_uri_decoded() {
    let n = normalize("file:///C:/Work/My%20Repo", None).unwrap();
    assert_eq!(n.key, r"c:\work\my repo");
    assert_eq!(n.display, r"C:\Work\My Repo");
}

#[test]
fn cwd_missing_means_unattributed() {
    assert_eq!(UNATTRIBUTED_KEY, "unattributed");
    assert!(ProjectResolver::default().resolve("").is_none());
    assert!(ProjectResolver::default().resolve("   ").is_none());

    let tmp = tempfile::tempdir().unwrap();
    let claude = tmp.path().join("claude");
    let f = claude.join("p").join("s.jsonl");
    fs::create_dir_all(f.parent().unwrap()).unwrap();
    // The body mentions a path, but there is no cwd field: it must never be used.
    let line = serde_json::json!({"type":"user","sessionId":"s","timestamp":chrono::Utc::now().to_rfc3339(),
        "message":{"role":"user","content":"please look at C:\\Work\\Secret\\Repo"}});
    fs::write(&f, format!("{line}\n")).unwrap();
    let mut eng = Engine::new(vec![SourceConfig { id: "claude-local".into(), agent: Agent::ClaudeCode, root: claude }]);
    eng.rescan();
    eng.process_all();
    let s = eng.snapshot(chrono::Utc::now());
    let x = s.sessions.iter().find(|x| x.session_uid == "claude-local:s").expect("session");
    assert!(x.projects.is_empty());
}
