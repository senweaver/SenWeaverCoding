// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use parking_lot::RwLock;

pub struct WorkspaceRunRegistry {
    inner: RwLock<HashMap<String, String>>,
}

impl WorkspaceRunRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: RwLock::new(HashMap::new()),
        })
    }

    pub fn current(&self, workspace_key: &str) -> Option<String> {
        self.inner.read().get(workspace_key).cloned()
    }

    pub fn try_acquire(
        self: &Arc<Self>,
        workspace_key: &str,
        session_id: &str,
    ) -> Option<WorkspaceRunGuard> {
        let mut guard = self.inner.write();
        if let Some(holder) = guard.get(workspace_key) {
            if holder == session_id {
                return Some(WorkspaceRunGuard {
                    registry: Arc::clone(self),
                    workspace_key: workspace_key.to_string(),
                    session_id: session_id.to_string(),
                    owned: false,
                });
            }
            return None;
        }
        guard.insert(workspace_key.to_string(), session_id.to_string());
        Some(WorkspaceRunGuard {
            registry: Arc::clone(self),
            workspace_key: workspace_key.to_string(),
            session_id: session_id.to_string(),
            owned: true,
        })
    }
}

pub struct WorkspaceRunGuard {
    registry: Arc<WorkspaceRunRegistry>,
    workspace_key: String,
    session_id: String,
    owned: bool,
}

impl WorkspaceRunGuard {
    pub fn workspace_key(&self) -> &str {
        &self.workspace_key
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }
}

impl Drop for WorkspaceRunGuard {
    fn drop(&mut self) {
        if !self.owned {
            return;
        }
        let mut guard = self.registry.inner.write();
        if let Some(holder) = guard.get(&self.workspace_key) {
            if holder == &self.session_id {
                guard.remove(&self.workspace_key);
            }
        }
    }
}

pub fn workspace_key_from_path(path: &Path, fallback_session_id: &str) -> String {
    let s = path.to_string_lossy().trim().to_string();
    if s.is_empty() {
        return format!("__solo::{fallback_session_id}");
    }
    normalize_workspace_key(&s, fallback_session_id)
}

pub fn normalize_workspace_key(raw: &str, fallback_session_id: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return format!("__solo::{fallback_session_id}");
    }
    let unified = trimmed.replace('\\', "/");
    let no_trailing = unified.trim_end_matches('/').to_string();
    let final_key = if no_trailing.is_empty() {
        unified
    } else {
        no_trailing
    };
    if cfg!(target_os = "windows") {
        final_key.to_lowercase()
    } else {
        final_key
    }
}
