// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMemoryEntry {
    pub key: String,
    pub value: String,
    pub category: SessionMemoryCategory,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub source_turn: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMemoryCategory {

    UserPreference,

    ProjectContext,

    TaskContext,

    Decision,

    ErrorPattern,

    Custom,
}

#[derive(Clone)]
pub struct SessionMemoryService {
    inner: Arc<RwLock<SessionMemoryInner>>,
}

struct SessionMemoryInner {
    entries: HashMap<String, SessionMemoryEntry>,
    enabled: bool,
}

impl SessionMemoryService {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(SessionMemoryInner {
                entries: HashMap::new(),
                enabled: true,
            })),
        }
    }

    pub async fn store(&self, key: &str, value: &str, category: SessionMemoryCategory) {
        let mut inner = self.inner.write().await;
        if !inner.enabled {
            return;
        }
        let now = now_ms();
        let entry = inner
            .entries
            .entry(key.to_string())
            .or_insert_with(|| SessionMemoryEntry {
                key: key.to_string(),
                value: String::new(),
                category,
                created_at_ms: now,
                updated_at_ms: now,
                source_turn: None,
            });
        entry.value = value.to_string();
        entry.updated_at_ms = now;
        entry.category = category;
    }

    pub async fn get(&self, key: &str) -> Option<SessionMemoryEntry> {
        let inner = self.inner.read().await;
        inner.entries.get(key).cloned()
    }

    pub async fn list(&self, category: Option<SessionMemoryCategory>) -> Vec<SessionMemoryEntry> {
        let inner = self.inner.read().await;
        inner
            .entries
            .values()
            .filter(|e| category.map_or(true, |c| e.category == c))
            .cloned()
            .collect()
    }

    pub async fn remove(&self, key: &str) -> bool {
        let mut inner = self.inner.write().await;
        inner.entries.remove(key).is_some()
    }

    pub async fn build_memory_prompt(&self, max_tokens_estimate: usize) -> String {
        let inner = self.inner.read().await;
        if inner.entries.is_empty() {
            return String::new();
        }
        let mut parts = Vec::new();
        let mut total_len = 0;
        for entry in inner.entries.values() {
            let line = format!(
                "- [{}] {}: {}",
                entry.category_label(),
                entry.key,
                entry.value
            );
            total_len += line.len();
            if total_len > max_tokens_estimate * 4 {
                break;
            }
            parts.push(line);
        }
        if parts.is_empty() {
            return String::new();
        }
        format!(
            "<session_memories>\n{}\n</session_memories>",
            parts.join("\n")
        )
    }

    pub async fn clear(&self) {
        let mut inner = self.inner.write().await;
        inner.entries.clear();
    }

    pub async fn set_enabled(&self, enabled: bool) {
        let mut inner = self.inner.write().await;
        inner.enabled = enabled;
    }
}

impl SessionMemoryEntry {
    fn category_label(&self) -> &'static str {
        match self.category {
            SessionMemoryCategory::UserPreference => "pref",
            SessionMemoryCategory::ProjectContext => "project",
            SessionMemoryCategory::TaskContext => "task",
            SessionMemoryCategory::Decision => "decision",
            SessionMemoryCategory::ErrorPattern => "error",
            SessionMemoryCategory::Custom => "custom",
        }
    }
}

impl Default for SessionMemoryService {
    fn default() -> Self {
        Self::new()
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
