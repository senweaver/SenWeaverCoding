// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Notification context — mirrors claude-code-typescript-src`context/notifications.tsx`.
// Manages in-session notifications displayed to the user.

use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Urgent = 3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationEntry {
    pub id: String,
    pub title: String,
    pub body: String,
    pub priority: NotificationPriority,
    pub source: String,
    pub timestamp_ms: u64,
    pub read: bool,
    pub dismissed: bool,
}

#[derive(Clone)]
pub struct NotificationContext {
    inner: Arc<RwLock<NotificationInner>>,
}

struct NotificationInner {
    entries: VecDeque<NotificationEntry>,
    max_entries: usize,
}

impl NotificationContext {
    pub fn new(max_entries: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(NotificationInner {
                entries: VecDeque::new(),
                max_entries,
            })),
        }
    }

    pub async fn add(&self, title: &str, body: &str, priority: NotificationPriority, source: &str) {
        let mut inner = self.inner.write().await;
        if inner.entries.len() >= inner.max_entries {
            inner.entries.pop_front();
        }
        inner.entries.push_back(NotificationEntry {
            id: uuid::Uuid::new_v4().to_string(),
            title: title.to_string(),
            body: body.to_string(),
            priority,
            source: source.to_string(),
            timestamp_ms: now_ms(),
            read: false,
            dismissed: false,
        });
    }

    pub async fn unread(&self) -> Vec<NotificationEntry> {
        let inner = self.inner.read().await;
        inner
            .entries
            .iter()
            .filter(|e| !e.read && !e.dismissed)
            .cloned()
            .collect()
    }

    pub async fn mark_read(&self, id: &str) {
        let mut inner = self.inner.write().await;
        if let Some(entry) = inner.entries.iter_mut().find(|e| e.id == id) {
            entry.read = true;
        }
    }

    pub async fn dismiss(&self, id: &str) {
        let mut inner = self.inner.write().await;
        if let Some(entry) = inner.entries.iter_mut().find(|e| e.id == id) {
            entry.dismissed = true;
        }
    }

    pub async fn all(&self) -> Vec<NotificationEntry> {
        let inner = self.inner.read().await;
        inner.entries.iter().cloned().collect()
    }

    pub async fn unread_count(&self) -> usize {
        let inner = self.inner.read().await;
        inner
            .entries
            .iter()
            .filter(|e| !e.read && !e.dismissed)
            .count()
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
