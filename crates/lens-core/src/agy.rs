//! Antigravity (AGY) adapter: read-only SQLite & lightweight protobuf wire decoder.

use crate::model::SessionKind;
use crate::session::{SessionState, TurnEnd};
use chrono::{DateTime, Utc};
use percent_encoding::percent_decode_str;
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

#[derive(Debug, Default, Clone)]
pub struct AgyFile {
    pub session: Option<SessionState>,
}

/// Lightweight protobuf wire decoder. Decodes varint from buffer.
pub fn decode_varint(data: &[u8], offset: &mut usize) -> Option<u64> {
    let mut res = 0u64;
    let mut shift = 0;
    while *offset < data.len() {
        let b = data[*offset];
        *offset += 1;
        res |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Some(res);
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
    None
}

/// Skips one field based on wire type.
fn skip_field(wire_type: u8, data: &[u8], offset: &mut usize) -> bool {
    match wire_type {
        0 => decode_varint(data, offset).is_some(),
        1 if *offset + 8 <= data.len() => {
            *offset += 8;
            true
        }
        2 => {
            if let Some(len) = decode_varint(data, offset) {
                let len = len as usize;
                if *offset + len <= data.len() {
                    *offset += len;
                    return true;
                }
            }
            false
        }
        5 if *offset + 4 <= data.len() => {
            *offset += 4;
            true
        }
        _ => false,
    }
}

/// Finds a length-delimited field (wire type 2) with the given field number.
pub fn get_len_delim_field(data: &[u8], target_num: u32) -> Option<&[u8]> {
    let mut offset = 0;
    while offset < data.len() {
        let tag = decode_varint(data, &mut offset)?;
        let wire_type = (tag & 0x7) as u8;
        let field_num = (tag >> 3) as u32;

        if wire_type == 2 {
            let len = decode_varint(data, &mut offset)? as usize;
            if offset + len > data.len() {
                return None;
            }
            let payload = &data[offset..offset + len];
            offset += len;
            if field_num == target_num {
                return Some(payload);
            }
        } else if !skip_field(wire_type, data, &mut offset) {
            return None;
        }
    }
    None
}

/// Finds a varint field (wire type 0) with the given field number.
pub fn get_varint_field(data: &[u8], target_num: u32) -> Option<u64> {
    let mut offset = 0;
    while offset < data.len() {
        let tag = decode_varint(data, &mut offset)?;
        let wire_type = (tag & 0x7) as u8;
        let field_num = (tag >> 3) as u32;

        if wire_type == 0 {
            let val = decode_varint(data, &mut offset)?;
            if field_num == target_num {
                return Some(val);
            }
        } else if !skip_field(wire_type, data, &mut offset) {
            return None;
        }
    }
    None
}

/// Decodes a `google.protobuf.Timestamp` message.
pub fn decode_timestamp(data: &[u8]) -> Option<DateTime<Utc>> {
    let seconds = get_varint_field(data, 1)? as i64;
    let nanos = get_varint_field(data, 2).unwrap_or(0) as u32;
    DateTime::from_timestamp(seconds, nanos)
}

/// Parses a string from a length-delimited field.
pub fn get_string_field(data: &[u8], target_num: u32) -> Option<String> {
    let bytes = get_len_delim_field(data, target_num)?;
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

/// Converts a `file:///...` URI into a clean filesystem path.
pub fn parse_cwd_from_uri(uri: &str) -> Option<String> {
    let trimmed = uri.trim();
    if trimmed.is_empty() {
        return None;
    }
    let rest = trimmed.strip_prefix("file://").unwrap_or(trimmed);
    let decoded = percent_decode_str(rest).decode_utf8().ok()?.into_owned();
    let t = decoded.strip_prefix('/').unwrap_or(&decoded);
    let b = t.as_bytes();
    let is_windows = b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic();
    let path = if is_windows {
        t.replace('/', "\\")
    } else {
        decoded
    };
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

impl AgyFile {
    /// Reads an Antigravity conversation SQLite database strictly read-only.
    pub fn load(path: &Path) -> Result<SessionState, rusqlite::Error> {
        let path_str = path.to_string_lossy().replace('\\', "/");
        // Strictly read-only connection without immutable=1 (so WAL uncheckpointed steps are visible)
        let uri = format!("file:{path_str}?mode=ro");
        let conn = Connection::open_with_flags(&uri, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI)?;
        conn.execute_batch("PRAGMA query_only = ON; PRAGMA busy_timeout = 1000;")?;

        // Fallback native ID from file stem
        let stem_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let mut cascade_id = None;
        if let Ok(mut stmt) = conn.prepare("SELECT cascade_id FROM trajectory_meta LIMIT 1") {
            if let Ok(mut rows) = stmt.query([]) {
                if let Ok(Some(row)) = rows.next() {
                    cascade_id = row.get::<_, Option<String>>(0).ok().flatten();
                }
            }
        }
        let native_id = cascade_id.unwrap_or(stem_id);

        let mut s = SessionState::new(native_id, SessionKind::TopLevel);

        // Read trajectory_metadata_blob for cwd and started_at
        if let Ok(mut stmt) = conn.prepare("SELECT data FROM trajectory_metadata_blob LIMIT 1") {
            if let Ok(mut rows) = stmt.query([]) {
                if let Ok(Some(row)) = rows.next() {
                    if let Ok(data) = row.get::<_, Vec<u8>>(0) {
                        // Field 7 = URI, or Field 1.1 = URI
                        let uri_opt = get_string_field(&data, 7).or_else(|| {
                            get_len_delim_field(&data, 1).and_then(|f1| get_string_field(f1, 1))
                        });
                        if let Some(uri) = uri_opt {
                            s.cwd = parse_cwd_from_uri(&uri);
                        }
                        // Field 2 = Timestamp
                        if let Some(ts_data) = get_len_delim_field(&data, 2) {
                            if let Some(ts) = decode_timestamp(ts_data) {
                                s.started_at = Some(ts);
                            }
                        }
                    }
                }
            }
        }

        // Read steps
        let mut stmt = conn.prepare(
            "SELECT idx, step_type, status, metadata, step_payload FROM steps ORDER BY idx ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let idx: i64 = row.get(0)?;
            let step_type: i64 = row.get(1)?;
            let status: i64 = row.get(2)?;
            let metadata: Option<Vec<u8>> = row.get(3)?;
            let payload: Option<Vec<u8>> = row.get(4)?;
            Ok((idx, step_type, status, metadata, payload))
        })?;

        for r in rows {
            let (idx, step_type, status, metadata, payload) = match r {
                Ok(t) => t,
                Err(_) => {
                    s.diag("malformed_record");
                    s.degraded = true;
                    continue;
                }
            };

            // Timestamp from metadata Field 1
            let ts = metadata.as_deref().and_then(|m| {
                get_len_delim_field(m, 1).and_then(decode_timestamp)
            });
            s.touch(ts);

            let payload_bytes = payload.as_deref().unwrap_or_default();

            match step_type {
                // User prompt
                14 => {
                    if let Some(f19) = get_len_delim_field(payload_bytes, 19) {
                        if let Some(text) = get_string_field(f19, 2) {
                            s.human_message(&text, ts);
                        }
                    }
                }
                // Assistant response / Tool call
                15 => {
                    if let Some(f20) = get_len_delim_field(payload_bytes, 20) {
                        // Check if it initiates a tool call (Field 7)
                        if let Some(f7) = get_len_delim_field(f20, 7) {
                            let call_id = get_string_field(f7, 1).unwrap_or_else(|| format!("call_{idx}"));
                            let name = get_string_field(f7, 2).unwrap_or_else(|| "tool".to_string());
                            let input = get_string_field(f7, 3).unwrap_or_default();
                            s.tool_start(call_id, &name, &input, ts);
                        }
                        // Check text response (Field 1 or Field 8)
                        let text_opt = get_string_field(f20, 1).or_else(|| get_string_field(f20, 8));
                        if let Some(text) = text_opt {
                            if !text.is_empty() {
                                s.assistant_text(&text);
                                if status == 3 && s.open_tools.is_empty() {
                                    s.end_turn(TurnEnd::Complete, ts, "assistant response");
                                }
                            }
                        }
                    }
                    if status == 7 {
                        s.end_turn(TurnEnd::Error, ts, "step error");
                    } else if status == 6 {
                        s.end_turn(TurnEnd::Aborted, ts, "step cancelled");
                    }
                }
                // Legacy command execution
                21 => {
                    let cmd_id = format!("cmd_{idx}");
                    if let Some(f28) = get_len_delim_field(payload_bytes, 28) {
                        let cmd = get_string_field(f28, 23)
                            .or_else(|| get_string_field(f28, 25))
                            .unwrap_or_default();
                        if status == 2 {
                            s.tool_start(cmd_id.clone(), "run_command", &cmd, ts);
                        } else {
                            s.tool_end(&cmd_id);
                        }
                    }
                    if status == 7 {
                        s.end_turn(TurnEnd::Error, ts, "command error");
                    }
                }
                // Tool result
                132 => {
                    if status == 3 {
                        // Close the most recent tool call
                        if let Some(tool) = s.open_tools.last().cloned() {
                            s.tool_end(&tool.call_id);
                        }
                    } else if status == 6 {
                        s.end_turn(TurnEnd::Aborted, ts, "tool cancelled");
                    } else if status == 7 {
                        s.end_turn(TurnEnd::Error, ts, "tool error");
                    }
                }
                _ => {}
            }
        }

        Ok(s)
    }
}
