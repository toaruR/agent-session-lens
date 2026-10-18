use lens_core::model::Agent;
use lens_core::tokens::{
    aggregate_tree, calculate, find_session_log, TokenCache, TokenTreeNode, TokenUsage,
};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

fn temp_dir() -> tempfile::TempDir {
    // Stay inside the writable checkout; sandbox temp directories may reject atomic renames.
    tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")).unwrap()
}

fn write(path: &Path, rows: &[Value]) {
    fs::write(
        path,
        rows.iter().map(|v| format!("{v}\n")).collect::<String>(),
    )
    .unwrap();
}

fn codex(input: u64, cached: u64, output: u64) -> Value {
    json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{
        "input_tokens":input,"cached_input_tokens":cached,"output_tokens":output,"reasoning_output_tokens":output
    }}}})
}

#[test]
fn codex_uses_latest_cumulative_usage_without_double_counting_reasoning() {
    let dir = temp_dir();
    let path = dir.path().join("rollout-test.jsonl");
    write(
        &path,
        &[
            codex(100, 70, 20),
            codex(200, 120, 50),
            json!({"type":"event_msg","payload":{"type":"token_count","info":null}}),
        ],
    );
    let usage = calculate(&path, Agent::Codex, "test").unwrap().unwrap();
    assert_eq!(
        (
            usage.cached_input_tokens,
            usage.uncached_input_tokens,
            usage.output_tokens,
            usage.total_tokens
        ),
        (120, 80, 50, 250)
    );
    write(&path, &[codex(10, 99, 5)]);
    let usage = calculate(&path, Agent::Codex, "test").unwrap().unwrap();
    assert_eq!(
        (
            usage.cached_input_tokens,
            usage.uncached_input_tokens,
            usage.total_tokens
        ),
        (10, 0, 15)
    );
}

#[test]
fn codex_fork_history_is_not_reported_as_child_usage() {
    let dir = temp_dir();
    let path = dir.path().join("child.jsonl");
    let meta =
        json!({"type":"session_meta","payload":{"id":"child","subagent_history_start_ordinal":3}});
    write(
        &path,
        &[
            meta.clone(),
            codex(9999, 500, 100),
            json!({"type":"session_meta","payload":{"id":"parent"}}),
        ],
    );
    assert!(calculate(&path, Agent::Codex, "child").unwrap().is_none());
    write(
        &path,
        &[
            meta,
            codex(9999, 500, 100),
            json!({"type":"session_meta","payload":{"id":"parent"}}),
            codex(100, 30, 10),
        ],
    );
    assert_eq!(
        calculate(&path, Agent::Codex, "child")
            .unwrap()
            .unwrap()
            .total_tokens,
        110
    );
}

fn claude(id: &str, input: u64, side: bool) -> Value {
    json!({"type":"assistant","sessionId":"session","isSidechain":side,"agentId":"child",
        "message":{"id":id,"usage":{"input_tokens":input,"cache_read_input_tokens":20,"cache_creation_input_tokens":5,"output_tokens":10}}})
}

#[test]
fn claude_deduplicates_streaming_and_keeps_sessions_separate() {
    let dir = temp_dir();
    let path = dir.path().join("session.jsonl");
    write(
        &path,
        &[
            claude("a", 1, false),
            claude("a", 10, false),
            claude("b", 30, false),
            claude("child-msg", 100, true),
        ],
    );
    let parent = calculate(&path, Agent::ClaudeCode, "session")
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            parent.uncached_input_tokens,
            parent.cache_creation_input_tokens,
            parent.total_tokens
        ),
        (40, 10, 110)
    );
    assert_eq!(
        calculate(&path, Agent::ClaudeCode, "session::agent:child")
            .unwrap()
            .unwrap()
            .total_tokens,
        135
    );
    let subdir = dir.path().join("session/subagents");
    assert_eq!(
        find_session_log(dir.path(), Agent::ClaudeCode, "session::agent:child").unwrap(),
        path
    );
    fs::create_dir_all(&subdir).unwrap();
    let child_path = subdir.join("agent-child.jsonl");
    write(&child_path, &[claude("c", 7, false)]);
    assert_eq!(
        find_session_log(dir.path(), Agent::ClaudeCode, "session::agent:child").unwrap(),
        child_path
    );
    assert_eq!(
        calculate(&child_path, Agent::ClaudeCode, "session::agent:child")
            .unwrap()
            .unwrap()
            .total_tokens,
        42
    );
}

