//! Display-string shaping applied in Rust before anything crosses IPC (architecture-v3 §7).

use crate::consts::{AWS_LEN, GH_LEN, R4_WORDS, SK_MIN, TRUNC_OUT_CHARS, TRUNC_TEXT_CHARS};
use regex::Regex;
use std::sync::OnceLock;

pub const MAX_MESSAGE_CHARS: usize = 2000;
pub const MAX_TOOL_INPUT_CHARS: usize = 2000;

fn secret_patterns() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r"\bsk-[A-Za-z0-9_\-]{{{sk},}}|\bgh[pousr]_[A-Za-z0-9]{{{gh},}}|\bAKIA[0-9A-Z]{{{aws}}}\b",
            sk = SK_MIN,
            gh = GH_LEN,
            aws = AWS_LEN
        ))
        .expect("static regex")
    })
}

/// R4: named secret assignment; group 1 is the name + delimiter, kept as is.
fn named_secret_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r"(?i)(\b[A-Z0-9_]*(?:{})[A-Z0-9_]*\s*[=:]\s*)\S+",
            R4_WORDS.join("|")
        ))
        .expect("static regex")
    })
}

fn redact(s: &str) -> String {
    let r = secret_patterns().replace_all(s, "[REDACTED]");
    named_secret_pattern()
        .replace_all(&r, "${1}[REDACTED]")
        .into_owned()
}

/// First `TRUNC_TEXT_CHARS` chars.
pub fn truncate_text(s: &str) -> String {
    s.chars().take(TRUNC_TEXT_CHARS).collect()
}

/// Whole if <= 2*`TRUNC_OUT_CHARS` chars, else first + last `TRUNC_OUT_CHARS` chars.
pub fn truncate_output(s: &str) -> String {
    let n = s.chars().count();
    if n <= TRUNC_OUT_CHARS * 2 {
        return s.to_string();
    }
    let head: String = s.chars().take(TRUNC_OUT_CHARS).collect();
    let tail: String = s.chars().skip(n - TRUNC_OUT_CHARS).collect();
    head + &tail
}

/// What kind of field is being sanitized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Text,
    Output,
}

/// Single entry point: redact, then truncate.
pub fn sanitize(s: &str, field: Field) -> String {
    let r = redact(s);
    match field {
        Field::Text => truncate_text(&r),
        Field::Output => truncate_output(&r),
    }
}

pub fn truncate_chars(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((idx, _)) => format!("{}…", &s[..idx]),
        None => s.to_string(),
    }
}

/// Collapses whitespace, redacts, then truncates.
pub fn shape(s: &str, max: usize) -> String {
    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&redact(&collapsed), max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_known_secret_shapes() {
        let s = "key sk-ant-abcdefghijklmnopqrstuvwxyz0123 and AKIAABCDEFGHIJKLMNOP";
        assert_eq!(redact(s), "key [REDACTED] and [REDACTED]");
    }

    #[test]
    fn truncates_on_char_boundary() {
        assert_eq!(truncate_chars("日本語テキスト", 3), "日本語…");
        assert_eq!(truncate_chars("abc", 3), "abc");
    }

    #[test]
    fn shape_collapses_whitespace() {
        assert_eq!(shape("a\n\n  b\tc", 10), "a b c");
    }
}
