// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Settings sync service — mirrors claude-code-typescript-src`services/settingsSync/`.
// Synchronizes agent settings across devices and sessions.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsSnapshot {
    pub version: u64,
    pub timestamp_ms: u64,
    pub device_id: String,
    pub settings: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictStrategy {
    LastWriterWins,
    LocalWins,
    RemoteWins,
    Manual,
}

#[derive(Clone)]
pub struct SettingsSyncService {
    inner: Arc<RwLock<SyncInner>>,
}

struct SyncInner {
    local_version: u64,
    remote_version: u64,
    pending_changes: HashMap<String, serde_json::Value>,
    conflict_strategy: ConflictStrategy,
    sync_file: PathBuf,
    enabled: bool,
}

impl SettingsSyncService {
    pub fn new(sync_file: PathBuf, conflict_strategy: ConflictStrategy) -> Self {
        Self {
            inner: Arc::new(RwLock::new(SyncInner {
                local_version: 0,
                remote_version: 0,
                pending_changes: HashMap::new(),
                conflict_strategy,
                sync_file,
                enabled: true,
            })),
        }
    }

    pub async fn on_local_change(&self, key: String, value: serde_json::Value) {
        let mut inner = self.inner.write().await;
        if inner.enabled {
            inner.local_version += 1;
            inner.pending_changes.insert(key, value);
        }
    }

    pub async fn has_pending_changes(&self) -> bool {
        let inner = self.inner.read().await;
        !inner.pending_changes.is_empty()
    }

    pub async fn export_snapshot(&self, device_id: &str) -> SettingsSnapshot {
        let inner = self.inner.read().await;
        SettingsSnapshot {
            version: inner.local_version,
            timestamp_ms: now_ms(),
            device_id: device_id.to_string(),
            settings: inner.pending_changes.clone(),
        }
    }

    pub async fn import_snapshot(&self, snapshot: SettingsSnapshot) -> Vec<String> {
        let mut inner = self.inner.write().await;
        let mut applied_keys = Vec::new();

        for (key, remote_value) in &snapshot.settings {
            let should_apply = match inner.conflict_strategy {
                ConflictStrategy::LastWriterWins => snapshot.version > inner.remote_version,
                ConflictStrategy::RemoteWins => true,
                ConflictStrategy::LocalWins => !inner.pending_changes.contains_key(key),
                ConflictStrategy::Manual => false,
            };
            if should_apply {
                inner
                    .pending_changes
                    .insert(key.clone(), remote_value.clone());
                applied_keys.push(key.clone());
            }
        }

        if snapshot.version > inner.remote_version {
            inner.remote_version = snapshot.version;
        }

        applied_keys
    }

    pub async fn mark_synced(&self) {
        let mut inner = self.inner.write().await;
        inner.pending_changes.clear();
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
