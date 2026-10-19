//! Adds the path marker `⟨${rootPath}⟩` to an editor's `window.title`, so windows can be matched
//! by full path. Same rewrite as vscode-selector's TitlePathSettings: settings.json is JSONC
//! (comments, trailing commas), so only the relevant span is edited instead of re-serializing.

use std::path::{Path, PathBuf};

pub const PATH_MARKER: &str = "⟨${rootPath}⟩";
// VS Code's default window.title plus the path marker.
const DEFAULT_TITLE: &str = "${dirty}${activeEditorShort}${separator}${rootName} ⟨${rootPath}⟩${separator}${profileName}${separator}${appName}";

/// User settings folder under %APPDATA% for each editor, keyed by the app name in window titles.
pub const EDITOR_DIRS: [(&str, &str); 6] = [
    ("Visual Studio Code - Insiders", "Code - Insiders"),
    ("Visual Studio Code", "Code"),
    ("VSCodium", "VSCodium"),
    ("Code - OSS", "Code - OSS"),
    ("Cursor", "Cursor"),
    ("Antigravity IDE", "Antigravity IDE"),
];

/// The default profile's settings.json (even if not created yet) and every profile's settings.json.
pub fn settings_files(appdata: &Path, dir: &str) -> Vec<PathBuf> {
    let user = appdata.join(dir).join("User");
    let mut files = vec![user.join("settings.json")];
    if let Ok(entries) = std::fs::read_dir(user.join("profiles")) {
        let mut profiles: Vec<PathBuf> = entries.flatten().map(|e| e.path().join("settings.json")).filter(|f| f.is_file()).collect();
        profiles.sort();
        files.extend(profiles);
    }
    files
}

/// Adds the marker to each file, keeping `.bak` of changed ones. Returns a message per failed file.
pub fn install(files: &[PathBuf]) -> Vec<String> {
    let mut errors = Vec::new();
    for f in files {
        let result = (|| -> Result<(), String> {
            let text = if f.exists() { std::fs::read_to_string(f).map_err(|e| e.to_string())? } else { String::new() };
            let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
            let updated = add_marker(text).ok_or("解析できませんでした")?;
            if updated == text { return Ok(()); }
            if f.exists() {
                std::fs::copy(f, f.with_extension("json.bak")).map_err(|e| e.to_string())?;
            } else if let Some(dir) = f.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(f, updated).map_err(|e| e.to_string())
        })();
        if let Err(e) = result { errors.push(format!("{}: {e}", f.display())); }
    }
    errors
}

/// settings.json text with the marker added. The same text if already present, `None` if unparsable.
pub fn add_marker(text: &str) -> Option<String> {
    if text.trim().is_empty() {
        return Some(format!("{{\n\t\"window.title\": \"{DEFAULT_TITLE}\"\n}}\n"));
    }
    let b = text.as_bytes();
    let (mut open, mut depth, mut first_key) = (None, 0i32, None);
    let mut last_key = None;
    let mut expect_value = false;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'/') {
            i += text[i..].find('\n')? + 1;
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i += 2 + text[i + 2..].find("*/")? + 2;
            continue;
        }
        if c == b'"' {
            let end = string_end(b, i)?;
            if depth == 1 {
                if expect_value {
                    if last_key == Some("window.title") { return Some(insert_into_value(text, i + 1, end)); }
                    expect_value = false;
                } else {
                    last_key = Some(&text[i + 1..end]);
                    first_key.get_or_insert(i);
                }
            }
            i = end + 1;
            continue;
        }
        match c {
            b'{' | b'[' => {
                if depth == 0 && c == b'{' && open.is_none() { open = Some(i); }
                depth += 1;
                expect_value = false;
            }
            b'}' | b']' => {
                depth -= 1;
                if depth == 0 { return Some(insert_key(text, open?, first_key)); }
            }
            b':' => if depth == 1 { expect_value = true; },
            _ => if depth == 1 { expect_value = false; },
        }
        open?;
        i += 1;
    }
    None
}

/// Closing quote of the string opened at `start`.
fn string_end(b: &[u8], start: usize) -> Option<usize> {
    let mut i = start + 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 1,
            b'"' => return Some(i),
            b'\n' => return None,
            _ => {}
        }
        i += 1;
    }
    None
}

