#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod ingest;
mod open;
mod state;
mod title_settings;
mod wsl;

use commands::{editors_without_title_path, focus_project, get_health, get_project_detail, get_session_tokens, install_title_path, list_projects, reveal_project, set_distro_enabled, set_source_root};
use lens_core::aggregator::Aggregator;
use lens_core::clock::{Clock, SystemClock};
use lens_core::ipc::Phase;
use lens_core::settings::{RealFs, SettingsStore};
use state::{lock, settings_dir, Shared, Worker};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicBool;
use tauri::{Manager, RunEvent};

fn main() {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let shared = Shared {
        agg: Mutex::new(Aggregator::new(clock)),
        phase: Mutex::new(Phase::NotStarted),
        health: Mutex::new(Vec::new()),
        store: Mutex::new(SettingsStore::load(RealFs, settings_dir())),
        known_distros: Mutex::new(Vec::new()),
        known_sources: Mutex::new(Vec::new()),
        session_paths: Mutex::new(Default::default()),
        tokens: Mutex::new(lens_core::tokens::TokenCache::load(settings_dir().join("session-tokens.json"))),
    };
    let stop = Arc::new(AtomicBool::new(false));

    let worker_stop = stop.clone();
    let app = tauri::Builder::default()
        .manage(shared)
        .manage(Worker { stop: stop.clone(), handle: Mutex::new(None) })
        .invoke_handler(tauri::generate_handler![
            list_projects,
            get_project_detail,
            get_health,
            get_session_tokens,
            set_distro_enabled,
            set_source_root,
            reveal_project,
            focus_project,
            editors_without_title_path,
            install_title_path
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let h = std::thread::Builder::new().name("asl-ingest".into()).spawn(move || ingest::ingest(handle, worker_stop))?;
            *lock(&app.state::<Worker>().handle) = Some(h);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build tauri app");

    app.run(|app, event| match event {
        RunEvent::ExitRequested { .. } => {
            app.state::<Worker>().stop.store(true, Ordering::Relaxed);
        }
        RunEvent::Exit => {
            let w = app.state::<Worker>();
            w.stop.store(true, Ordering::Relaxed);
            let h = lock(&w.handle).take();
            if let Some(h) = h {
                let _ = h.join();
            }
        }
        _ => {}
    });
}
