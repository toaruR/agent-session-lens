//! Codex `session_index.jsonl` thread-name cache.

use super::Engine;
use crate::model::Agent;
use serde_json::Value;
use std::collections::HashMap;

/// Codex `session_index.jsonl` cache entry: (mtime, len, id -> thread_name).
pub(super) type ThreadNameCache = (Option<std::time::SystemTime>, u64, HashMap<String, String>);

/// Parses `session_index.jsonl`; later lines win, blank names are ignored.
fn parse_session_index(text: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let (Some(id), Some(name)) = (v.get("id").and_then(Value::as_str), v.get("thread_name").and_then(Value::as_str)) else { continue };
        if !name.trim().is_empty() {
            m.insert(id.to_string(), name.to_string());
        }
    }
    m
}

impl Engine {
    /// Refreshes the Codex thread-name cache (the index sits beside the `sessions` root).
    pub(super) fn refresh_thread_names(&mut self) {
        for (i, src) in self.sources.iter().enumerate() {
            if src.agent != Agent::Codex {
                continue;
            }
            let path = src.root.parent().map(|p| p.join("session_index.jsonl"));
            let meta = path.as_ref().and_then(|p| std::fs::metadata(p).ok());
            let key = (meta.as_ref().and_then(|m| m.modified().ok()), meta.as_ref().map_or(0, |m| m.len()));
            if self.thread_names.get(&i).is_some_and(|c| (c.0, c.1) == key) {
                continue;
            }
            let names = match (&meta, &path) {
                (Some(_), Some(p)) => std::fs::read(p).map(|b| parse_session_index(&String::from_utf8_lossy(&b))).unwrap_or_default(),
                _ => HashMap::new(),
            };
            self.thread_names.insert(i, (key.0, key.1, names));
        }
    }
}
