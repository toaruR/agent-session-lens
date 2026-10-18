//! Closing traceability audit: every section 11 F-id maps to an existing test fn,
//! and every section 12 mechanism maps to a test fn or an accepted-limit note.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn doc_section(start: &str, end: Option<&str>) -> String {
    let doc = fs::read_to_string(root().join("docs/architecture-v5.md")).expect("read architecture-v5.md");
    let from = doc.find(start).unwrap_or_else(|| panic!("section {start} missing"));
    let rest = &doc[from..];
    match end.and_then(|e| rest.find(e)) {
        Some(to) => rest[..to].to_string(),
        None => rest.to_string(),
    }
}

fn section11_ids() -> BTreeSet<String> {
    let sec = doc_section("## 11. ", Some("## 12. "));
    let mut ids = BTreeSet::new();
    for line in sec.lines() {
        let Some(rest) = line.strip_prefix("| F") else { continue };
        let cell = rest.split('|').next().unwrap_or("").trim();
        if !cell.is_empty() && cell.chars().all(|c| c.is_ascii_alphanumeric()) {
            ids.insert(format!("F{cell}"));
        }
    }
    ids
}

fn mapping() -> serde_json::Map<String, serde_json::Value> {
    let text = fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/traceability.json"))
        .expect("read traceability.json");
    match serde_json::from_str::<serde_json::Value>(&text).expect("parse traceability.json") {
        serde_json::Value::Object(m) => m,
        _ => panic!("traceability.json must be an object"),
    }
}

/// True when `file` contains `fn <name>` as a whole identifier.
fn has_fn(file: &str, name: &str) -> bool {
    let Ok(src) = fs::read_to_string(root().join(file)) else { return false };
    let needle = format!("fn {name}");
    src.match_indices(&needle).any(|(i, _)| {
        let after = src[i + needle.len()..].chars().next();
        !matches!(after, Some(c) if c.is_ascii_alphanumeric() || c == '_')
    })
}

#[test]
fn every_f_id_maps_to_existing_test_fn() {
    let ids = section11_ids();
    assert_eq!(ids.len(), 54, "section 11 must define 54 F-ids, got {}", ids.len());
    let map = mapping();
    for id in &ids {
        let entry = map.get(id).unwrap_or_else(|| panic!("{id}: no mapping in traceability.json"));
        let file = entry["file"].as_str().unwrap_or_else(|| panic!("{id}: missing file"));
        let name = entry["fn"].as_str().unwrap_or_else(|| panic!("{id}: missing fn"));
        assert!(
            file.starts_with("crates/") && file.contains("/tests/") && file.ends_with(".rs"),
            "{id}: file must be crates/*/tests/*.rs, got {file}"
        );
        assert!(has_fn(file, name), "{id}: `fn {name}` not found in {file}");
    }
}

#[test]
fn traceability_json_has_54_entries() {
    let map = mapping();
    assert_eq!(map.len(), 54, "traceability.json must have exactly 54 keys");
    let ids = section11_ids();
    for k in map.keys() {
        assert!(ids.contains(k), "{k}: key is not a section 11 F-id");
    }
}

/// Section 12 mechanism (substring of the first table cell) -> evidence.
/// `Fn` = test fn; `Limit` = accepted limit documented in section 12 (with the F-id or reason).
enum Evidence {
    Fn(&'static str, &'static str),
    Limit(&'static str),
}

const L: &str = "crates/lens-core/tests/";

#[test]
fn section12_mechanisms_have_test_or_accepted_limit() {
    use Evidence::*;
    let table: Vec<(&str, Evidence)> = vec![
        ("厳格 UTF-8", Fn("reader.rs", "f4_invalid_utf8_line_dropped")),
        ("redaction R1", Fn("redact.rs", "f16a_sk_key")),
        ("fork 除外", Fn("codex_parse.rs", "f1b_fork_boundary_unknown")),
        ("guardian_review 除外", Fn("aggregator.rs", "f19_guardian_review_not_activity")),
        ("STALE_SECS", Fn("status_table.rs", "f14_unresolved_tool_use_700_is_stale_running")),
        ("子を親終了で idle", Fn("aggregator.rs", "f2_child_idle_after_parent_end_turn")),
        ("停止中は起動せず", Fn("supervisor_lifecycle.rs", "f12_stopped_distro_state")),
        ("clockOffset", Fn("clock_offset.rs", "f10a_threshold_cases")),
        ("heartbeat 無応答", Fn("supervisor_lifecycle.rs", "f11a_heartbeat_silence_29_vs_30")),
        ("Job Object", Fn("child_stdin.rs", "job_object_contains_child_windows")),
        ("DEBOUNCE_MS", Fn("change_detector.rs", "debounce")),
        ("一時ファイル + rename", Fn("settings.rs", "f27_persist_exact_json")),
        ("不正行の破棄", Fn("ndjson.rs", "f13e_malformed_json")),
        ("二重 redaction", Fn("supervisor_ingest.rs", "f31_leaked_token_redacted_before_ipc")),
        ("Stdio::null()", Fn("child_stdin.rs", "f9_child_stdin_null_exits_zero_read_zero")),
        ("HOT_POLL_SECS", Fn("change_detector.rs", "hot_poll")),
        ("INITIAL_SCAN_DAYS", Fn("initial_scan.rs", "f21_seven_day_boundary")),
        ("TRUNC_TEXT_CHARS", Fn("ipc.rs", "f30_ipc_payload_truncated_nothing_on_disk")),
        ("Windows パスキー", Fn("project.rs", "f18_windows_key_case_insensitive")),
    ];
    // Accepted limits (section 12 column "限界") are documented, not tested, beyond the above.
    let _ = Limit("documented in section 12");

    let sec = doc_section("## 12. ", None);
    let mut rows = 0;
    for line in sec.lines() {
        if !line.starts_with("| ") || line.starts_with("| 機構") || line.starts_with("|---") {
            continue;
        }
        rows += 1;
        let cell = line.trim_start_matches('|').split('|').next().unwrap_or("").trim();
        let (_, ev) = table
            .iter()
            .find(|(k, _)| cell.contains(k))
            .unwrap_or_else(|| panic!("section 12 mechanism without test or limit note: {cell}"));
        match ev {
            Fn(file, name) => assert!(has_fn(&format!("{L}{file}"), name), "{cell}: `fn {name}` not found in {file}"),
            Limit(note) => assert!(!note.is_empty(), "{cell}: empty limit note"),
        }
    }
    assert_eq!(rows, table.len(), "section 12 has {rows} mechanisms, table has {}", table.len());
}
