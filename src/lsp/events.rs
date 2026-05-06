// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
//! LSP-related events broadcast to desktop WebSocket clients.
//!
//! All events are wrapped in [`LspBroadcastEvent`] before being sent on
//! the [`LspBroadcast`] channel held inside [`crate::gateway::AppState`].
//! The desktop side mirrors these wire shapes in
//! `desktop/src/types/lsp.ts`.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum InstallPhase {

    Resolving { message: String },

    Downloading {
        percent: Option<u8>,
        #[serde(rename = "bytesDownloaded")]
        bytes_downloaded: u64,
        #[serde(rename = "bytesTotal")]
        bytes_total: Option<u64>,
    },

    Extracting { message: String },

    Verifying { message: String },

    Done { version: String, path: String },

    Failed { reason: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ServerLifecycleStatus {

    Starting,

    Ready,

    Stopped,

    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LspBroadcastEvent {

    LspDiagnostics {
        #[serde(rename = "serverId")]
        server_id: String,
        uri: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<i64>,
        diagnostics: serde_json::Value,
    },

    LspInstallProgress {
        #[serde(rename = "serverId")]
        server_id: String,
        #[serde(flatten)]
        phase: InstallPhase,
    },

    LspServerStatus {
        #[serde(rename = "serverId")]
        server_id: String,
        #[serde(rename = "languageId")]
        language_id: String,
        status: ServerLifecycleStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

#[derive(Clone)]
pub struct LspBroadcast {
    inner: Arc<broadcast::Sender<LspBroadcastEvent>>,
}

impl std::fmt::Debug for LspBroadcast {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspBroadcast")
            .field("receivers", &self.inner.receiver_count())
            .finish()
    }
}

impl Default for LspBroadcast {
    fn default() -> Self {
        Self::new(256)
    }
}

impl LspBroadcast {

    pub fn new(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self { inner: Arc::new(tx) }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LspBroadcastEvent> {
        self.inner.subscribe()
    }

    pub fn send(&self, event: LspBroadcastEvent) -> usize {
        self.inner.send(event).unwrap_or(0)
    }

    pub fn receiver_count(&self) -> usize {
        self.inner.receiver_count()
    }
}
