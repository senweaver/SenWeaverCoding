// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! ??in-process [`SessionSyncHub`].
//!
//! A single tokio `broadcast::Sender` per live session id.  CLI /
//! TUI / GUI each create their own `subscribe`-backed receiver and
//! drive a local "chat view" reducer from the delta stream; the
//! [`crate::session::state::SessionActor`] is the sole publisher.
//!
//! Cross-process synchronisation (UDS / Windows Named Pipe) is
//! documented as a follow-up in the plan and intentionally
//! **not** wired here ??doing so would change the failure modes of
//! the hub (network backpressure, auth) and conflict with the
//! "ship single process first" risk mitigation.  See
//! `c:\Users\cai\.cursor\plans\phase_7_session_unification_4643f099.plan.md`
//! section "任务 7.3 ??跨端同步（进程内?? for the deferral note.

use std::collections::HashMap;
use std::sync::Arc;

use once_cell::sync::Lazy;
use parking_lot::RwLock;
use tokio::sync::broadcast;

use crate::observability::session_write_mode_metrics;
use crate::session::state::{SessionDelta, SessionId};

const CHANNEL_CAPACITY: usize = 256;

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
        {
            let guard = self.inner.read();
            if let Some(tx) = guard.get(id) {
                let _ = tx.send(delta.clone());
            }
        }
        let transport = self.transport.read().clone();
        if let Some(transport) = transport {
            let session_id = id.to_string();
            tokio::spawn(async move {
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
