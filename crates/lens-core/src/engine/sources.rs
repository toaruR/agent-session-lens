//! Source configuration and default discovery roots.

use crate::model::Agent;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SourceConfig {
    pub id: String,
    pub agent: Agent,
    pub root: PathBuf,
}

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn home_dir() -> Option<PathBuf> {
    env_dir("USERPROFILE").or_else(|| env_dir("HOME"))
}

/// Default sources with environment overrides (architecture-v2 §2).
pub fn default_sources() -> Vec<SourceConfig> {
    let home = home_dir().unwrap_or_default();
    let claude = env_dir("CLAUDE_CONFIG_DIR").unwrap_or_else(|| home.join(".claude"));
    let codex = env_dir("CODEX_HOME").unwrap_or_else(|| home.join(".codex"));
    let agy_env = env_dir("ANTIGRAVITY_HOME").or_else(|| env_dir("AGY_HOME"));

    let mut sources = vec![
        SourceConfig { id: "claude-local".into(), agent: Agent::ClaudeCode, root: claude.join("projects") },
        SourceConfig { id: "codex-local".into(), agent: Agent::Codex, root: codex.join("sessions") },
    ];

    if let Some(agy) = agy_env {
        let root = if agy.ends_with("conversations") { agy } else { agy.join("conversations") };
        sources.push(SourceConfig { id: "agy-local".into(), agent: Agent::Antigravity, root });
    } else {
        let gemini = home.join(".gemini");
        sources.push(SourceConfig {
            id: "agy-local".into(),
            agent: Agent::Antigravity,
            root: gemini.join("antigravity").join("conversations"),
        });
        sources.push(SourceConfig {
            id: "agy-cli".into(),
            agent: Agent::Antigravity,
            root: gemini.join("antigravity-cli").join("conversations"),
        });
        sources.push(SourceConfig {
            id: "agy-ide".into(),
            agent: Agent::Antigravity,
            root: gemini.join("antigravity-ide").join("conversations"),
        });
    }

    sources
}

pub(super) fn matches_source(agent: Agent, path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    match agent {
        Agent::ClaudeCode => name.ends_with(".jsonl"),
        Agent::Codex => name.starts_with("rollout-") && name.ends_with(".jsonl"),
        Agent::Antigravity => name.ends_with(".db"),
    }
}
