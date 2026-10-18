//! settings.json persistence (architecture-v5 §6.1). Single file, temp file + rename.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::consts::HELPER_PATH;
use crate::model::Diagnostic;

pub const SETTINGS_FILE: &str = "settings.json";
pub const SETTINGS_BAD_FILE: &str = "settings.json.bad";
const SETTINGS_TMP_FILE: &str = "settings.json.tmp";

pub const SETTINGS_LOAD_FAILED: &str = "settings_load_failed";
pub const SETTINGS_SAVE_FAILED: &str = "settings_save_failed";

/// File-system operations used by the settings store (injectable for tests).
pub trait SettingsFs {
    /// `Ok(None)` when the file does not exist.
    fn read_to_string(&self, path: &Path) -> io::Result<Option<String>>;
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    fn write(&self, path: &Path, data: &[u8]) -> io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RealFs;

impl SettingsFs for RealFs {
    fn read_to_string(&self, path: &Path) -> io::Result<Option<String>> {
        match std::fs::read(path) {
            Ok(b) => Ok(Some(
                String::from_utf8(b).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?,
            )),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }
    fn write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        std::fs::write(path, data)
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Settings {
    pub helper_path: Option<String>,
    pub disabled_distros: Vec<String>,
    pub source_roots: BTreeMap<String, String>,
}

impl Settings {
    /// Effective helper path: override or HELPER_PATH.
    pub fn effective_helper_path(&self) -> &str {
        self.helper_path.as_deref().unwrap_or(HELPER_PATH)
    }

    pub fn to_json(&self) -> String {
        let mut out = String::from("{\"version\":1");
        if let Some(h) = &self.helper_path {
            out.push_str(&format!(",\"helperPath\":{}", json!(h)));
        }
        if !self.disabled_distros.is_empty() {
            out.push_str(&format!(",\"disabledDistros\":{}", json!(self.disabled_distros)));
        }
        if !self.source_roots.is_empty() {
            out.push_str(&format!(",\"sourceRoots\":{}", json!(self.source_roots)));
        }
        out.push('}');
        out
    }

    /// Parse and validate. `Err` = whole file invalid. `Ok((settings, helper_ignored))`.
    fn parse(text: &str) -> Result<(Settings, bool), ()> {
        let v: Value = serde_json::from_str(text).map_err(|_| ())?;
        let o = v.as_object().ok_or(())?;
        if o.get("version").and_then(Value::as_u64) != Some(1) {
            return Err(());
        }
        let mut s = Settings::default();
        let mut helper_ignored = false;
        if let Some(h) = o.get("helperPath") {
            match h.as_str() {
                Some(p) if p.starts_with('/') => s.helper_path = Some(p.to_string()),
                _ => helper_ignored = true,
            }
        }
        if let Some(d) = o.get("disabledDistros") {
            for item in d.as_array().ok_or(())? {
                let n = item.as_str().filter(|n| !n.is_empty()).ok_or(())?;
                if s.disabled_distros.iter().any(|x| x == n) {
                    return Err(());
                }
                s.disabled_distros.push(n.to_string());
            }
        }
        if let Some(r) = o.get("sourceRoots") {
            for (k, p) in r.as_object().ok_or(())? {
                let p = p.as_str().filter(|p| !p.is_empty()).ok_or(())?;
                s.source_roots.insert(k.clone(), p.to_string());
            }
        }
        Ok((s, helper_ignored))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsError {
    pub code: &'static str,
}

pub struct SettingsStore<F: SettingsFs = RealFs> {
    fs: F,
    dir: PathBuf,
    settings: Settings,
    diagnostics: Vec<Diagnostic>,
}

impl<F: SettingsFs> SettingsStore<F> {
    /// Load from `<dir>/settings.json`. Never fails: falls back to defaults with diagnostics.
    pub fn load(fs: F, dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        let path = dir.join(SETTINGS_FILE);
        let mut failed = 0u64;
        let mut settings = Settings::default();
        match fs.read_to_string(&path) {
            Ok(None) => {}
            Ok(Some(text)) => match Settings::parse(&text) {
                Ok((s, helper_ignored)) => {
                    settings = s;
                    if helper_ignored {
                        failed += 1;
                    }
                }
                Err(()) => {
                    failed += 1;
                    let _ = fs.rename(&path, &dir.join(SETTINGS_BAD_FILE));
                }
            },
            Err(_) => {
                failed += 1;
                let _ = fs.rename(&path, &dir.join(SETTINGS_BAD_FILE));
            }
        }
        let diagnostics = if failed > 0 {
            vec![Diagnostic { code: SETTINGS_LOAD_FAILED.into(), count: failed }]
        } else {
            Vec::new()
        };
        Self { fs, dir, settings, diagnostics }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(SETTINGS_FILE)
    }

    pub fn set_distro_enabled(&mut self, distro: &str, enabled: bool) -> Result<(), SettingsError> {
        let mut next = self.settings.clone();
        next.disabled_distros.retain(|d| d != distro);
        if !enabled {
            next.disabled_distros.push(distro.to_string());
        }
        self.commit(next)
    }

    /// `path = None` removes the entry.
    pub fn set_source_root(&mut self, source_id: &str, path: Option<&str>) -> Result<(), SettingsError> {
        let mut next = self.settings.clone();
        match path {
            Some(p) => {
                next.source_roots.insert(source_id.to_string(), p.to_string());
            }
            None => {
                next.source_roots.remove(source_id);
            }
        }
        self.commit(next)
    }

    fn commit(&mut self, next: Settings) -> Result<(), SettingsError> {
        self.save(&next).map_err(|_| SettingsError { code: SETTINGS_SAVE_FAILED })?;
        self.settings = next;
        Ok(())
    }

    fn save(&self, s: &Settings) -> io::Result<()> {
        self.fs.create_dir_all(&self.dir)?;
        let tmp = self.dir.join(SETTINGS_TMP_FILE);
        self.fs.write(&tmp, s.to_json().as_bytes())?;
        self.fs.rename(&tmp, &self.path())
    }
}
