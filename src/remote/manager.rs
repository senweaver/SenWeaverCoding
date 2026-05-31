// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use serde::{Deserialize, Serialize};

use super::websocket::SessionWebSocket;

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

struct RemoteSessionEntry {
    meta: RemoteSession,
    socket: Arc<SessionWebSocket>,
}

#[derive(Clone)]
pub struct RemoteSessionManager {
    inner: Arc<RwLock<HashMap<String, RemoteSessionEntry>>>,
}

impl RemoteSessionManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register_session(&self, session: RemoteSession) -> Arc<SessionWebSocket> {
        let socket = Arc::new(SessionWebSocket::new(&session.url));
        let entry = RemoteSessionEntry {
            meta: session.clone(),
            socket: Arc::clone(&socket),
        };
        let mut inner = self.inner.write().await;
        inner.insert(session.session_id.clone(), entry);
        socket
    }

    pub async fn add_session(&self, session: RemoteSession) {
        let _ = self.register_session(session).await;
    }

    pub async fn connect_session(&self, session_id: &str) -> anyhow::Result<()> {
        let socket = {
            let inner = self.inner.read().await;
            inner
                .get(session_id)
                .map(|entry| Arc::clone(&entry.socket))
        };
        let Some(socket) = socket else {
            anyhow::bail!("remote session not found: {session_id}");
        };
        self.set_status(session_id, RemoteSessionStatus::Connecting)
            .await;
        match socket.connect().await {
            Ok(()) => {
                self.set_status(session_id, RemoteSessionStatus::Connected)
                    .await;
                Ok(())
            }
            Err(e) => {
                self.set_status(session_id, RemoteSessionStatus::Error)
                    .await;
                Err(e)
            }
        }
    }

    pub async fn disconnect_session(&self, session_id: &str) -> anyhow::Result<()> {
        let socket = {
            let inner = self.inner.read().await;
            inner
                .get(session_id)
                .map(|entry| Arc::clone(&entry.socket))
        };
        let Some(socket) = socket else {
            anyhow::bail!("remote session not found: {session_id}");
        };
        socket.disconnect().await;
        self.set_status(session_id, RemoteSessionStatus::Disconnected)
            .await;
        Ok(())
    }

    pub async fn send_message(
        &self,
        session_id: &str,
        message: super::websocket::WsMessage,
    ) -> anyhow::Result<()> {
        let socket = {
            let inner = self.inner.read().await;
            inner
                .get(session_id)
                .map(|entry| Arc::clone(&entry.socket))
        };
        let Some(socket) = socket else {
            anyhow::bail!("remote session not found: {session_id}");
        };
        let result = socket.send(message).await;
        if result.is_ok() {
            let mut inner = self.inner.write().await;
            if let Some(entry) = inner.get_mut(session_id) {
                entry.meta.last_activity_ms = now_ms();
            }
        }
        result
    }

    pub async fn websocket(&self, session_id: &str) -> Option<Arc<SessionWebSocket>> {
        let inner = self.inner.read().await;
        inner
            .get(session_id)
            .map(|entry| Arc::clone(&entry.socket))
    }

    pub async fn get_session(&self, session_id: &str) -> Option<RemoteSession> {
        let inner = self.inner.read().await;
        inner.get(session_id).map(|entry| entry.meta.clone())
    }

    pub async fn set_status(&self, session_id: &str, status: RemoteSessionStatus) {
        let mut inner = self.inner.write().await;
        if let Some(entry) = inner.get_mut(session_id) {
            entry.meta.status = status;
            entry.meta.last_activity_ms = now_ms();
        }
    }

    pub async fn remove_session(&self, session_id: &str) -> Option<RemoteSession> {
        if self.get_session(session_id).await.is_some() {
            let _ = self.disconnect_session(session_id).await;
        }
        let mut inner = self.inner.write().await;
        inner.remove(session_id).map(|entry| entry.meta)
    }

    pub async fn list_sessions(&self) -> Vec<RemoteSession> {
        let inner = self.inner.read().await;
        inner.values().map(|entry| entry.meta.clone()).collect()
    }

    pub async fn active_sessions(&self) -> Vec<RemoteSession> {
        let inner = self.inner.read().await;
        inner
            .values()
            .filter(|entry| entry.meta.status == RemoteSessionStatus::Connected)
            .map(|entry| entry.meta.clone())
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