#[test]
fn cache_persists_reuses_and_invalidates_after_log_changes() {
    let dir = temp_dir();
    let path = dir.path().join("session.jsonl");
    let cache_path = dir.path().join("cache/session-tokens.json");
    let mut cache = TokenCache::load(cache_path.clone());
    write(&path, &[codex(100, 50, 20)]);
    assert_eq!(
        cache
            .get("uid", "test", Agent::Codex, &path)
            .unwrap()
            .unwrap()
            .total_tokens,
        120
    );
    let bytes = fs::read(&cache_path).unwrap();
    let mut cache = TokenCache::load(cache_path.clone());
    assert_eq!(
        cache
            .get("uid", "test", Agent::Codex, &path)
            .unwrap()
            .unwrap()
            .total_tokens,
        120
    );
    assert_eq!(fs::read(&cache_path).unwrap(), bytes);
    write(&path, &[codex(1000, 100, 20)]);
    assert_eq!(
        cache
            .get("uid", "test", Agent::Codex, &path)
            .unwrap()
            .unwrap()
            .total_tokens,
        1020
    );
    assert_ne!(fs::read(&cache_path).unwrap(), bytes);
    assert_eq!(
        TokenCache::load(cache_path)
            .get("uid", "test", Agent::Codex, &path)
            .unwrap()
            .unwrap()
            .total_tokens,
        1020
    );
}

#[test]
fn unknown_is_not_zero_or_cached_and_broken_cache_is_recoverable() {
    let dir = temp_dir();
    let path = dir.path().join("session.jsonl");
    let cache_path = dir.path().join("cache.json");
    fs::write(&cache_path, "broken").unwrap();
    let mut cache = TokenCache::load(cache_path.clone());
    fs::write(&path, "{bad json}\n{\"type\":").unwrap();
    assert!(cache
        .get("uid", "test", Agent::Codex, &path)
        .unwrap()
        .is_none());
    assert_eq!(fs::read_to_string(&cache_path).unwrap(), "broken");
    write(&path, &[codex(0, 0, 0)]);
    assert_eq!(
        cache
            .get("uid", "test", Agent::Codex, &path)
            .unwrap()
            .unwrap()
            .total_tokens,
        0
    );
    assert!(serde_json::from_slice::<Value>(&fs::read(cache_path).unwrap()).is_ok());
}

fn proto(fields: &[(u8, u8)]) -> Vec<u8> {
    fields
        .iter()
        .flat_map(|(field, value)| [field << 3, *value])
        .collect()
}
fn nested(field: u8, data: &[u8]) -> Vec<u8> {
    [vec![(field << 3) | 2, data.len() as u8], data.to_vec()].concat()
}

#[test]
fn agy_counts_steps_or_falls_back_to_generator_metadata_without_double_counting() {
    let dir = temp_dir();
    let path = dir.path().join("session.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE steps(idx INTEGER, metadata BLOB); CREATE TABLE gen_metadata(idx INTEGER, data BLOB);").unwrap();
    let usage = proto(&[(2, 30), (3, 10), (4, 5), (5, 20)]);
    db.execute("INSERT INTO steps VALUES(1, ?1)", [nested(9, &usage)])
        .unwrap();
    db.execute(
        "INSERT INTO gen_metadata VALUES(1, ?1)",
        [nested(1, &nested(4, &usage))],
    )
    .unwrap();
    let step = calculate(&path, Agent::Antigravity, "session")
        .unwrap()
        .unwrap();
    assert_eq!(step.total_tokens, 65);
    db.execute("DELETE FROM steps", []).unwrap();
    assert_eq!(
        calculate(&path, Agent::Antigravity, "session")
            .unwrap()
            .unwrap(),
        step
    );
    db.execute("DELETE FROM gen_metadata", []).unwrap();
    assert!(calculate(&path, Agent::Antigravity, "session")
        .unwrap()
        .is_none());
}

