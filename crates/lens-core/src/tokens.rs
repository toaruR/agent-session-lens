//! On-demand accounting. Source logs are read-only; only derived numeric totals are cached.
use crate::agy::{get_len_delim_field, get_varint_field};
use crate::claude::{path_agent_id, session_identity};
use crate::model::Agent;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub cached_input_tokens: u64,
    pub uncached_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
    /// Includes reasoning/thinking, never added a second time.
    pub output_tokens: u64,
    pub total_tokens: u64,
}

impl TokenUsage {
    fn finish(mut self) -> Self {
        self.total_tokens = self
            .cached_input_tokens
            .saturating_add(self.uncached_input_tokens)
            .saturating_add(self.cache_creation_input_tokens)
            .saturating_add(self.output_tokens);
        self
    }
    fn add(&mut self, other: &Self) {
        self.cached_input_tokens = self
            .cached_input_tokens
            .saturating_add(other.cached_input_tokens);
        self.uncached_input_tokens = self
            .uncached_input_tokens
            .saturating_add(other.uncached_input_tokens);
        self.cache_creation_input_tokens = self
            .cache_creation_input_tokens
            .saturating_add(other.cache_creation_input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChildLog {
    pub native_id: String,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenRecord {
    pub usage: Option<TokenUsage>,
    pub children: Vec<ChildLog>,
}

#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenReport {
    pub session: Option<TokenUsage>,
    pub including_subagents: Option<TokenUsage>,
    pub subagent_count: usize,
    pub unknown_count: usize,
    pub failed_count: usize,
}

impl TokenReport {
    /// The caller visits each UID once, even for cyclic or converging hierarchies.
    pub fn include(&mut self, usage: Option<&TokenUsage>, root: bool, failed: bool) {
        if !root {
            self.subagent_count += 1;
        }
        if root {
            self.session = usage.cloned();
        }
        if let Some(usage) = usage {
            let mut total = self.including_subagents.take().unwrap_or_default();
            total.add(usage);
            self.including_subagents = Some(total.finish());
        } else if failed {
            self.failed_count += 1;
        } else {
            self.unknown_count += 1;
        }
    }
}

pub struct TokenTreeNode {
    pub usage: Option<TokenUsage>,
    pub children: Vec<String>,
    pub failed: bool,
}

pub fn aggregate_tree<E>(
    root: &str,
    mut load: impl FnMut(&str) -> Result<TokenTreeNode, E>,
) -> Result<TokenReport, E> {
    let mut pending = vec![root.to_owned()];
    let mut seen = BTreeSet::new();
    let mut report = TokenReport::default();
    while let Some(uid) = pending.pop() {
        if !seen.insert(uid.clone()) {
            continue;
        }
        let node = load(&uid)?;
        report.include(node.usage.as_ref(), uid == root, node.failed);
        pending.extend(node.children);
    }
    Ok(report)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Stamp {
    path: PathBuf,
    len: u64,
    modified_ns: u128,
    wal: Option<(u64, u128)>,
    children_dir: Option<(u64, u128)>,
    transcript: Option<(u64, u128)>,
}

fn stat(path: &Path) -> io::Result<(u64, u128)> {
    let m = fs::metadata(path)?;
    Ok((
        m.len(),
        m.modified()?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    ))
}

fn stamp(path: &Path, agent: Agent) -> io::Result<Stamp> {
    let (len, modified_ns) = stat(path)?;
    let wal = if agent == Agent::Antigravity {
        match stat(&PathBuf::from(format!("{}-wal", path.display()))) {
            Ok(s) => Some(s),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        }
    } else {
        None
    };
    Ok(Stamp {
        path: path.to_owned(),
        len,
        modified_ns,
        wal,
        children_dir: optional_stat(&path.with_extension("").join("subagents"))?,
        transcript: if agent == Agent::Antigravity {
            optional_stat(&transcript_path(path))?
        } else {
            None
        },
    })
}

fn optional_stat(path: &Path) -> io::Result<Option<(u64, u128)>> {
    match stat(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn transcript_path(path: &Path) -> PathBuf {
    path.parent()
        .unwrap_or(Path::new(""))
        .parent()
        .unwrap_or(Path::new(""))
        .join("brain")
        .join(path.file_stem().unwrap_or_default())
        .join(".system_generated/logs/transcript.jsonl")
}

#[derive(Serialize, Deserialize)]
struct Cached {
    stamp: Stamp,
    record: TokenRecord,
}

#[derive(Default, Serialize, Deserialize)]
struct CacheFile {
    version: u32,
    sessions: BTreeMap<String, Cached>,
}

pub struct TokenCache {
    path: PathBuf,
    data: CacheFile,
}

impl TokenCache {
    pub fn load(path: PathBuf) -> Self {
        let data = fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<CacheFile>(&b).ok())
            .filter(|d| d.version == 2)
            .unwrap_or(CacheFile {
                version: 2,
                ..Default::default()
            });
        Self { path, data }
    }

    /// Never cache unknown totals or a moving snapshot. Validate even after an app restart.
    pub fn get(
        &mut self,
        uid: &str,
        native_id: &str,
        agent: Agent,
        path: &Path,
    ) -> io::Result<Option<TokenUsage>> {
        self.inspect(uid, native_id, agent, path).map(|r| r.usage)
    }

    pub fn inspect(
        &mut self,
        uid: &str,
        native_id: &str,
        agent: Agent,
        path: &Path,
    ) -> io::Result<TokenRecord> {
        for _ in 0..2 {
            let before = stamp(path, agent)?;
            if let Some(c) = self.data.sessions.get(uid).filter(|c| c.stamp == before) {
                return Ok(c.record.clone());
            }
            let usage = calculate(path, agent, native_id)?;
            let record = TokenRecord {
                usage,
                children: child_logs(path, agent, native_id)?,
            };
            if before != stamp(path, agent)? {
                continue;
            }
            if record.usage.is_some() {
                self.data.sessions.insert(
                    uid.to_owned(),
                    Cached {
                        stamp: before,
                        record: record.clone(),
                    },
                );
                if let Err(e) = self.save() {
                    self.data.sessions.remove(uid);
                    return Err(e);
                }
            }
            return Ok(record);
        }
        Err(io::Error::other("session changed during token calculation"))
    }

    fn save(&self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec(&self.data)?)?;
        fs::rename(tmp, &self.path)
    }
}

fn count(v: &Value, key: &str) -> u64 {
    v[key].as_u64().unwrap_or(0)
}

fn child_logs(path: &Path, agent: Agent, native_id: &str) -> io::Result<Vec<ChildLog>> {
    let mut children = BTreeMap::new();
    if agent == Agent::ClaudeCode {
        let dir = path.with_extension("").join("subagents");
        match fs::read_dir(dir) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry?;
                    if !entry.file_type()?.is_file() {
                        continue;
                    }
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if let Some(id) = name
                        .strip_prefix("agent-")
                        .and_then(|s| s.strip_suffix(".jsonl"))
                    {
                        let base = native_id
                            .split(crate::claude::AGENT_PREFIX)
                            .next()
                            .unwrap_or(native_id);
                        children.insert(
                            format!("{base}{}{id}", crate::claude::AGENT_PREFIX),
                            Some(entry.path()),
                        );
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    } else {
        let log = if agent == Agent::Antigravity {
            transcript_path(path)
        } else {
            path.to_owned()
        };
        let file = match File::open(log) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound && agent == Agent::Antigravity => {
                return Ok(Vec::new())
            }
            Err(e) => return Err(e),
        };
        let mut history_end = None;
        let mut seen_meta = false;
        let mut spawns = BTreeSet::new();
        for (ordinal, line) in BufReader::new(file).lines().enumerate() {
            let line = line?;
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if agent == Agent::Antigravity {
                collect_conversation_ids(&v, native_id, &mut children);
                continue;
            }
            let p = &v["payload"];
            if v["type"] == "session_meta" && !seen_meta {
                seen_meta = true;
                history_end = p["subagent_history_start_ordinal"].as_u64();
            }
            if history_end.is_some_and(|end| (ordinal as u64) < end) {
                continue;
            }
            let item = &p["item"];
            if v["type"] == "event_msg"
                && p["type"] == "item_completed"
                && item["type"] == "SubAgentActivity"
                && item["kind"] == "started"
            {
                if let Some(id) = item["agent_thread_id"].as_str() {
                    children.insert(id.to_owned(), None);
                }
            }
            if v["type"] == "response_item" {
                if p["name"] == "spawn_agent" {
                    if let Some(id) = p["call_id"].as_str() {
                        spawns.insert(id.to_owned());
                    }
                }
                if p["call_id"].as_str().is_some_and(|id| spawns.contains(id)) {
                    let output = p["output"]
                        .as_str()
                        .and_then(|s| serde_json::from_str::<Value>(s).ok());
                    if let Some(id) = output.as_ref().and_then(|o| {
                        o["agent_id"]
                            .as_str()
                            .or_else(|| o["agent_thread_id"].as_str())
                    }) {
                        children.insert(id.to_owned(), None);
                    }
                }
            }
        }
    }
    children.remove(native_id);
    Ok(children
        .into_iter()
        .map(|(native_id, path)| ChildLog { native_id, path })
        .collect())
}

fn collect_conversation_ids(
    v: &Value,
    parent: &str,
    children: &mut BTreeMap<String, Option<PathBuf>>,
) {
    match v {
        Value::Object(object) => {
            for (key, value) in object {
                if matches!(
                    key.as_str(),
                    "conversationId" | "conversation_id" | "subagent_session_id"
                ) {
                    if let Some(id) = value.as_str().filter(|id| {
                        *id != parent
                            && id.len() == 36
                            && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
                    }) {
                        children.insert(id.to_owned(), None);
                    }
                }
                collect_conversation_ids(value, parent, children);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_conversation_ids(value, parent, children);
            }
        }
        Value::String(text) => {
            if let Ok(decoded) = serde_json::from_str::<Value>(text) {
                if decoded.is_object() || decoded.is_array() {
                    collect_conversation_ids(&decoded, parent, children);
                }
            }
        }
        _ => {}
    }
}

/// WSL helpers publish a root rather than host file handles. Resolve only known session filenames.
pub fn find_session_log(root: &Path, agent: Agent, native_id: &str) -> io::Result<PathBuf> {
    let (parent, child) = native_id
        .split_once(crate::claude::AGENT_PREFIX)
        .unwrap_or((native_id, ""));
    let mut paths = Vec::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(io::Error::other)?;
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        let matches = match agent {
            Agent::Codex => {
                name.starts_with("rollout-") && name.ends_with(&format!("-{native_id}.jsonl"))
            }
            Agent::ClaudeCode if !child.is_empty() => {
                name == format!("agent-{child}.jsonl")
                    && entry
                        .path()
                        .ancestors()
                        .any(|p| p.file_name().is_some_and(|n| n == parent))
            }
            Agent::ClaudeCode => name == format!("{native_id}.jsonl"),
            Agent::Antigravity => name == format!("{native_id}.db"),
        };
        let legacy_sidechain =
            agent == Agent::ClaudeCode && !child.is_empty() && name == format!("{parent}.jsonl");
        if matches || legacy_sidechain {
            paths.push((matches, entry.into_path()));
        }
    }
    paths.sort_by_key(|(exact, p)| {
        std::cmp::Reverse((*exact, fs::metadata(p).and_then(|m| m.modified()).ok()))
    });
    paths
        .into_iter()
        .next()
        .map(|(_, path)| path)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "session log not found"))
}

fn usage_object(v: &Value) -> bool {
    v.is_object()
        && [
            "input_tokens",
            "output_tokens",
            "cached_input_tokens",
            "cache_read_input_tokens",
            "cache_creation_input_tokens",
        ]
        .iter()
        .any(|k| v[*k].as_u64().is_some())
}

pub fn calculate(path: &Path, agent: Agent, native_id: &str) -> io::Result<Option<TokenUsage>> {
    if agent == Agent::Antigravity {
        return agy(path);
    }
    let mut latest = None;
    let mut messages = HashMap::new();
    let path_agent = path_agent_id(path);
    let mut history_end = None;
    let mut seen_meta = false;
    for (ordinal, line) in BufReader::new(File::open(path)?).lines().enumerate() {
        let line = line?;
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match agent {
            Agent::Codex => {
                let p = &v["payload"];
                if v["type"] == "session_meta" && !seen_meta {
                    seen_meta = true;
                    history_end = p["subagent_history_start_ordinal"].as_u64();
                }
                if history_end.is_some_and(|end| (ordinal as u64) < end) {
                    continue;
                }
                if v["type"] != "event_msg" || p["type"] != "token_count" {
                    continue;
                }
                let raw = &p["info"]["total_token_usage"];
                if !usage_object(raw) {
                    continue;
                }
                let input = count(raw, "input_tokens");
                let cached = count(raw, "cached_input_tokens").min(input);
                let creation = count(raw, "cache_write_input_tokens").min(input - cached);
                latest = Some(TokenUsage {
                    cached_input_tokens: cached,
                    uncached_input_tokens: input - cached - creation,
                    cache_creation_input_tokens: creation,
                    output_tokens: count(raw, "output_tokens"),
                    ..Default::default()
                });
            }
            Agent::ClaudeCode => {
                if v["type"] != "assistant" {
                    continue;
                }
                if !session_identity(&v, path_agent.as_deref())
                    .is_some_and(|(id, _, _)| id == native_id)
                {
                    continue;
                }
                let msg = &v["message"];
                let raw = &msg["usage"];
                if !usage_object(raw) {
                    continue;
                }
                // Streaming/content-block copies share a message.id: keep the final usage.
                let key = msg["id"]
                    .as_str()
                    .or_else(|| v["uuid"].as_str())
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("line:{ordinal}"));
                messages.insert(
                    key,
                    TokenUsage {
                        cached_input_tokens: count(raw, "cache_read_input_tokens"),
                        uncached_input_tokens: count(raw, "input_tokens"),
                        cache_creation_input_tokens: count(raw, "cache_creation_input_tokens"),
                        output_tokens: count(raw, "output_tokens"),
                        ..Default::default()
                    },
                );
            }
            Agent::Antigravity => unreachable!(),
        }
    }
    if agent == Agent::ClaudeCode && !messages.is_empty() {
        let mut total = TokenUsage::default();
        for usage in messages.values() {
            total.add(usage);
        }
        latest = Some(total);
    }
    Ok(latest.map(TokenUsage::finish))
}

