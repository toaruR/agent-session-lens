//! Incremental reading of one tracked file into its parser.

use super::parser::{Parser, Probe};
use super::Engine;
use crate::reader;
use serde_json::Value;
use std::path::Path;

impl Engine {
    pub(super) fn process_file(&mut self, path: &Path) -> bool {
        let Some(tf) = self.files.get_mut(path) else { return false };
        let outcome = match reader::read_new(path, &mut tf.cursor, &self.limits) {
            Ok(o) => {
                tf.read_error = None;
                o
            }
            Err(e) => {
                tf.read_error = Some(e.kind());
                return false;
            }
        };
        if outcome.replaced {
            tf.parser = Parser::new(self.sources[tf.source].agent, path);
            if let Some(p) = tf.parser.primary_mut() {
                p.diag("file_replaced");
            }
        }
        let mut pending_diags: Vec<&str> = Vec::new();
        let mut attached_invalid = 0u64;
        for line in &outcome.lines {
            for _ in 0..line.invalid_before {
                Self::unreadable(&mut tf.parser, &mut pending_diags, "invalid_utf8");
            }
            attached_invalid += u64::from(line.invalid_before);
            let text = match std::str::from_utf8(&line.bytes) {
                Ok(t) => t,
                Err(_) => {
                    Self::unreadable(&mut tf.parser, &mut pending_diags, "invalid_utf8");
                    continue;
                }
            };
            // Cheap probe first: large irrelevant records (request blobs, snapshots) skip the Value build.
            let ty_key: String = match serde_json::from_str::<Probe>(text) {
                Ok(p) if !tf.parser.wants(p.ty.as_deref()) => continue,
                Ok(p) => p.ty.as_deref().unwrap_or_default().to_string(),
                Err(_) => {
                    Self::unreadable(&mut tf.parser, &mut pending_diags, "malformed_record");
                    continue;
                }
            };
            *self.full_parses.entry(ty_key).or_default() += 1;
            match serde_json::from_str::<Value>(text) {
                Ok(v) => {
                    tf.parser.apply(line.line_no, &v);
                    if let Some(p) = tf.parser.primary_mut() {
                        for d in pending_diags.drain(..) {
                            p.diag(d);
                            p.degraded = true;
                        }
                    }
                }
                Err(_) => Self::unreadable(&mut tf.parser, &mut pending_diags, "malformed_record"),
            }
        }
        for _ in attached_invalid..outcome.invalid_utf8_lines {
            Self::unreadable(&mut tf.parser, &mut pending_diags, "invalid_utf8");
        }
        for _ in 0..outcome.oversized_lines {
            Self::unreadable(&mut tf.parser, &mut pending_diags, "oversized_line");
        }
        if !outcome.lines.is_empty() {
            for s in tf.parser.sessions_mut() {
                s.tail_empty = false;
            }
        }
        if outcome.tail_empty {
            // §3.1 rule 1: no complete line inside TAIL_SCAN_BYTES.
            for s in tf.parser.sessions_mut() {
                s.tail_empty = true;
            }
        }
        if outcome.truncated_initial_scan {
            for s in tf.parser.sessions_mut() {
                s.truncated_scan = true;
                s.diag("truncated_initial_scan");
            }
        }
        outcome.more
    }

    fn unreadable(parser: &mut Parser, pending: &mut Vec<&'static str>, code: &'static str) {
        match parser.primary_mut() {
            Some(p) => {
                p.diag(code);
                p.degraded = true;
                p.last_record_unreadable = true;
            }
            None => pending.push(code),
        }
    }
}
