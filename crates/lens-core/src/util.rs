use chrono::{DateTime, Utc};
use serde_json::Value;

pub fn str_at<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut cur = v;
    for key in path {
        cur = cur.get(key)?;
    }
    cur.as_str()
}

pub fn parse_ts(v: Option<&Value>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(v?.as_str()?).ok().map(|d| d.with_timezone(&Utc))
}

/// A short, human-readable description of a tool input.
pub fn tool_input_summary(input: &Value) -> String {
    const KEYS: [&str; 9] = ["command", "cmd", "file_path", "path", "pattern", "url", "query", "description", "prompt"];
    match input {
        Value::Object(map) => {
            for k in KEYS {
                match map.get(k) {
                    Some(Value::String(s)) => return s.clone(),
                    Some(Value::Array(a)) => {
                        let parts: Vec<&str> = a.iter().filter_map(Value::as_str).collect();
                        if !parts.is_empty() {
                            return parts.join(" ");
                        }
                    }
                    _ => {}
                }
            }
            serde_json::to_string(input).unwrap_or_default()
        }
        Value::String(s) => match serde_json::from_str::<Value>(s) {
            Ok(inner @ Value::Object(_)) => tool_input_summary(&inner),
            _ => s.lines().next().unwrap_or_default().to_string(),
        },
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn summarizes_common_tool_inputs() {
        assert_eq!(tool_input_summary(&json!({"command": "npm test", "timeout": 5})), "npm test");
        assert_eq!(tool_input_summary(&json!("{\"cmd\":[\"git\",\"status\"]}")), "git status");
        assert_eq!(tool_input_summary(&json!("*** Begin Patch\n*** Update File: a")), "*** Begin Patch");
    }
}
