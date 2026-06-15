// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use serde::{Deserialize, Serialize};

use crate::memory::Memory;

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

impl SessionMemoryCategory {
    fn persist_label(self) -> &'static str {
        match self {
            SessionMemoryCategory::UserPreference => "pref",
            SessionMemoryCategory::ProjectContext => "project",
            SessionMemoryCategory::TaskContext => "task",
            SessionMemoryCategory::Decision => "decision",
            SessionMemoryCategory::ErrorPattern => "error",
            SessionMemoryCategory::Custom => "custom",
        }
    }
}

enum LongTermBackend {
    Uninitialized,
    Disabled,
    Ready(Arc<dyn Memory>),
}

enum BackendBuild {
    Ready(Arc<dyn Memory>),
    ServicesUnavailable,
    Failed,
}

#[derive(Clone)]
pub struct SessionMemoryService {
    inner: Arc<RwLock<SessionMemoryInner>>,
    long_term: Arc<Mutex<LongTermBackend>>,
}

const DEFAULT_SESSION_BUCKET: &str = "__no_session__";

fn session_bucket_key(session_id: Option<&str>) -> String {
    match session_id {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => DEFAULT_SESSION_BUCKET.to_string(),
    }
}

fn current_session_id() -> Option<String> {
    crate::session::current_session_context().map(|ctx| ctx.session_id)
}

fn long_term_key(session_id: Option<&str>, category: SessionMemoryCategory, key: &str) -> String {
    match session_id {
        Some(id) if !id.is_empty() => {
            format!("session_mem:{}:{}:{}", id, category.persist_label(), key)
        }
        _ => format!(
            "session_mem:{}:{}:{}",
            DEFAULT_SESSION_BUCKET,
            category.persist_label(),
            key
        ),
    }
}

struct SessionMemoryInner {
    entries: HashMap<String, HashMap<String, SessionMemoryEntry>>,
    enabled: bool,
}

impl SessionMemoryService {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(SessionMemoryInner {
                entries: HashMap::new(),
                enabled: true,
            })),
            long_term: Arc::new(Mutex::new(LongTermBackend::Uninitialized)),
        }
    }

    pub async fn store(&self, key: &str, value: &str, category: SessionMemoryCategory) {
        let session_id = current_session_id();
        let bucket = session_bucket_key(session_id.as_deref());
        {
            let mut inner = self.inner.write().await;
            if !inner.enabled {
                return;
            }
            let now = now_ms();
            let entry = inner
                .entries
                .entry(bucket)
                .or_default()
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
        let this = self.clone();
        let key = key.to_string();
        let value = value.to_string();
        crate::runtime::spawn_supervised("session.memory.persist", async move {
            this.persist_to_long_term(&key, &value, category, session_id)
                .await;
        });
    }

    async fn persist_to_long_term(
        &self,
        key: &str,
        value: &str,
        category: SessionMemoryCategory,
        session_id: Option<String>,
    ) {
        let Some(mem) = self.long_term_backend().await else {
            return;
        };
        let key = long_term_key(session_id.as_deref(), category, key);
        if let Err(err) = mem
            .store(
                &key,
                value,
                crate::memory::MemoryCategory::Conversation,
                session_id.as_deref(),
            )
            .await
        {
            tracing::warn!(
                key = %key,
                error = %err,
                "session memory long-term persistence failed"
            );
        }
    }

    fn forget_from_long_term(
        &self,
        items: Vec<(String, SessionMemoryCategory)>,
        session_id: Option<String>,
    ) {
        if items.is_empty() {
            return;
        }
        let this = self.clone();
        crate::runtime::spawn_supervised("session.memory.forget", async move {
            let Some(mem) = this.long_term_backend().await else {
                return;
            };
            for (key, category) in items {
                let long_term = long_term_key(session_id.as_deref(), category, &key);
                if let Err(err) = mem.forget(&long_term).await {
                    tracing::warn!(
                        key = %long_term,
                        error = %err,
                        "session memory long-term forget failed"
                    );
                }
            }
        });
    }

    async fn long_term_backend(&self) -> Option<Arc<dyn Memory>> {
        let mut guard = self.long_term.lock().await;
        match &*guard {
            LongTermBackend::Ready(mem) => Some(mem.clone()),
            LongTermBackend::Disabled => None,
            LongTermBackend::Uninitialized => match build_long_term_backend().await {
                BackendBuild::Ready(mem) => {
                    *guard = LongTermBackend::Ready(mem.clone());
                    Some(mem)
                }
                BackendBuild::Failed => {
                    *guard = LongTermBackend::Disabled;
                    None
                }
                BackendBuild::ServicesUnavailable => None,
            },
        }
    }

    pub async fn get(&self, key: &str) -> Option<SessionMemoryEntry> {
        let bucket = session_bucket_key(current_session_id().as_deref());
        let inner = self.inner.read().await;
        inner.entries.get(&bucket).and_then(|m| m.get(key).cloned())
    }

    pub async fn list(&self, category: Option<SessionMemoryCategory>) -> Vec<SessionMemoryEntry> {
        let bucket = session_bucket_key(current_session_id().as_deref());
        let inner = self.inner.read().await;
        inner
            .entries
            .get(&bucket)
            .map(|m| {
                m.values()
                    .filter(|e| category.map_or(true, |c| e.category == c))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub async fn remove(&self, key: &str) -> bool {
        let session_id = current_session_id();
        let bucket = session_bucket_key(session_id.as_deref());
        let removed_category = {
            let mut inner = self.inner.write().await;
            inner
                .entries
                .get_mut(&bucket)
                .and_then(|m| m.remove(key))
                .map(|entry| entry.category)
        };
        match removed_category {
            Some(category) => {
                self.forget_from_long_term(vec![(key.to_string(), category)], session_id);
                true
            }
            None => false,
        }
    }

    pub async fn build_memory_prompt(&self, max_tokens_estimate: usize) -> String {
        let bucket = session_bucket_key(current_session_id().as_deref());
        let inner = self.inner.read().await;
        let Some(entries) = inner.entries.get(&bucket) else {
            return String::new();
        };
        if entries.is_empty() {
            return String::new();
        }
        let mut parts = Vec::new();
        let mut total_len = 0;
        for entry in entries.values() {
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
        let session_id = current_session_id();
        let bucket = session_bucket_key(session_id.as_deref());
        let removed = {
            let mut inner = self.inner.write().await;
            inner.entries.remove(&bucket)
        };
        if let Some(entries) = removed {
            let items: Vec<(String, SessionMemoryCategory)> = entries
                .into_values()
                .map(|entry| (entry.key, entry.category))
                .collect();
            self.forget_from_long_term(items, session_id);
        }
    }

    pub async fn clear_session(&self, session_id: &str) {
        let bucket = session_bucket_key(Some(session_id));
        let mut inner = self.inner.write().await;
        inner.entries.remove(&bucket);
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

async fn build_long_term_backend() -> BackendBuild {
    let Some(services) = crate::services::try_get_services() else {
        return BackendBuild::ServicesUnavailable;
    };
    let config = services.config();
    match crate::memory::create_memory_with_storage_and_routes_async(
        config.memory.clone(),
        config.embedding_routes.clone(),
        Some(config.storage.provider.config.clone()),
        config.workspace_dir.clone(),
        config.api_key.clone(),
    )
    .await
    {
        Ok(mem) => BackendBuild::Ready(Arc::from(mem)),
        Err(err) => {
            tracing::warn!(
                error = %err,
                "session memory long-term backend initialization failed; persistence disabled"
            );
            BackendBuild::Failed
        }
    }
}
