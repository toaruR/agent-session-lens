//! Tauri commands (thin wrappers over `lens_core::ipc`).

use crate::state::{lock, Shared};
use lens_core::ipc::{self, AggregatorView, RealPathProbe};
use lens_core::model::v5::{Detail, IpcError, ProjectActivity, SourceHealth};
use lens_core::model::v5::{Origin, OriginKind, SourceState, UnifiedSession};
use lens_core::model::{Agent, SessionKind};
use lens_core::tokens::{aggregate_tree, find_session_log, ChildLog, TokenReport, TokenTreeNode};
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::Manager;

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

/// Full log reads and cache writes run off the UI and ingestion threads.
#[tauri::command]
pub async fn get_session_tokens(app: tauri::AppHandle, session_uid: String) -> Result<TokenReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Shared>();
        let session = lock(&state.agg).session(&session_uid)
            .ok_or("セッションが見つかりません")?;
        let mut targets = HashMap::from([(session_uid.clone(), TokenTarget::from(session))]);
        aggregate_tree(&session_uid, |uid| {
            let target = targets[uid].clone();
            let mut children = Vec::new();
            // Resolve known children without restricting them to the selected project.
            let known = lock(&state.agg).session(uid).map(|s| s.hierarchy.child_uids).unwrap_or_default();
            for child_uid in known {
                if let Some(child) = lock(&state.agg).session(&child_uid).filter(|s| !matches!(s.hierarchy.kind, SessionKind::Fork | SessionKind::Automated)) {
                    children.push(child_uid.clone());
                    targets.entry(child_uid).or_insert_with(|| TokenTarget::from(child));
                }
            }
            let record = token_path(&state, &target).and_then(|path| {
                lock(&state.tokens).inspect(uid, &target.native_id, target.agent, &path)
                    .map_err(|_| "トークン数の計算・保存に失敗しました。再試行してください".to_string())
            });
            match record {
                Ok(record) => {
                    for child in record.children {
                        let child = child_target(&state, &target, child);
                        children.push(child.uid.clone());
                        targets.entry(child.uid.clone()).or_insert(child);
                    }
                    Ok(TokenTreeNode { usage: record.usage, children, failed: false })
                }
                Err(e) if uid == session_uid => Err(e),
                Err(_) => Ok(TokenTreeNode { usage: None, children, failed: true }),
            }
        })
    }).await.map_err(|_| "トークン計算を完了できませんでした".to_string())?
}

#[derive(Clone)]
struct TokenTarget {
    uid: String,
    native_id: String,
    agent: Agent,
    source_id: String,
    origin: Origin,
    path: Option<PathBuf>,
}

impl From<UnifiedSession> for TokenTarget {
    fn from(s: UnifiedSession) -> Self {
        Self { uid: s.session_uid, native_id: s.native_id, agent: s.agent, source_id: s.source_id, origin: s.origin, path: None }
    }
}

fn token_path(state: &Shared, target: &TokenTarget) -> Result<PathBuf, String> {
    if target.origin.kind == OriginKind::Wsl && !lock(&state.agg).origin_available(&target.origin) {
        return Err("WSL のログ収集元を利用できません".into());
    }
    if let Some(path) = target.path.clone().or_else(|| lock(&state.session_paths).get(&target.uid).cloned()) {
        return Ok(path);
    }
    let source = lock(&state.health).iter().find(|h| h.source_id == target.source_id).cloned()
        .ok_or("ログの収集元が見つかりません")?;
    if source.state != SourceState::Ok { return Err("ログの収集元を利用できません".into()); }
    let root = if target.origin.kind == OriginKind::Wsl {
        let distro = target.origin.distro.as_deref().ok_or("WSL 名が不明です")?;
        PathBuf::from(format!(r"\\wsl.localhost\{}{}", distro, source.root_path.replace('/', r"\")))
    } else { PathBuf::from(source.root_path) };
    find_session_log(&root, target.agent, &target.native_id).map_err(|_| "セッションのログが見つかりません".into())
}

fn child_target(state: &Shared, parent: &TokenTarget, child: ChildLog) -> TokenTarget {
    let mut target = TokenTarget { uid: lens_core::model::v5::session_uid(&parent.source_id, &child.native_id),
        native_id: child.native_id, agent: parent.agent, source_id: parent.source_id.clone(), origin: parent.origin.clone(), path: child.path };
    // AGY IDE and CLI subagents can write into different conversations roots.
    if target.agent == Agent::Antigravity && target.origin.kind == OriginKind::Windows {
        let mut sources = lock(&state.health).clone();
        sources.sort_by_key(|source| source.source_id != parent.source_id);
        for source in sources.iter().filter(|h| h.agent == Some(target.agent) && h.origin == target.origin && h.state == SourceState::Ok) {
            let path = PathBuf::from(&source.root_path).join(format!("{}.db", target.native_id));
            if path.is_file() {
                target.uid = lens_core::model::v5::session_uid(&source.source_id, &target.native_id);
                target.source_id = source.source_id.clone();
                target.path = Some(path);
                break;
            }
        }
    }
    target
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

