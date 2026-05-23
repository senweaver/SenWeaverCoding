// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
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

#[derive(Clone)]
pub struct AutoDreamService {
    inner: Arc<RwLock<AutoDreamInner>>,
}

struct AutoDreamInner {
    tasks: Vec<DreamTask>,
    enabled: bool,
    max_concurrent: u32,
    running_count: u32,
}

impl AutoDreamService {
    pub fn new(enabled: bool) -> Self {
        Self {
            inner: Arc::new(RwLock::new(AutoDreamInner {
                tasks: Vec::new(),
                enabled,
                max_concurrent: 2,
                running_count: 0,
            })),
        }
    }

    pub async fn add_task(&self, task: DreamTask) {
        let mut inner = self.inner.write().await;
        inner.tasks.push(task);
    }

    pub async fn remove_task(&self, id: &str) -> bool {
        let mut inner = self.inner.write().await;
        let before = inner.tasks.len();
        inner.tasks.retain(|t| t.id != id);
        inner.tasks.len() < before
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
