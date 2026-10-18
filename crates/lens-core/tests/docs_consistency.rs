//! Mechanical check that docs/implementation-v5.md matches the code and schema.

use serde_json::Value;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn doc() -> String {
    read("docs/implementation-v5.md")
}

fn missing(doc: &str, items: &[String]) -> Vec<String> {
    items.iter().filter(|i| !doc.contains(&format!("`{i}`"))).cloned().collect()
}

fn schema() -> Value {
    serde_json::from_str(&read("crates/lens-core/schema/lens.schema.json")).unwrap()
}

#[test]
fn doc_lists_every_ipc_error_code() {
    let s = schema();
    let codes: Vec<String> = s["definitions"]["IpcError"]["properties"]["code"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(codes.len(), 7);
    let m = missing(&doc(), &codes);
    assert!(m.is_empty(), "IpcError codes missing from doc: {m:?}");
}

fn handler_commands() -> Vec<String> {
    let src = read("src-tauri/src/main.rs");
    let start = src.find("generate_handler![").expect("generate_handler!") + "generate_handler![".len();
    let end = start + src[start..].find(']').unwrap();
    src[start..end].split(',').map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect()
}

#[test]
fn doc_lists_every_tauri_command_and_no_extra() {
    let cmds = handler_commands();
    assert_eq!(cmds.len(), 6, "doc states exactly 6 commands: {cmds:?}");
    let d = doc();
    let m = missing(&d, &cmds);
    assert!(m.is_empty(), "commands missing from doc: {m:?}");
    // Every `#[tauri::command]` fn must be registered.
    let defined: usize = read("src-tauri/src/commands.rs").matches("#[tauri::command]").count();
    assert_eq!(defined, cmds.len());
    // Table rows in section 1 name only registered commands.
    let sec = d.split("## 2.").next().unwrap();
    for line in sec.lines().filter(|l| l.starts_with("| `")) {
        let name = line.trim_start_matches("| `").split('`').next().unwrap();
        assert!(cmds.iter().any(|c| c == name), "doc row for unknown command {name}");
    }
}

#[test]
fn doc_lists_every_event_name() {
    let d = doc();
    let ingest = read("src-tauri/src/ingest.rs");
    for ev in ["projects_changed", "project_detail_changed", "health_changed"] {
        assert!(d.contains(&format!("`{ev}`")), "event {ev} missing from doc");
        assert!(ingest.contains(&format!("emit(\"{ev}\"")), "event {ev} not emitted by code");
    }
}

#[test]
fn doc_lists_helper_exit_codes_and_stderr_tokens() {
    let cli = read("crates/lens-wsl-helper/src/cli.rs");
    let d = doc();
    let mut codes = Vec::new();
    for line in cli.lines().filter(|l| l.starts_with("pub const EXIT_")) {
        let n: u32 = line.rsplit('=').next().unwrap().trim().trim_end_matches(';').trim().parse().unwrap();
        codes.push(n);
    }
    codes.push(101);
    assert_eq!(codes.len(), 6);
    for n in codes {
        assert!(d.contains(&format!("| {n} |")), "exit code {n} missing from doc table");
    }
    for tok in ["usage_error", "protocol_unsupported", "root_unreadable", "stdout_failed", "internal"] {
        assert!(d.contains(&format!("`{tok}`")), "stderr token {tok} missing from doc");
    }
    // Every token the CLI can emit is documented.
    for part in cli.split("fail(err, EXIT_").skip(1) {
        let tok = part.split('"').nth(1).unwrap();
        assert!(d.contains(&format!("`{tok}`")), "cli stderr token {tok} undocumented");
    }
}

#[test]
fn doc_lists_every_settings_field() {
    let s = schema();
    let props = s["definitions"]["Settings"]["properties"].as_object().unwrap();
    let d = doc();
    assert_eq!(props.len(), 4);
    for k in props.keys() {
        assert!(d.contains(&format!("\"{k}\"")) || d.contains(&format!("`{k}`")), "settings field {k} missing from doc");
    }
    assert!(d.contains("settings.json"));
    assert!(d.contains("settings.json.bad"));
    assert!(d.contains("settings_load_failed"));
}

#[test]
fn doc_source_paths_exist() {
    let d = doc();
    for rel in [
        "src-tauri/src/main.rs",
        "src-tauri/src/commands.rs",
        "src-tauri/src/state.rs",
        "src-tauri/src/ingest.rs",
        "src-tauri/src/wsl.rs",
        "crates/lens-core/src/engine/mod.rs",
    ] {
        assert!(root().join(rel).is_file(), "{rel} missing");
    }
    for name in ["commands", "state", "ingest", "wsl"] {
        assert!(d.contains(name), "doc does not mention {name}.rs");
    }
    assert!(!d.contains("`engine.rs`"), "stale engine.rs reference");
}
