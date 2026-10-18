//! State shared between the IPC commands, the ingest thread and `main`.

use lens_core::aggregator::Aggregator;
use lens_core::ipc::Phase;
use lens_core::model::v5::SourceHealth;
use lens_core::settings::{RealFs, SettingsStore};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

/// State shared between the IPC commands and the ingest thread.
pub struct Shared {
    pub agg: Mutex<Aggregator>,
    pub phase: Mutex<Phase>,
    pub health: Mutex<Vec<SourceHealth>>,
    pub store: Mutex<SettingsStore<RealFs>>,
    pub known_distros: Mutex<Vec<String>>,
    pub known_sources: Mutex<Vec<String>>,
}

pub struct Worker {
    pub stop: Arc<AtomicBool>,
    pub handle: Mutex<Option<JoinHandle<()>>>,
}

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn settings_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").filter(|v| !v.is_empty()).map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_default();
    base.join("agent-session-lens")
}

