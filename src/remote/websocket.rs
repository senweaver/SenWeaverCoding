// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsState {
    Disconnected,
    Connecting,
    Connected,
    Closing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsMessage {
    pub msg_type: String,
    pub payload: serde_json::Value,
}

pub struct SessionWebSocket {
    url: String,
    state: Arc<RwLock<WsState>>,
    message_tx: broadcast::Sender<WsMessage>,
}

impl SessionWebSocket {
    pub fn new(url: &str) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            url: url.to_string(),
            state: Arc::new(RwLock::new(WsState::Disconnected)),
            message_tx: tx,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<WsMessage> {
        self.message_tx.subscribe()
    }

    pub async fn state(&self) -> WsState {
        *self.state.read().await
    }

    pub async fn connect(&self) -> anyhow::Result<()> {
        *self.state.write().await = WsState::Connecting;
        tracing::info!(url = %self.url, "WebSocket connecting");

        *self.state.write().await = WsState::Connected;
        Ok(())
    }

    pub async fn disconnect(&self) {
        *self.state.write().await = WsState::Closing;
        tracing::info!(url = %self.url, "WebSocket disconnecting");
        *self.state.write().await = WsState::Disconnected;
    }

    pub async fn send(&self, message: WsMessage) -> anyhow::Result<()> {
        let state = *self.state.read().await;
        if state != WsState::Connected {
            anyhow::bail!("WebSocket not connected (state: {:?})", state);
        }

        tracing::debug!(msg_type = %message.msg_type, "WebSocket sending");
        Ok(())
    }
}
