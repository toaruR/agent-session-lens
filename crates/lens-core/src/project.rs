//! Project Resolver (architecture-v5 sections 6 and 8). The comparison key is kept
//! separate from the display string. Git is never started: `.git` is looked up on disk.

use crate::consts::RESOLVE_MAX_DEPTH;
use crate::model::{Attribution, ProjectRef};
use percent_encoding::percent_decode_str;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct ProjectResolver {
    cache: HashMap<(String, Option<String>), Option<ProjectRef>>,
}

/// A normalized cwd: comparison `key` and human `display`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Normalized {
    pub key: String,
    pub display: String,
    /// Name shown for the project (last path segment).
    pub name: String,
    /// `Some(distro)` when the path lives inside WSL (no on-disk git search possible).
    pub wsl: Option<String>,
}

fn looks_like_windows(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic()
}

fn name_of(display: &str) -> String {
    display.rsplit(['\\', '/']).find(|s| !s.is_empty()).unwrap_or(display).to_string()
}

fn windows_norm(path: &str) -> Normalized {
    let mut s = path.replace('/', "\\");
    let mut chars: Vec<char> = s.chars().collect();
    chars[0] = chars[0].to_ascii_uppercase();
    s = chars.into_iter().collect();
    while s.len() > 3 && s.ends_with('\\') {
        s.pop();
    }
    if s.len() == 2 {
        s.push('\\');
    }
    // uppercase drive letter, `\` separators, no trailing separator, THEN lowercase.
    Normalized { key: s.to_lowercase(), name: name_of(&s), display: s, wsl: None }
}

fn wsl_norm(distro: &str, path: &str) -> Normalized {
    let mut p = path.replace('\\', "/");
    while p.len() > 1 && p.ends_with('/') {
        p.pop();
    }
    if !p.starts_with('/') {
        p.insert(0, '/');
    }
    let name = if p == "/" { "/".to_string() } else { name_of(&p) };
    Normalized {
        key: format!("wsl:{distro}:{p}"),
        display: format!("{name} (WSL: {distro})"),
        name,
        wsl: Some(distro.to_string()),
    }
}

/// `\\wsl.localhost\<distro>\rest` / `\\wsl$\<distro>\rest` -> (distro, "/rest").
fn parse_wsl_unc(s: &str) -> Option<(String, String)> {
    let t = s.replace('/', "\\");
    let rest = t.strip_prefix(r"\\")?;
    let mut parts = rest.splitn(3, '\\');
    let host = parts.next()?.to_ascii_lowercase();
    if host != "wsl.localhost" && host != "wsl$" {
        return None;
    }
    let distro = parts.next().filter(|d| !d.is_empty())?;
    let tail = parts.next().unwrap_or("");
    Some((distro.to_string(), format!("/{tail}")))
}

/// `/mnt/<drive>[/rest]` -> `X:/rest`.
fn parse_mnt(s: &str) -> Option<String> {
    let rest = s.strip_prefix("/mnt/")?;
    let b = rest.as_bytes();
    if b.is_empty() || !b[0].is_ascii_alphabetic() || (b.len() > 1 && b[1] != b'/') {
        return None;
    }
    let tail = &rest[1..];
    Some(format!("{}:{}", b[0] as char, if tail.is_empty() { "/" } else { tail }))
}

