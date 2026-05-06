// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Team memory sync service — mirrors claude-code-typescript-src`services/teamMemorySync/`.
// Synchronizes shared memories across team members working on the same project.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMemory {
    pub key: String,
    pub content: String,
    pub author: String,
    pub version: u64,
    pub created_ms: u64,
    pub updated_ms: u64,
    pub tags: Vec<String>,
}

#[derive(Clone)]
pub struct TeamMemorySyncService {
    inner: Arc<RwLock<TeamSyncInner>>,
}

struct TeamSyncInner {
    memories: HashMap<String, TeamMemory>,
    local_version: u64,
    remote_version: u64,
    enabled: bool,
}

impl TeamMemorySyncService {
    pub fn new(enabled: bool) -> Self {
        Self {
            inner: Arc::new(RwLock::new(TeamSyncInner {
                memories: HashMap::new(),
                local_version: 0,
                remote_version: 0,
                enabled,
            })),
        }
    }

    pub async fn upsert(&self, key: &str, content: &str, author: &str, tags: Vec<String>) {
        let mut inner = self.inner.write().await;
        let now = now_ms();
        inner.local_version += 1;
        let version = inner.local_version;
        let created_ms = inner.memories.get(key).map(|m| m.created_ms).unwrap_or(now);
        inner.memories.insert(
            key.to_string(),
            TeamMemory {
                key: key.to_string(),
                content: content.to_string(),
                author: author.to_string(),
                version,
                created_ms,
                updated_ms: now,
                tags,
            },
        );
    }

    pub async fn get(&self, key: &str) -> Option<TeamMemory> {
        let inner = self.inner.read().await;
        inner.memories.get(key).cloned()
    }

    pub async fn list(&self) -> Vec<TeamMemory> {
        let inner = self.inner.read().await;
        inner.memories.values().cloned().collect()
    }

    pub async fn merge_remote(&self, remote: Vec<TeamMemory>) -> u32 {
        let mut inner = self.inner.write().await;
        let mut merged = 0u32;
        for mem in remote {
            let should_update = inner
                .memories
                .get(&mem.key)
                .map(|local| mem.version > local.version)
                .unwrap_or(true);
            if should_update {
                inner.memories.insert(mem.key.clone(), mem);
                merged += 1;
            }
        }
        merged
    }

    pub async fn export_since(&self, since_version: u64) -> Vec<TeamMemory> {
        let inner = self.inner.read().await;
        inner
            .memories
            .values()
            .filter(|m| m.version > since_version)
            .cloned()
            .collect()
    }

    pub async fn remove(&self, key: &str) -> bool {
        let mut inner = self.inner.write().await;
        inner.memories.remove(key).is_some()
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