/// Inserts the marker into the existing window.title value `text[from..to]`.
fn insert_into_value(text: &str, from: usize, to: usize) -> String {
    let value = &text[from..to];
    if value.contains(PATH_MARKER) { return text.to_string(); }
    const ROOT_NAME: &str = "${rootName}";
    let at = match value.find(ROOT_NAME) {
        Some(a) => a + ROOT_NAME.len(),
        None => value.rfind("${separator}${appName}").unwrap_or(value.len()),
    };
    format!("{} {PATH_MARKER}{}", &text[..from + at], &text[from + at..])
}

/// Adds window.title before the first key (right after `{` for an empty object).
fn insert_key(text: &str, open: usize, first_key: Option<usize>) -> String {
    let entry = format!("\"window.title\": \"{DEFAULT_TITLE}\"");
    let Some(first) = first_key else {
        return format!("{}\n\t{entry}{}", &text[..open + 1], &text[open + 1..]);
    };
    let line_start = text[..first].rfind('\n').map_or(0, |n| n + 1);
    let indent = &text[line_start..first];
    let indent = if indent.trim().is_empty() { indent } else { "\t" };
    format!("{}{entry},\n{}{}", &text[..first], if line_start > open { indent } else { "" }, &text[first..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_gets_default_title() {
        assert_eq!(add_marker("").unwrap(), format!("{{\n\t\"window.title\": \"{DEFAULT_TITLE}\"\n}}\n"));
    }

    #[test]
    fn inserts_after_root_name() {
        let t = "{\n  // c\n  \"window.title\": \"${rootName}${separator}${appName}\",\n}";
        assert_eq!(add_marker(t).unwrap(), "{\n  // c\n  \"window.title\": \"${rootName} ⟨${rootPath}⟩${separator}${appName}\",\n}");
    }

    #[test]
    fn inserts_before_app_name_or_at_end() {
        assert_eq!(add_marker(r#"{"window.title": "${folderName}${separator}${appName}"}"#).unwrap(), r#"{"window.title": "${folderName} ⟨${rootPath}⟩${separator}${appName}"}"#);
        assert_eq!(add_marker(r#"{"window.title": "x"}"#).unwrap(), r#"{"window.title": "x ⟨${rootPath}⟩"}"#);
    }

    #[test]
    fn already_present_is_unchanged() {
        let t = r#"{"window.title": "${rootName} ⟨${rootPath}⟩"}"#;
        assert_eq!(add_marker(t).unwrap(), t);
    }

    #[test]
    fn adds_key_before_first_key() {
        let t = "{\n\t/* x */\n\t\"a\": {\"window.title\": 1},\n\t\"b\": \"日本語\",\n}";
        assert_eq!(add_marker(t).unwrap(), format!("{{\n\t/* x */\n\t\"window.title\": \"{DEFAULT_TITLE}\",\n\t\"a\": {{\"window.title\": 1}},\n\t\"b\": \"日本語\",\n}}"));
        assert_eq!(add_marker("{}").unwrap(), format!("{{\n\t\"window.title\": \"{DEFAULT_TITLE}\"}}"));
    }

    #[test]
    fn unparsable_is_none() {
        assert_eq!(add_marker("not json"), None);
        assert_eq!(add_marker("{\"a\": \"x"), None);
        assert_eq!(add_marker("{ /* open"), None);
    }

    #[test]
    fn install_writes_and_backs_up() {
        let dir = std::env::temp_dir().join(format!("asl-title-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let files = settings_files(&dir, "Code");
        assert!(install(&files).is_empty());
        let f = &files[0];
        assert!(std::fs::read_to_string(f).unwrap().contains(PATH_MARKER));
        std::fs::write(f, "\u{feff}{\"a\": 1}").unwrap();
        assert!(install(&files).is_empty());
        assert!(std::fs::read_to_string(f).unwrap().starts_with("{\"window.title\""));
        assert_eq!(std::fs::read_to_string(f.with_extension("json.bak")).unwrap(), "\u{feff}{\"a\": 1}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
