// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Bridge session management — mirrors claude-code-typescript-src`bridge/createSession.ts` and `bridge/sessionRunner.ts`.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::types::BridgeStatus;

#[derive(Debug, Clone)]
pub struct BridgeSession {
    pub session_id: String,
    pub device_id: String,
    pub status: BridgeStatus,
    pub created_at_ms: u64,
    pub last_activity_ms: u64,
}

#[derive(Clone)]
pub struct BridgeSessionManager {
    inner: Arc<RwLock<SessionManagerInner>>,
}

struct SessionManagerInner {
    sessions: HashMap<String, BridgeSession>,
    max_sessions: u32,
}

impl BridgeSessionManager {
    pub fn new(max_sessions: u32) -> Self {
        Self {
            inner: Arc::new(RwLock::new(SessionManagerInner {
                sessions: HashMap::new(),
                max_sessions,
            })),
        }
    }

    pub async fn create_session(&self, device_id: &str) -> anyhow::Result<BridgeSession> {
        let mut inner = self.inner.write().await;

        if inner.sessions.len() >= inner.max_sessions as usize {
            anyhow::bail!("Maximum sessions ({}) reached", inner.max_sessions);
        }

        let now = now_ms();
        let session = BridgeSession {
            session_id: uuid::Uuid::new_v4().to_string(),
            device_id: device_id.to_string(),
            status: BridgeStatus::Connected,
            created_at_ms: now,
            last_activity_ms: now,
        };

        inner
            .sessions
            .insert(session.session_id.clone(), session.clone());
        Ok(session)
    }

    pub async fn get_session(&self, session_id: &str) -> Option<BridgeSession> {
        let inner = self.inner.read().await;
        inner.sessions.get(session_id).cloned()
    }

    pub async fn touch_session(&self, session_id: &str) {
        let mut inner = self.inner.write().await;
        if let Some(session) = inner.sessions.get_mut(session_id) {
            session.last_activity_ms = now_ms();
        }
    }

    pub async fn remove_session(&self, session_id: &str) -> Option<BridgeSession> {
        let mut inner = self.inner.write().await;
        inner.sessions.remove(session_id)
    }

    pub async fn list_sessions(&self) -> Vec<BridgeSession> {
        let inner = self.inner.read().await;
        inner.sessions.values().cloned().collect()
    }

    pub async fn evict_stale(&self, timeout_ms: u64) -> Vec<String> {
        let now = now_ms();
        let mut inner = self.inner.write().await;
        let stale: Vec<String> = inner
            .sessions
            .iter()
            .filter(|(_, s)| now.saturating_sub(s.last_activity_ms) > timeout_ms)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &stale {
            inner.sessions.remove(id);
        }
        stale
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
