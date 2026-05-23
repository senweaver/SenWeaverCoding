// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteSession {
    pub session_id: String,
    pub url: String,
    pub status: RemoteSessionStatus,
    pub created_at_ms: u64,
    pub last_activity_ms: u64,
    pub auth_token: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteSessionStatus {
    Connecting,
    Connected,
    Disconnected,
    Error,
}

#[derive(Clone)]
pub struct RemoteSessionManager {
    inner: Arc<RwLock<HashMap<String, RemoteSession>>>,
}

impl RemoteSessionManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn add_session(&self, session: RemoteSession) {
        let mut inner = self.inner.write().await;
        inner.insert(session.session_id.clone(), session);
    }

    pub async fn get_session(&self, session_id: &str) -> Option<RemoteSession> {
        let inner = self.inner.read().await;
        inner.get(session_id).cloned()
    }

    pub async fn set_status(&self, session_id: &str, status: RemoteSessionStatus) {
        let mut inner = self.inner.write().await;
        if let Some(session) = inner.get_mut(session_id) {
            session.status = status;
            session.last_activity_ms = now_ms();
        }
    }

    pub async fn remove_session(&self, session_id: &str) -> Option<RemoteSession> {
        let mut inner = self.inner.write().await;
        inner.remove(session_id)
    }

    pub async fn list_sessions(&self) -> Vec<RemoteSession> {
        let inner = self.inner.read().await;
        inner.values().cloned().collect()
    }

    pub async fn active_sessions(&self) -> Vec<RemoteSession> {
        let inner = self.inner.read().await;
        inner
            .values()
            .filter(|s| s.status == RemoteSessionStatus::Connected)
            .cloned()
            .collect()
    }
}

impl Default for RemoteSessionManager {
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
