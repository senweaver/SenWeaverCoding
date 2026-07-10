// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::Serialize;
use tokio::sync::broadcast;

const RUN_STATE_BROADCAST_CAPACITY: usize = 256;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRunStateEvent {
    pub session_id: String,
    pub running: bool,
}

pub struct SessionRunStateRegistry {
    inner: RwLock<HashSet<String>>,
    tx: broadcast::Sender<SessionRunStateEvent>,
}

impl SessionRunStateRegistry {
    pub fn new() -> Arc<Self> {
        let (tx, _) = broadcast::channel(RUN_STATE_BROADCAST_CAPACITY);
        Arc::new(Self {
            inner: RwLock::new(HashSet::new()),
            tx,
        })
    }

    pub fn snapshot(&self) -> Vec<String> {
        let guard = self.inner.read();
        guard.iter().cloned().collect()
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.inner.read().contains(id)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SessionRunStateEvent> {
        self.tx.subscribe()
    }

    pub fn guard(self: &Arc<Self>, session_id: impl Into<String>) -> SessionRunGuard {
        let session_id = session_id.into();
        let was_inserted = {
            let mut guard = self.inner.write();
            guard.insert(session_id.clone())
        };
        if was_inserted {
            let _ = self.tx.send(SessionRunStateEvent {
                session_id: session_id.clone(),
                running: true,
            });
        }
        SessionRunGuard {
            registry: Arc::clone(self),
            session_id,
            was_inserted,
        }
    }
}

pub struct SessionRunGuard {
    registry: Arc<SessionRunStateRegistry>,
    session_id: String,
    was_inserted: bool,
}

impl SessionRunGuard {
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Whether this guard actually claimed the session's run slot (i.e. the
    /// session was not already running when this guard was created).
    pub fn was_inserted(&self) -> bool {
        self.was_inserted
    }
}

impl Drop for SessionRunGuard {
    fn drop(&mut self) {
        // A nested/duplicate guard (was_inserted == false) does NOT own this
        // session's run: releasing its resource locks here would strip the
        // still-running outer turn of the file/shell/browser locks it holds.
        if !self.was_inserted {
            return;
        }
        if let Some(manager) = crate::session::global_workspace_resources() {
            manager.release_all_for_session(&self.session_id);
        }
        let removed = {
            let mut guard = self.registry.inner.write();
            guard.remove(&self.session_id)
        };
        if removed {
            let _ = self.registry.tx.send(SessionRunStateEvent {
                session_id: self.session_id.clone(),
                running: false,
            });
        }
    }
}
