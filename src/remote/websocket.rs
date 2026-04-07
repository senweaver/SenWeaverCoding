// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Session WebSocket — mirrors claude-code-typescript-src`remote/SessionsWebSocket.ts`.
// WebSocket client for connecting to remote session endpoints.

use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};

use serde::{Deserialize, Serialize};

/// WebSocket connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsState {
    Disconnected,
    Connecting,
    Connected,
    Closing,
}

/// A message received over the WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsMessage {
    pub msg_type: String,
    pub payload: serde_json::Value,
}

/// WebSocket client for remote sessions.
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

    /// Subscribe to incoming messages.
    pub fn subscribe(&self) -> broadcast::Receiver<WsMessage> {
        self.message_tx.subscribe()
    }

    /// Get current connection state.
    pub async fn state(&self) -> WsState {
        *self.state.read().await
    }

    /// Connect to the WebSocket endpoint.
    pub async fn connect(&self) -> anyhow::Result<()> {
        *self.state.write().await = WsState::Connecting;
        tracing::info!(url = %self.url, "WebSocket connecting");
        // Actual WebSocket connection via tokio-tungstenite would go here.
        *self.state.write().await = WsState::Connected;
        Ok(())
    }

    /// Disconnect from the WebSocket endpoint.
    pub async fn disconnect(&self) {
        *self.state.write().await = WsState::Closing;
        tracing::info!(url = %self.url, "WebSocket disconnecting");
        *self.state.write().await = WsState::Disconnected;
    }

    /// Send a message over the WebSocket.
    pub async fn send(&self, message: WsMessage) -> anyhow::Result<()> {
        let state = *self.state.read().await;
        if state != WsState::Connected {
            anyhow::bail!("WebSocket not connected (state: {:?})", state);
        }
        // Actual send via tokio-tungstenite would go here.
        tracing::debug!(msg_type = %message.msg_type, "WebSocket sending");
        Ok(())
    }
}