fn proto_usage(data: &[u8]) -> Option<TokenUsage> {
    let input = get_varint_field(data, 2);
    let output = get_varint_field(data, 3);
    let cached = get_varint_field(data, 5);
    let creation = get_varint_field(data, 4);
    if input.is_none() && output.is_none() && cached.is_none() && creation.is_none() {
        return None;
    }
    Some(TokenUsage {
        uncached_input_tokens: input.unwrap_or(0),
        output_tokens: output.unwrap_or(0),
        cached_input_tokens: cached.unwrap_or(0),
        cache_creation_input_tokens: creation.unwrap_or(0),
        ..Default::default()
    })
}

fn agy(path: &Path) -> io::Result<Option<TokenUsage>> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(io::Error::other)?;
    db.execute_batch("PRAGMA query_only = ON; PRAGMA busy_timeout = 1000;")
        .map_err(io::Error::other)?;
    let mut total = TokenUsage::default();
    let mut found = false;
    // Prefer the step usage used by calc-session-tokens. Older/newer DBs may expose gen_metadata only.
    for (query, step) in [
        (
            "SELECT metadata FROM steps WHERE metadata IS NOT NULL ORDER BY idx",
            true,
        ),
        (
            "SELECT data FROM gen_metadata WHERE data IS NOT NULL ORDER BY idx",
            false,
        ),
    ] {
        let mut stmt = match db.prepare(query) {
            Ok(stmt) => stmt,
            Err(rusqlite::Error::SqliteFailure(_, Some(ref msg)))
                if msg.starts_with("no such table:") || msg.starts_with("no such column:") =>
            {
                continue
            }
            Err(e) => return Err(io::Error::other(e)),
        };
        let rows = stmt
            .query_map([], |row| row.get::<_, Vec<u8>>(0))
            .map_err(io::Error::other)?;
        for row in rows {
            let data = row.map_err(io::Error::other)?;
            let usage = if step {
                get_len_delim_field(&data, 9)
            } else {
                get_len_delim_field(&data, 1).and_then(|chat| get_len_delim_field(chat, 4))
            };
            if let Some(usage) = usage.and_then(proto_usage) {
                total.add(&usage);
                found = true;
            }
        }
        if found {
            break;
        } // Never count the same generation from both tables.
    }
    Ok(found.then(|| total.finish()))
}
