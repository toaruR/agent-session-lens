//! Tauri commands (thin wrappers over `lens_core::ipc`).

use crate::state::{lock, Shared};
use lens_core::ipc::{self, AggregatorView, RealPathProbe};
use lens_core::model::v5::{Detail, IpcError, ProjectActivity, SourceHealth};

// ---- commands (thin wrappers over lens_core::ipc) ----

#[tauri::command]
pub fn list_projects(state: tauri::State<'_, Shared>) -> Result<Vec<ProjectActivity>, IpcError> {
    let phase = *lock(&state.phase);
    let health = lock(&state.health).clone();
    let agg = lock(&state.agg);
    ipc::list_projects(&AggregatorView { aggregator: &agg, phase, health })
}

#[tauri::command]
pub fn get_project_detail(state: tauri::State<'_, Shared>, key: String) -> Result<Detail, IpcError> {
    let phase = *lock(&state.phase);
    let agg = lock(&state.agg);
    ipc::get_project_detail(&AggregatorView { aggregator: &agg, phase, health: Vec::new() }, &key)
}

#[tauri::command]
pub fn get_health(state: tauri::State<'_, Shared>) -> Result<Vec<SourceHealth>, IpcError> {
    let phase = *lock(&state.phase);
    let health = lock(&state.health).clone();
    let agg = lock(&state.agg);
    ipc::get_health(&AggregatorView { aggregator: &agg, phase, health })
}

#[tauri::command]
pub fn set_distro_enabled(state: tauri::State<'_, Shared>, distro: String, enabled: bool) -> Result<serde_json::Value, IpcError> {
    let known = lock(&state.known_distros).clone();
    ipc::set_distro_enabled(&mut lock(&state.store), &known, &distro, enabled)?;
    Ok(serde_json::json!({ "ok": true }))
}

#[tauri::command]
pub fn set_source_root(
    state: tauri::State<'_, Shared>,
    source_id: String,
    path: Option<String>,
) -> Result<serde_json::Value, IpcError> {
    let known = lock(&state.known_sources).clone();
    ipc::set_source_root(&mut lock(&state.store), &known, &RealPathProbe, &source_id, path.as_deref())?;
    Ok(serde_json::json!({ "ok": true }))
}

