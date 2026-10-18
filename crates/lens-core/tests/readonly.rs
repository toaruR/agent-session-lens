//! F22 read-only guarantee (architecture-v5 sections 1 and 11).
//! Equivalent manual check: `Get-FileHash -Algorithm SHA256 <file>` (PowerShell) / `sha256sum <file>`.

use chrono::{DateTime, Utc};
use filetime::{set_file_mtime, FileTime};
use lens_core::engine::{Engine, SourceConfig};
use lens_core::model::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn ts() -> String {
    Utc::now().to_rfc3339()
}

fn put(path: &Path, lines: &[serde_json::Value], mtime_secs: i64, readonly: bool) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut s = String::new();
    for l in lines {
        s.push_str(&l.to_string());
        s.push('\n');
    }
    fs::write(path, s).unwrap();
    set_file_mtime(path, FileTime::from_unix_time(mtime_secs, 0)).unwrap();
    if readonly {
        let mut p = fs::metadata(path).unwrap().permissions();
        p.set_readonly(true);
        fs::set_permissions(path, p).unwrap();
    }
}

fn claude_user(sid: &str, text: &str) -> serde_json::Value {
    json!({"type":"user","sessionId":sid,"timestamp":ts(),"message":{"role":"user","content":text}})
}

fn codex_meta(id: &str, cwd: &Path) -> serde_json::Value {
    json!({"type":"session_meta","timestamp":ts(),"payload":{"id":id,"cwd":cwd,"cli_version":"0.1.0","thread_source":"user","timestamp":ts()}})
}

fn fingerprint(path: &Path) -> (String, String) {
    let bytes = fs::read(path).unwrap();
    let hash: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    let mtime = format!("{:?}", fs::metadata(path).unwrap().modified().unwrap());
    (hash, mtime)
}

#[test]
fn f22_hash_and_mtime_unchanged_after_scan() {
    let tmp = tempfile::tempdir().unwrap();
    let claude = tmp.path().join("claude").join("projects");
    let codex = tmp.path().join("codex").join("sessions");
    let denied_root = tmp.path().join("denied").join("projects");
    let cwd = tmp.path().join("work");
    fs::create_dir_all(&cwd).unwrap();
    let recent = Utc::now().timestamp() - 60;

    let rollout = |id: &str| codex.join("2026").join("10").join("17").join(format!("rollout-2026-10-17T00-00-00-{id}.jsonl"));
    let fixtures: Vec<PathBuf> = vec![
        claude.join("p").join("writable.jsonl"),
        claude.join("p").join("readonly.jsonl"),
        rollout("w1"),
        rollout("r1"),
        denied_root.join("p").join("unreadable.jsonl"),
    ];
    put(&fixtures[0], &[claude_user("w", "hello")], recent, false);
    put(&fixtures[1], &[claude_user("r", "hello")], recent, true);
    put(&fixtures[2], &[codex_meta("w1", &cwd)], recent, false);
    put(&fixtures[3], &[codex_meta("r1", &cwd)], recent, true);
    // Unreadable file: making one unreadable is OS-specific (icacls deny / chmod 000), so the
    // injected directory opener (the initial-scan fallback) denies access to its source root.
    put(&fixtures[4], &[claude_user("u", "hello")], recent, false);
    eprintln!("f22 unreadable variant: injected_opener");

    // Live file appended to through the scan loop; kept outside the hashed set because the
    // test itself appends to it (the engine must never write).
    let live = claude.join("p").join("live.jsonl");
    put(&live, &[claude_user("live", "start")], recent, false);

    let before: Vec<_> = fixtures.iter().map(|p| fingerprint(p)).collect();

    let denied: lens_core::engine::DirOpener =
        Arc::new(|_p: &Path| Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)));
    let mut eng = Engine::new(vec![
        SourceConfig { id: "claude-local".into(), agent: Agent::ClaudeCode, root: claude.clone() },
        SourceConfig { id: "codex-local".into(), agent: Agent::Codex, root: codex.clone() },
    ]);
    eng.rescan();
    eng.process_all();
    for i in 0..3 {
        let mut f = OpenOptions::new().append(true).open(&live).unwrap();
        writeln!(f, "{}", claude_user("live", &format!("more {i}"))).unwrap();
        drop(f);
        eng.mark_dirty(&live);
        eng.process_all();
        eng.rescan();
    }
    let snap = eng.snapshot(DateTime::<Utc>::from(std::time::SystemTime::now()));
    assert!(snap.sessions.len() >= 4);

    let mut deng = Engine::new(vec![SourceConfig { id: "denied".into(), agent: Agent::ClaudeCode, root: denied_root.clone() }])
        .with_dir_opener(denied);
    deng.rescan();
    deng.process_all();
    let dsnap = deng.snapshot(DateTime::<Utc>::from(std::time::SystemTime::now()));
    assert_eq!(dsnap.sources[0].state, SourceState::AccessDenied);

    let after: Vec<_> = fixtures.iter().map(|p| fingerprint(p)).collect();
    assert_eq!(before, after);
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_rs(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn no_write_capable_open_in_sources() {
    let ws = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let mut roots = vec![ws.join("src-tauri").join("src")];
    for e in fs::read_dir(ws.join("crates")).unwrap().flatten() {
        roots.push(e.path().join("src"));
    }
    let needles = [".write(true)", ".append(true)", ".create(true)", ".create_new(true)", "File::create"];
    let mut files = Vec::new();
    for r in &roots {
        collect_rs(r, &mut files);
    }
    assert!(!files.is_empty());
    let mut hits = Vec::new();
    for f in files {
        if f.file_name().is_some_and(|n| n == "settings.rs") {
            continue;
        }
        let text = fs::read_to_string(&f).unwrap();
        for (i, line) in text.lines().enumerate() {
            if needles.iter().any(|n| line.contains(n)) {
                hits.push(format!("{}:{}", f.display(), i + 1));
            }
        }
    }
    assert!(hits.is_empty(), "write-capable opens: {hits:?}");
}
