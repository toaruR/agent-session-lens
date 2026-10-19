//! Tauri commands (thin wrappers over `lens_core::ipc`).

use crate::open;
use crate::title_settings;
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


fn open_target(state: &Shared, key: &str) -> Result<open::Target, String> {
    let detail = lock(&state.agg).project_detail(key).ok_or("プロジェクトが見つかりません")?;
    let p = detail.project;
    if let Some((distro, linux)) = p.key.strip_prefix("wsl:").and_then(|rest| rest.split_once(':')) {
        let path = format!(r"\wsl.localhost\{}{}", distro, linux.replace('/', r"\"));
        return Ok(open::Target { distro: Some(distro.to_string()), key: p.key.clone(), name: p.name, path });
    }
    // The key is lower-cased; prefer the display path, which keeps the original case.
    let path = detail.sessions.iter().flat_map(|s| &s.projects).find(|sp| sp.key == p.key)
        .map(|sp| sp.display_path.clone()).unwrap_or_else(|| p.key.clone());
    Ok(open::Target { key: p.key, name: p.name, distro: None, path })
}

#[tauri::command]
pub fn reveal_project(state: tauri::State<'_, Shared>, key: String) -> Result<(), String> {
    open::reveal_project(&open_target(&state, &key)?)
}

/// Sync on purpose: runs on the UI thread, which holds the foreground right after the click.
#[tauri::command]
pub fn focus_project(state: tauri::State<'_, Shared>, key: String) -> Result<usize, String> {
    open::focus_project(&open_target(&state, &key)?)
}

/// Editors (settings folder names) whose window titles cannot be matched by path.
#[tauri::command]
pub fn editors_without_title_path() -> Vec<&'static str> {
    open::editors_without_marker()
}

/// Adds the path marker to window.title of the given editors. Returns a message per failed file.
#[tauri::command]
pub fn install_title_path(editors: Vec<String>) -> Result<Vec<String>, String> {
    let appdata = std::env::var_os("APPDATA").filter(|v| !v.is_empty()).ok_or("APPDATA が設定されていません")?;
    let mut errors = Vec::new();
    for editor in editors {
        let (_, dir) = title_settings::EDITOR_DIRS.iter().find(|(_, d)| *d == editor).ok_or("未対応のエディタです")?;
        errors.extend(title_settings::install(&title_settings::settings_files(std::path::Path::new(&appdata), dir)));
    }
    Ok(errors)
}