#[test]
fn agy_wal_changes_invalidate_cached_totals_without_checkpointing() {
    let dir = temp_dir();
    let path = dir.path().join("session.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE steps(idx INTEGER, metadata BLOB);").unwrap();
    db.execute(
        "INSERT INTO steps VALUES(1, ?1)",
        [nested(9, &proto(&[(2, 10), (3, 2)]))],
    )
    .unwrap();
    let mut cache = TokenCache::load(dir.path().join("tokens.json"));
    assert_eq!(
        cache
            .get("agy", "session", Agent::Antigravity, &path)
            .unwrap()
            .unwrap()
            .total_tokens,
        12
    );
    db.execute(
        "INSERT INTO steps VALUES(2, ?1)",
        [nested(9, &proto(&[(2, 20), (3, 3)]))],
    )
    .unwrap();
    assert_eq!(
        cache
            .get("agy", "session", Agent::Antigravity, &path)
            .unwrap()
            .unwrap()
            .total_tokens,
        35
    );
}

fn usage(tokens: u64) -> TokenUsage {
    TokenUsage {
        uncached_input_tokens: tokens,
        total_tokens: tokens,
        ..Default::default()
    }
}

#[test]
fn descendants_include_grandchildren_once_and_mark_unknown_and_failed_totals() {
    let mut visited = Vec::new();
    let report = aggregate_tree("root", |id| {
        visited.push(id.to_owned());
        let (tokens, children, failed) = match id {
            "root" => (
                Some(100),
                vec!["child", "sibling", "child", "unknown"],
                false,
            ),
            "child" => (Some(20), vec!["grandchild"], false),
            "grandchild" => (Some(5), vec!["root", "child"], false),
            "sibling" => (Some(30), vec!["grandchild", "failed"], false),
            "unknown" => (None, vec![], false),
            "failed" => (None, vec!["grandchild"], true),
            _ => panic!("unrelated session must not be visited"),
        };
        Ok::<_, ()>(TokenTreeNode {
            usage: tokens.map(usage),
            children: children.into_iter().map(str::to_owned).collect(),
            failed,
        })
    })
    .unwrap();
    assert_eq!(report.session.unwrap().total_tokens, 100);
    assert_eq!(report.including_subagents.unwrap().total_tokens, 155);
    assert_eq!(
        (
            report.subagent_count,
            report.unknown_count,
            report.failed_count
        ),
        (5, 1, 1)
    );
    assert_eq!(visited.len(), 6);
}

#[test]
fn missing_parent_usage_is_not_zero_and_does_not_hide_known_descendants() {
    let report = aggregate_tree("root", |id| {
        Ok::<_, ()>(TokenTreeNode {
            usage: (id != "root").then(|| usage(25)),
            children: if id == "root" {
                vec!["child".into()]
            } else {
                vec![]
            },
            failed: false,
        })
    })
    .unwrap();
    assert!(report.session.is_none());
    assert_eq!(
        report.including_subagents.as_ref().unwrap().total_tokens,
        25
    );
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["subagentCount"], 1);
    assert_eq!(json["unknownCount"], 1);
    assert_eq!(json["includingSubagents"]["totalTokens"], 25);
}

#[test]
fn codex_child_references_are_cached_and_inherited_spawns_are_ignored() {
    let dir = temp_dir();
    let path = dir.path().join("root.jsonl");
    let spawn = |id| {
        json!({"type":"event_msg","payload":{"type":"item_completed","item":{
            "type":"SubAgentActivity","kind":"started","agent_thread_id":id
        }}})
    };
    write(
        &path,
        &[
            json!({"type":"session_meta","payload":{"id":"root","subagent_history_start_ordinal":3}}),
            spawn("inherited"),
            json!({"type":"session_meta","payload":{"id":"old"}}),
            spawn("child"),
            spawn("child"),
            codex(100, 50, 20),
            json!({"type":"response_item","payload":{"type":"function_call","name":"spawn_agent","call_id":"spawn"}}),
            json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"spawn","output":"{\"agent_id\":\"second\"}"}}),
        ],
    );
    let cache_path = dir.path().join("cache.json");
    let record = TokenCache::load(cache_path.clone())
        .inspect("root", "root", Agent::Codex, &path)
        .unwrap();
    assert_eq!(
        record
            .children
            .iter()
            .map(|c| c.native_id.as_str())
            .collect::<Vec<_>>(),
        vec!["child", "second"]
    );
    assert_eq!(
        TokenCache::load(cache_path)
            .inspect("root", "root", Agent::Codex, &path)
            .unwrap()
            .children,
        record.children
    );
}

