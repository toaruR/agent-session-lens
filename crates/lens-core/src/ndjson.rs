//! NDJSON receiver, first half (architecture-v5 §6.3, §7 `Msg`).
//!
//! Check order per line (fixed by F13): JSON parse (`malformed_record`) ->
//! object with string `type` (`invalid_message`) -> unknown `type`
//! (`unknown_message`) -> hello-first rule (`invalid_message`) -> shape
//! (`invalid_message`) -> protocol (`helper_version_mismatch`).
//! Byte-level front (`feed`): size (`oversized_line`) -> strict UTF-8
//! (`invalid_utf8`) -> the per-line order above. Incoming sessions are
//! sanitized again (double redaction).

use crate::consts::PROTOCOL_VERSION;
use crate::consts::MAX_LINE_BYTES;
use crate::model::v5::{SourceHealth, UnifiedSession};
use crate::reader::{split_lines, Cursor, Limits, ReadOutcome};
use crate::redact::{sanitize, Field};
use chrono::DateTime;
use serde_json::Value;
use std::collections::BTreeMap;

pub const INVALID_MESSAGE: &str = "invalid_message";
pub const UNKNOWN_MESSAGE: &str = "unknown_message";
pub const MALFORMED_RECORD: &str = "malformed_record";
pub const OVERSIZED_LINE: &str = "oversized_line";
pub const INVALID_UTF8: &str = "invalid_utf8";
pub const HELPER_VERSION_MISMATCH: &str = "helper_version_mismatch";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    pub protocol: i64,
    pub helper_version: String,
    pub distro: String,
    pub home: String,
    pub now: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Accepted {
    Hello,
    SourceHealth,
    SessionUpsert,
    SessionRemove,
    Heartbeat,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Accepted(Accepted),
    /// Dropped; the diagnostic code was counted.
    Rejected(&'static str),
    /// Receiver already stopped; line not examined.
    Stopped,
}

#[derive(Debug, Default)]
pub struct Receiver {
    hello: Option<Hello>,
    stopped: bool,
    restart_allowed: bool,
    diag: BTreeMap<String, u64>,
    sessions: BTreeMap<String, UnifiedSession>,
    health: BTreeMap<String, SourceHealth>,
    last_heartbeat: Option<String>,
    buf: Vec<u8>,
    cursor: Cursor,
}

impl Receiver {
    pub fn new() -> Self {
        Self { restart_allowed: true, ..Default::default() }
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped
    }
    pub fn restart_allowed(&self) -> bool {
        self.restart_allowed
    }
    pub fn hello(&self) -> Option<&Hello> {
        self.hello.as_ref()
    }
    pub fn sessions(&self) -> &BTreeMap<String, UnifiedSession> {
        &self.sessions
    }
    pub fn source_health(&self) -> &BTreeMap<String, SourceHealth> {
        &self.health
    }
    pub fn last_heartbeat(&self) -> Option<&str> {
        self.last_heartbeat.as_deref()
    }
    pub fn diagnostics(&self) -> &BTreeMap<String, u64> {
        &self.diag
    }
    pub fn count(&self, code: &str) -> u64 {
        self.diag.get(code).copied().unwrap_or(0)
    }

    fn reject(&mut self, code: &'static str) -> Outcome {
        *self.diag.entry(code.to_string()).or_insert(0) += 1;
        Outcome::Rejected(code)
    }

    /// Feed raw bytes from the helper's stdout. Complete lines are limit-checked
    /// (<= MAX_LINE_BYTES, strict UTF-8) and handled; an incomplete tail is held.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Outcome> {
        if self.stopped {
            return Vec::new();
        }
        self.buf.extend_from_slice(chunk);
        let limits = Limits { max_line: MAX_LINE_BYTES, ..Limits::default() };
        let mut out = ReadOutcome::default();
        let consumed = split_lines(&self.buf, &mut self.cursor, &limits, &mut out) as usize;
        self.buf.drain(..consumed);
        for _ in 0..out.oversized_lines {
            self.reject(OVERSIZED_LINE);
        }
        for _ in 0..out.invalid_utf8_lines {
            self.reject(INVALID_UTF8);
        }
        let mut res = Vec::with_capacity(out.lines.len());
        for l in out.lines {
            // split_lines guarantees strict UTF-8.
            let text = String::from_utf8_lossy(&l.bytes).into_owned();
            res.push(self.handle_line(&text));
        }
        res
    }

    /// Process one complete, already-decoded line.
    pub fn handle_line(&mut self, line: &str) -> Outcome {
        if self.stopped {
            return Outcome::Stopped;
        }
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => return self.reject(MALFORMED_RECORD),
        };
        let Some(obj) = v.as_object() else { return self.reject(INVALID_MESSAGE) };
        let Some(ty) = obj.get("type").and_then(Value::as_str) else {
            return self.reject(INVALID_MESSAGE);
        };
        if !matches!(
            ty,
            "hello" | "source_health" | "session_upsert" | "session_remove" | "heartbeat" | "error"
        ) {
            return self.reject(UNKNOWN_MESSAGE);
        }
        if ty != "hello" && self.hello.is_none() {
            return self.reject(INVALID_MESSAGE);
        }
        match ty {
            "hello" => self.on_hello(&v),
            "source_health" => {
                let ok = str_field(&v, "sourceId").is_some();
                match (ok, v.get("health").cloned().map(serde_json::from_value::<SourceHealth>)) {
                    (true, Some(Ok(h))) => {
                        self.health.insert(h.source_id.clone(), h);
                        Outcome::Accepted(Accepted::SourceHealth)
                    }
                    _ => self.reject(INVALID_MESSAGE),
                }
            }
            "session_upsert" => match v.get("session").cloned().map(serde_json::from_value::<UnifiedSession>) {
                Some(Ok(mut s)) => {
                    resanitize(&mut s);
                    self.sessions.insert(s.session_uid.clone(), s);
                    Outcome::Accepted(Accepted::SessionUpsert)
                }
                _ => self.reject(INVALID_MESSAGE),
            },
            "session_remove" => match str_field(&v, "sessionUid") {
                Some(uid) => {
                    self.sessions.remove(uid);
                    Outcome::Accepted(Accepted::SessionRemove)
                }
                None => self.reject(INVALID_MESSAGE),
            },
            "heartbeat" => match str_field(&v, "now").filter(|s| is_ts(s)) {
                Some(now) => {
                    self.last_heartbeat = Some(now.to_string());
                    Outcome::Accepted(Accepted::Heartbeat)
                }
                None => self.reject(INVALID_MESSAGE),
            },
            _ => match str_field(&v, "code") {
                Some(c @ ("root_unreadable" | "internal")) => {
                    *self.diag.entry(c.to_string()).or_insert(0) += 1;
                    Outcome::Accepted(Accepted::Error(c.to_string()))
                }
                _ => self.reject(INVALID_MESSAGE),
            },
        }
    }

    fn on_hello(&mut self, v: &Value) -> Outcome {
        let h = (|| {
            let protocol = v.get("protocol")?.as_i64()?;
            let distro = str_field(v, "distro").filter(|s| !s.is_empty())?;
            let home = str_field(v, "home").filter(|s| s.starts_with('/'))?;
            let now = str_field(v, "now").filter(|s| is_ts(s))?;
            Some(Hello {
                protocol,
                helper_version: str_field(v, "helperVersion")?.to_string(),
                distro: distro.to_string(),
                home: home.to_string(),
                now: now.to_string(),
            })
        })();
        let Some(h) = h else { return self.reject(INVALID_MESSAGE) };
        if h.protocol != i64::from(PROTOCOL_VERSION) {
            self.stopped = true;
            self.restart_allowed = false;
            return self.reject(HELPER_VERSION_MISMATCH);
        }
        self.hello = Some(h);
        Outcome::Accepted(Accepted::Hello)
    }
}

/// Double redaction: never trust the helper's sanitization.
fn resanitize(s: &mut UnifiedSession) {
    let t = &mut s.current_task;
    for x in [&mut t.title, &mut t.last_user_message, &mut t.last_assistant_message].into_iter().flatten() {
        *x = sanitize(x, Field::Text);
    }
    if let Some(a) = &mut t.active_tool {
        a.input = sanitize(&a.input, Field::Text);
    }
}

fn str_field<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k)?.as_str()
}

fn is_ts(s: &str) -> bool {
    DateTime::parse_from_rfc3339(s).is_ok()
}
