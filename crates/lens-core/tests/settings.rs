use std::io;
use std::path::Path;

use lens_core::consts::HELPER_PATH;
use lens_core::settings::{RealFs, SettingsFs, SettingsStore, SETTINGS_BAD_FILE, SETTINGS_FILE};

struct FailRename;
impl SettingsFs for FailRename {
    fn read_to_string(&self, p: &Path) -> io::Result<Option<String>> {
        RealFs.read_to_string(p)
    }
    fn create_dir_all(&self, p: &Path) -> io::Result<()> {
        RealFs.create_dir_all(p)
    }
    fn write(&self, p: &Path, d: &[u8]) -> io::Result<()> {
        RealFs.write(p, d)
    }
    fn rename(&self, _: &Path, _: &Path) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::PermissionDenied, "injected"))
    }
}

fn load_count(s: &SettingsStore<impl SettingsFs>) -> u64 {
    s.diagnostics().iter().filter(|d| d.code == "settings_load_failed").map(|d| d.count).sum()
}

#[test]
fn f27_persist_exact_json() {
    let t = tempfile::tempdir().unwrap();
    let mut s = SettingsStore::load(RealFs, t.path());
    s.set_distro_enabled("Ubuntu", false).unwrap();
    s.set_source_root("claude-local", Some(r"C:\x")).unwrap();
    let text = std::fs::read_to_string(t.path().join(SETTINGS_FILE)).unwrap();
    assert_eq!(text, r#"{"version":1,"disabledDistros":["Ubuntu"],"sourceRoots":{"claude-local":"C:\\x"}}"#);
    let s2 = SettingsStore::load(RealFs, t.path());
    assert_eq!(s2.settings(), s.settings());
    assert!(s2.diagnostics().is_empty());
}

#[test]
fn f27_null_path_removes_entry() {
    let t = tempfile::tempdir().unwrap();
    let mut s = SettingsStore::load(RealFs, t.path());
    s.set_source_root("claude-local", Some(r"C:\x")).unwrap();
    s.set_source_root("claude-local", None).unwrap();
    assert!(!s.settings().source_roots.contains_key("claude-local"));
    let text = std::fs::read_to_string(t.path().join(SETTINGS_FILE)).unwrap();
    assert!(!text.contains("claude-local"));
}

#[test]
fn f28_invalid_json_defaults_and_bad_file() {
    for content in ["{not json", r#"{"version":2}"#] {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join(SETTINGS_FILE), content).unwrap();
        let s = SettingsStore::load(RealFs, t.path());
        assert_eq!(s.settings(), &Default::default());
        assert!(t.path().join(SETTINGS_BAD_FILE).exists());
        assert!(!t.path().join(SETTINGS_FILE).exists());
        assert_eq!(load_count(&s), 1);
    }
}

#[test]
fn f28_relative_helper_path_ignored() {
    let t = tempfile::tempdir().unwrap();
    std::fs::write(
        t.path().join(SETTINGS_FILE),
        r#"{"version":1,"helperPath":"usr/bin/x","disabledDistros":["A"]}"#,
    )
    .unwrap();
    let s = SettingsStore::load(RealFs, t.path());
    assert_eq!(s.settings().effective_helper_path(), HELPER_PATH);
    assert_eq!(s.settings().disabled_distros, vec!["A".to_string()]);
    assert_eq!(load_count(&s), 1);
}

#[test]
fn f26_save_failure_leaves_memory_unchanged() {
    let t = tempfile::tempdir().unwrap();
    let mut s = SettingsStore::load(FailRename, t.path());
    let e = s.set_distro_enabled("Ubuntu", false).unwrap_err();
    assert_eq!(e.code, "settings_save_failed");
    assert!(s.settings().disabled_distros.is_empty());
}

#[cfg(unix)]
#[test]
fn f26_save_failure_readonly_dir_unix() {
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::tempdir().unwrap();
    std::fs::set_permissions(t.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
    let mut s = SettingsStore::load(RealFs, t.path());
    let r = s.set_distro_enabled("Ubuntu", false);
    std::fs::set_permissions(t.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    if r.is_ok() {
        return; // running as root: permissions not enforced
    }
    assert_eq!(r.unwrap_err().code, "settings_save_failed");
    assert!(s.settings().disabled_distros.is_empty());
}

#[test]
fn missing_file_defaults_no_create() {
    let t = tempfile::tempdir().unwrap();
    let mut s = SettingsStore::load(RealFs, t.path());
    assert_eq!(s.settings(), &Default::default());
    assert!(s.diagnostics().is_empty());
    assert_eq!(std::fs::read_dir(t.path()).unwrap().count(), 0);
    s.set_distro_enabled("Ubuntu", false).unwrap();
    let names: Vec<_> = std::fs::read_dir(t.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, vec![SETTINGS_FILE.to_string()]);
}