#[test]
fn nested_claude_files_and_changes_in_descendants_update_the_total() {
    let dir = temp_dir();
    let root = dir.path().join("session.jsonl");
    let child = dir.path().join("session/subagents/agent-child.jsonl");
    let grand = dir
        .path()
        .join("session/subagents/agent-child/subagents/agent-grand.jsonl");
    fs::create_dir_all(grand.parent().unwrap()).unwrap();
    write(&root, &[claude("a", 10, false)]);
    write(&child, &[claude("b", 7, false)]);
    write(&grand, &[claude("c", 3, false)]);
    let cache_path = dir.path().join("cache.json");
    let sum = |cache: &mut TokenCache| {
        let mut paths = std::collections::BTreeMap::from([("session".to_owned(), root.clone())]);
        aggregate_tree("session", |id| {
            let path = paths[id].clone();
            let record = cache.inspect(id, id, Agent::ClaudeCode, &path)?;
            let mut children = Vec::new();
            for child in record.children {
                children.push(child.native_id.clone());
                paths.insert(child.native_id, child.path.unwrap());
            }
            Ok::<_, std::io::Error>(TokenTreeNode {
                usage: record.usage,
                children,
                failed: false,
            })
        })
        .unwrap()
    };
    let initial = sum(&mut TokenCache::load(cache_path.clone()));
    assert_eq!(initial.session.unwrap().total_tokens, 45);
    assert_eq!(initial.including_subagents.unwrap().total_tokens, 125);
    assert_eq!(initial.subagent_count, 2);
    // Parent log unchanged: a grandchild update must invalidate only its own cached count.
    write(&grand, &[claude("c", 300, false)]);
    let updated = sum(&mut TokenCache::load(cache_path));
    assert_eq!(updated.including_subagents.unwrap().total_tokens, 422);
}

#[test]
fn agy_explicit_transcript_children_invalidate_the_cached_discovery() {
    let dir = temp_dir();
    let parent = "11111111-1111-1111-1111-111111111111";
    let child = "22222222-2222-2222-2222-222222222222";
    fs::create_dir_all(dir.path().join("conversations")).unwrap();
    let db_path = dir
        .path()
        .join("conversations")
        .join(format!("{parent}.db"));
    let db = rusqlite::Connection::open(&db_path).unwrap();
    db.execute_batch("CREATE TABLE steps(idx INTEGER, metadata BLOB);")
        .unwrap();
    db.execute(
        "INSERT INTO steps VALUES(1, ?1)",
        [nested(9, &proto(&[(2, 10)]))],
    )
    .unwrap();
    let mut cache = TokenCache::load(dir.path().join("cache.json"));
    assert!(cache
        .inspect(parent, parent, Agent::Antigravity, &db_path)
        .unwrap()
        .children
        .is_empty());
    let transcript = dir
        .path()
        .join("brain")
        .join(parent)
        .join(".system_generated/logs/transcript.jsonl");
    fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    write(
        &transcript,
        &[
            json!({"conversationId":parent}),
            json!({"tool":"invoke_subagent","result":{"conversationId":child}}),
            json!({"text":"{\"conversation_id\":\"22222222-2222-2222-2222-222222222222\"}"}),
            json!({"other":"33333333-3333-3333-3333-333333333333"}),
        ],
    );
    let record = cache
        .inspect(parent, parent, Agent::Antigravity, &db_path)
        .unwrap();
    assert_eq!(record.children.len(), 1);
    assert_eq!(record.children[0].native_id, child);
}