/// Normalize a raw cwd (path or `file://` URI). `distro` is the WSL distro of the
/// source that produced the cwd, if any.
pub fn normalize(raw: &str, distro: Option<&str>) -> Option<Normalized> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let mut s = if let Some(rest) = raw.strip_prefix("file://") {
        let decoded = percent_decode_str(rest).decode_utf8().ok()?.into_owned();
        let t = decoded.strip_prefix('/').unwrap_or(&decoded);
        if looks_like_windows(t) { t.to_string() } else { decoded }
    } else {
        raw.to_string()
    };
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        s = rest.to_string();
    }
    if s.is_empty() {
        return None;
    }
    if let Some((d, p)) = parse_wsl_unc(&s) {
        return Some(wsl_norm(&d, &p));
    }
    if looks_like_windows(&s) {
        return Some(windows_norm(&s));
    }
    if let Some(w) = parse_mnt(&s) {
        return Some(windows_norm(&w));
    }
    if s.starts_with(r"\\") || s.starts_with("//") {
        let d = s.replace('/', "\\");
        return Some(Normalized { key: d.to_lowercase(), name: name_of(&d), display: d, wsl: None });
    }
    if s.starts_with('/') {
        if let Some(d) = distro {
            return Some(wsl_norm(d, &s));
        }
        while s.len() > 1 && s.ends_with('/') {
            s.pop();
        }
        return Some(Normalized { key: s.clone(), name: name_of(&s), display: s, wsl: None });
    }
    // Relative or otherwise odd path: keep as is.
    while s.len() > 1 && (s.ends_with('/') || s.ends_with('\\')) {
        s.pop();
    }
    let key = if cfg!(windows) { s.to_lowercase() } else { s.clone() };
    Some(Normalized { key, name: name_of(&s), display: s, wsl: None })
}

/// Display string for a raw cwd (no WSL distro context).
pub fn display_path(raw: &str) -> Option<String> {
    normalize(raw, None).map(|n| n.display)
}

/// Search `.git` (directory or file) in `start` and up to RESOLVE_MAX_DEPTH ancestors.
fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut cur = Some(start);
    for _ in 0..=RESOLVE_MAX_DEPTH {
        let dir = cur?;
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}

impl ProjectResolver {
    pub fn resolve(&mut self, raw_cwd: &str) -> Option<ProjectRef> {
        self.resolve_in(raw_cwd, None)
    }

    /// `distro`: WSL distro of the producing source (for Linux-style cwd).
    pub fn resolve_in(&mut self, raw_cwd: &str, distro: Option<&str>) -> Option<ProjectRef> {
        let n = normalize(raw_cwd, distro)?;
        let ck = (n.key.clone(), distro.map(str::to_string));
        if let Some(hit) = self.cache.get(&ck) {
            return hit.clone();
        }
        let git = if n.wsl.is_some() || n.display.starts_with(r"\\") {
            None
        } else {
            find_git_root(Path::new(&n.display))
        };
        let project = match git.and_then(|r| normalize(&r.to_string_lossy(), None)) {
            Some(g) => ProjectRef { key: g.key, name: g.name, display_path: g.display, attribution: Attribution::GitRoot },
            None => ProjectRef { key: n.key.clone(), name: n.name, display_path: n.display, attribution: Attribution::RawPath },
        };
        self.cache.insert(ck, Some(project.clone()));
        Some(project)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_file_uri() {
        assert_eq!(display_path("file:///d%3A/work/My%20Repo/").as_deref(), Some(r"D:\work\My Repo"));
        assert_eq!(display_path(r"\\?\C:\x\y\").as_deref(), Some(r"C:\x\y"));
        assert_eq!(display_path("c:/a/b").as_deref(), Some(r"C:\a\b"));
        assert_eq!(display_path("  "), None);
    }

    #[test]
    fn subdirectory_resolves_to_git_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("src").join("deep")).unwrap();
        let mut r = ProjectResolver::default();
        let p = r.resolve(&root.join("src").join("deep").to_string_lossy()).unwrap();
        assert_eq!(p.attribution, Attribution::GitRoot);
        assert_eq!(p.name, "repo");
        let q = r.resolve(&root.to_string_lossy()).unwrap();
        assert_eq!(p.key, q.key);
    }

    #[test]
    fn worktree_git_file_counts_as_root() {
        let tmp = tempfile::tempdir().unwrap();
        let wt = tmp.path().join("wt");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join(".git"), "gitdir: elsewhere").unwrap();
        let p = ProjectResolver::default().resolve(&wt.to_string_lossy()).unwrap();
        assert_eq!(p.attribution, Attribution::GitRoot);
    }

    #[test]
    fn missing_or_untracked_dir_is_raw_path() {
        let tmp = tempfile::tempdir().unwrap();
        let gone = tmp.path().join("deleted").join("dir");
        let p = ProjectResolver::default().resolve(&gone.to_string_lossy()).unwrap();
        assert_eq!(p.attribution, Attribution::RawPath);
        assert_eq!(p.name, "dir");
    }
}
