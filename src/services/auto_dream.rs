// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamTask {
    pub id: String,
    pub prompt: String,
    pub priority: DreamPriority,
    pub trigger: DreamTrigger,
    pub max_duration_ms: u64,
    pub allowed_tools: Vec<String>,
    pub created_at_ms: u64,
    pub last_run_ms: Option<u64>,
    pub run_count: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DreamPriority {
    Low,
    Normal,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DreamTrigger {
    Idle { after_idle_ms: u64 },
    Interval { every_ms: u64 },
    Once { at_ms: u64 },
    OnSessionEnd,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoDreamState {
    pub enabled: bool,
    pub max_concurrent: u32,
    pub tasks: Vec<DreamTask>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DreamTaskInput {
    pub prompt: String,
    #[serde(default)]
    pub priority: Option<DreamPriority>,
    pub trigger: DreamTrigger,
    #[serde(default)]
    pub max_duration_ms: Option<u64>,
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

const DEFAULT_DREAM_DURATION_MS: u64 = 120_000;

#[derive(Clone)]
pub struct AutoDreamService {
    inner: Arc<RwLock<AutoDreamInner>>,
}

struct AutoDreamInner {
    tasks: Vec<DreamTask>,
    enabled: bool,
    max_concurrent: u32,
    running_count: u32,
    persist_path: Option<PathBuf>,
}

impl AutoDreamService {
    pub fn new(enabled: bool) -> Self {
        Self {
            inner: Arc::new(RwLock::new(AutoDreamInner {
                tasks: Vec::new(),
                enabled,
                max_concurrent: 2,
                running_count: 0,
                persist_path: None,
            })),
        }
    }

    pub async fn bind_persistence(&self, path: PathBuf) {
        let loaded = tokio::fs::read(&path).await.ok().and_then(|bytes| {
            serde_json::from_slice::<AutoDreamState>(&bytes).ok()
        });
        let mut inner = self.inner.write().await;
        inner.persist_path = Some(path);
        if let Some(state) = loaded {
            inner.enabled = state.enabled;
            inner.max_concurrent = state.max_concurrent.max(1);
            inner.tasks = state.tasks;
        }
    }

    async fn persist(&self) {
        let (path, state) = {
            let inner = self.inner.read().await;
            let Some(path) = inner.persist_path.clone() else {
                return;
            };
            (
                path,
                AutoDreamState {
                    enabled: inner.enabled,
                    max_concurrent: inner.max_concurrent,
                    tasks: inner.tasks.clone(),
                },
            )
        };
        match serde_json::to_vec_pretty(&state) {
            Ok(bytes) => {
                if let Err(err) = crate::util::atomic_write_async(&path, bytes).await {
                    tracing::warn!(
                        target: "auto_dream",
                        error = %err,
                        "failed to persist auto_dream state"
                    );
                }
            }
            Err(err) => {
                tracing::warn!(
                    target: "auto_dream",
                    error = %err,
                    "failed to serialize auto_dream state"
                );
            }
        }
    }

    pub async fn is_enabled(&self) -> bool {
        self.inner.read().await.enabled
    }

    pub async fn set_enabled(&self, enabled: bool) {
        {
            let mut inner = self.inner.write().await;
            inner.enabled = enabled;
        }
        self.persist().await;
    }

    pub async fn snapshot_state(&self) -> AutoDreamState {
        let inner = self.inner.read().await;
        AutoDreamState {
            enabled: inner.enabled,
            max_concurrent: inner.max_concurrent,
            tasks: inner.tasks.clone(),
        }
    }

    pub async fn create_task(&self, input: DreamTaskInput) -> DreamTask {
        let task = DreamTask {
            id: format!("dream-{}", uuid::Uuid::new_v4()),
            prompt: input.prompt,
            priority: input.priority.unwrap_or(DreamPriority::Normal),
            trigger: input.trigger,
            max_duration_ms: input.max_duration_ms.unwrap_or(DEFAULT_DREAM_DURATION_MS),
            allowed_tools: input.allowed_tools.unwrap_or_default(),
            created_at_ms: now_ms(),
            last_run_ms: None,
            run_count: 0,
            enabled: input.enabled.unwrap_or(true),
        };
        {
            let mut inner = self.inner.write().await;
            inner.tasks.push(task.clone());
        }
        self.persist().await;
        task
    }

    pub async fn update_task(&self, id: &str, input: DreamTaskInput) -> Option<DreamTask> {
        let updated = {
            let mut inner = self.inner.write().await;
            let Some(task) = inner.tasks.iter_mut().find(|t| t.id == id) else {
                return None;
            };
            task.prompt = input.prompt;
            if let Some(priority) = input.priority {
                task.priority = priority;
            }
            task.trigger = input.trigger;
            if let Some(duration) = input.max_duration_ms {
                task.max_duration_ms = duration;
            }
            if let Some(tools) = input.allowed_tools {
                task.allowed_tools = tools;
            }
            if let Some(enabled) = input.enabled {
                task.enabled = enabled;
            }
            task.clone()
        };
        self.persist().await;
        Some(updated)
    }

    pub async fn add_task(&self, task: DreamTask) {
        {
            let mut inner = self.inner.write().await;
            inner.tasks.push(task);
        }
        self.persist().await;
    }

    pub async fn remove_task(&self, id: &str) -> bool {
        let removed = {
            let mut inner = self.inner.write().await;
            let before = inner.tasks.len();
            inner.tasks.retain(|t| t.id != id);
            inner.tasks.len() < before
        };
        if removed {
            self.persist().await;
        }
        removed
    }

    pub async fn try_begin(&self, id: &str) -> bool {
        let ok = {
            let mut inner = self.inner.write().await;
            if !inner.enabled || inner.running_count >= inner.max_concurrent {
                false
            } else if let Some(task) = inner.tasks.iter_mut().find(|t| t.id == id && t.enabled) {
                task.last_run_ms = Some(now_ms());
                task.run_count += 1;
                inner.running_count += 1;
                true
            } else {
                false
            }
        };
        if ok {
            self.persist().await;
        }
        ok
    }

    pub async fn pending_tasks(&self, now_ms: u64, is_idle: bool) -> Vec<DreamTask> {
        let inner = self.inner.read().await;
        if !inner.enabled || inner.running_count >= inner.max_concurrent {
            return Vec::new();
        }
        inner
            .tasks
            .iter()
            .filter(|t| t.enabled)
            .filter(|t| match &t.trigger {
                DreamTrigger::Idle { after_idle_ms } => {
                    is_idle && {
                        t.last_run_ms
                            .map(|lr| now_ms.saturating_sub(lr) >= *after_idle_ms)
                            .unwrap_or(true)
                    }
                }
                DreamTrigger::Interval { every_ms } => t
                    .last_run_ms
                    .map(|lr| now_ms.saturating_sub(lr) >= *every_ms)
                    .unwrap_or(true),
                DreamTrigger::Once { at_ms } => now_ms >= *at_ms && t.run_count == 0,
                DreamTrigger::OnSessionEnd => false,
            })
            .cloned()
            .collect()
    }

    pub async fn mark_running(&self, id: &str) {
        let mut inner = self.inner.write().await;
        inner.running_count += 1;
        if let Some(t) = inner.tasks.iter_mut().find(|t| t.id == id) {
            t.last_run_ms = Some(now_ms());
            t.run_count += 1;
        }
    }

    pub async fn mark_done(&self, _id: &str) {
        let mut inner = self.inner.write().await;
        inner.running_count = inner.running_count.saturating_sub(1);
    }

    pub async fn session_end_tasks(&self) -> Vec<DreamTask> {
        let inner = self.inner.read().await;
        inner
            .tasks
            .iter()
            .filter(|t| t.enabled && matches!(t.trigger, DreamTrigger::OnSessionEnd))
            .cloned()
            .collect()
    }

    pub async fn list_tasks(&self) -> Vec<DreamTask> {
        let inner = self.inner.read().await;
        inner.tasks.clone()
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
