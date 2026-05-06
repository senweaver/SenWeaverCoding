// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Notifier service — mirrors claude-code-typescript-src`services/notifier.ts`.
// System notification dispatch (desktop, sound, bridge push).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyPriority {
    Low,
    Normal,
    High,
    Urgent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub title: String,
    pub body: String,
    pub priority: NotifyPriority,
    pub sound: bool,
    pub source: String,
}

#[async_trait::async_trait]
pub trait NotifyBackend: Send + Sync {
    fn name(&self) -> &str;
    async fn send(&self, notification: &Notification) -> anyhow::Result<()>;
}

pub struct Notifier {
    backends: Vec<Box<dyn NotifyBackend>>,
    enabled: bool,
}

impl Notifier {
    pub fn new() -> Self {
        Self {
            backends: Vec::new(),
            enabled: true,
        }
    }

    pub fn add_backend(&mut self, backend: Box<dyn NotifyBackend>) {
        self.backends.push(backend);
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub async fn notify(&self, notification: &Notification) {
        if !self.enabled {
            return;
        }
        for backend in &self.backends {
            if let Err(e) = backend.send(notification).await {
                tracing::warn!(backend = backend.name(), error = %e, "notification send failed");
            }
        }
    }

    pub async fn notify_text(&self, title: &str, body: &str) {
        self.notify(&Notification {
            title: title.to_string(),
            body: body.to_string(),
            priority: NotifyPriority::Normal,
            sound: false,
            source: "system".to_string(),
        })
        .await;
    }
}

impl Default for Notifier {
    fn default() -> Self {
        Self::new()
    }
}
