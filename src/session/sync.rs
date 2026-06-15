// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::Arc;

use once_cell::sync::Lazy;
use parking_lot::RwLock;
use tokio::sync::broadcast;

use crate::observability::session_write_mode_metrics;
use crate::session::state::{SessionDelta, SessionId};

const CHANNEL_CAPACITY: usize = 1024;

static GLOBAL: Lazy<Arc<SessionSyncHub>> = Lazy::new(|| Arc::new(SessionSyncHub::new()));

pub struct SessionSyncHub {
    inner: RwLock<HashMap<SessionId, broadcast::Sender<SessionDelta>>>,

    transport: RwLock<Option<Arc<dyn super::rpc::SessionRpcTransport>>>,
}

impl SessionSyncHub {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
            transport: RwLock::new(None),
        }
    }

    pub fn global() -> Arc<SessionSyncHub> {
        GLOBAL.clone()
    }

    pub fn register(&self, id: impl Into<SessionId>) -> broadcast::Sender<SessionDelta> {
        let id = id.into();
        {
            let guard = self.inner.read();
            if let Some(tx) = guard.get(&id) {
                return tx.clone();
            }
        }
        let (tx, _rx) = broadcast::channel(CHANNEL_CAPACITY);
        let mut guard = self.inner.write();
        let entry = guard
            .entry(id)
            .or_insert_with(|| tx.clone());
        let handle = entry.clone();
        session_write_mode_metrics::set_session_hub_active_sessions(guard.len() as u64);
        handle
    }

    pub fn deregister(&self, id: &str) {
        let mut guard = self.inner.write();
        guard.remove(id);
        session_write_mode_metrics::set_session_hub_active_sessions(guard.len() as u64);
    }

    pub fn subscribe(&self, id: &str) -> broadcast::Receiver<SessionDelta> {
        let tx = self.register(id.to_string());
        session_write_mode_metrics::incr_session_hub_subscribers();
        tx.subscribe()
    }

    pub fn publish(&self, id: &str, delta: SessionDelta) {
        self.publish_local(id, &delta);
        let transport = self.transport.read().clone();
        if let Some(transport) = transport {
            let session_id = id.to_string();
            crate::runtime::spawn_supervised("session.sync_transport", async move {
                if let Err(e) = transport.send(&delta).await {
                    tracing::warn!(
                        target: "session.sync",
                        session_id = %session_id,
                        error = %e,
                        "cross-process transport send failed"
                    );
                }
            });
        }
    }

    pub fn publish_local(&self, id: &str, delta: &SessionDelta) {
        let guard = self.inner.read();
        if let Some(tx) = guard.get(id) {
            let _ = tx.send(delta.clone());
        }
    }

    pub fn sender(&self, id: &str) -> broadcast::Sender<SessionDelta> {
        self.register(id.to_string())
    }

    pub fn active_session_count(&self) -> usize {
        self.inner.read().len()
    }

    pub fn with_transport(&self, transport: Arc<dyn super::rpc::SessionRpcTransport>) {
        *self.transport.write() = Some(transport);
        tracing::debug!(target: "session.sync", "cross-process transport registered");
    }

    pub fn clear_transport(&self) {
        *self.transport.write() = None;
    }
}

impl Default for SessionSyncHub {
    fn default() -> Self {
        Self::new()
    }
}
