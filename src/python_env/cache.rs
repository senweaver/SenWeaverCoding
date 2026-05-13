// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::manager::PythonEnvState;

fn config_root() -> PathBuf {
    if let Ok(custom) = std::env::var("SEN_CONFIG_DIR") {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    let home = directories::UserDirs::new()
        .and_then(|u| Some(u.home_dir().to_path_buf()))
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".senweavercoding")
}

fn cache_path() -> PathBuf {
    config_root().join("python-envs.json")
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersistedCache {
    #[serde(default)]
    pub workspaces: HashMap<String, PythonEnvState>,
}

static CACHE: OnceLock<RwLock<PersistedCache>> = OnceLock::new();

fn cache_lock() -> &'static RwLock<PersistedCache> {
    CACHE.get_or_init(|| RwLock::new(load_from_disk()))
}

fn load_from_disk() -> PersistedCache {
    let path = cache_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return PersistedCache::default();
    };
    serde_json::from_str::<PersistedCache>(&text).unwrap_or_default()
}

fn write_to_disk(cache: &PersistedCache) -> std::io::Result<()> {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(cache)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

fn workspace_key(workspace: &Path) -> String {
    workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf())
        .to_string_lossy()
        .to_string()
}

pub fn load_state(workspace: &Path) -> Option<PythonEnvState> {
    let key = workspace_key(workspace);
    cache_lock().read().workspaces.get(&key).cloned()
}

pub fn store_state(workspace: &Path, state: &PythonEnvState) {
    let key = workspace_key(workspace);
    {
        let mut guard = cache_lock().write();
        guard.workspaces.insert(key, state.clone());
    }
    let snapshot = cache_lock().read().clone();
    if let Err(err) = write_to_disk(&snapshot) {
        tracing::warn!(error = %err, "failed to persist python-envs.json");
    }
}

pub fn forget_state(workspace: &Path) {
    let key = workspace_key(workspace);
    {
        let mut guard = cache_lock().write();
        guard.workspaces.remove(&key);
    }
    let snapshot = cache_lock().read().clone();
    if let Err(err) = write_to_disk(&snapshot) {
        tracing::warn!(error = %err, "failed to persist python-envs.json on forget");
    }
}
