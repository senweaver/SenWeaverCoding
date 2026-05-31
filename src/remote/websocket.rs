// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Mutex, RwLock, broadcast, mpsc};

use futures_util::{SinkExt, StreamExt};
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
    outbox: mpsc::Sender<String>,
    out_rx: Arc<Mutex<mpsc::Receiver<String>>>,
    shutdown: Arc<AtomicBool>,
}

impl SessionWebSocket {
    pub fn new(url: &str) -> Self {
        let (out_tx, out_rx) = mpsc::channel(256);
        let (message_tx, _) = broadcast::channel(256);
        Self {
            url: url.to_string(),
            state: Arc::new(RwLock::new(WsState::Disconnected)),
            message_tx,
            outbox: out_tx,
            out_rx: Arc::new(Mutex::new(out_rx)),
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<WsMessage> {
        self.message_tx.subscribe()
    }

    pub async fn state(&self) -> WsState {
        *self.state.read().await
    }

    pub async fn connect(&self) -> anyhow::Result<()> {
        if matches!(
            *self.state.read().await,
            WsState::Connected | WsState::Connecting
        ) {
            return Ok(());
        }

        *self.state.write().await = WsState::Connecting;
        tracing::info!(url = %self.url, "WebSocket connecting");

        let (ws_stream, _) = tokio_tungstenite::connect_async(&self.url)
            .await
            .map_err(|e| anyhow::anyhow!("WebSocket connect failed: {e}"))?;

        let (mut ws_sink, mut ws_stream_rx) = ws_stream.split();
        self.shutdown.store(false, Ordering::SeqCst);

        let message_tx = self.message_tx.clone();
        let shutdown_read = Arc::clone(&self.shutdown);
        crate::runtime::spawn_supervised("remote.websocket.reader", async move {
            while let Some(msg) = ws_stream_rx.next().await {
                if shutdown_read.load(Ordering::SeqCst) {
                    break;
                }
                match msg {
                    Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                        match serde_json::from_str::<WsMessage>(&text) {
                            Ok(parsed) => {
                                let _ = message_tx.send(parsed);
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "WebSocket JSON parse failed");
                            }
                        }
                    }
                    Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => {
                        tracing::info!("WebSocket closed by server");
                        break;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(error = %e, "WebSocket read error");
                        break;
                    }
                }
            }
            shutdown_read.store(true, Ordering::SeqCst);
        });

        let out_rx = Arc::clone(&self.out_rx);
        let shutdown_write = Arc::clone(&self.shutdown);
        crate::runtime::spawn_supervised("remote.websocket.writer", async move {
            let mut rx = out_rx.lock().await;
            while let Some(payload) = rx.recv().await {
                if shutdown_write.load(Ordering::SeqCst) {
                    break;
                }
                if let Err(e) = ws_sink
                    .send(tokio_tungstenite::tungstenite::Message::Text(
                        payload.into(),
                    ))
                    .await
                {
                    tracing::warn!(error = %e, "WebSocket write error");
                    break;
                }
            }
            let _ = ws_sink
                .send(tokio_tungstenite::tungstenite::Message::Close(None))
                .await;
            shutdown_write.store(true, Ordering::SeqCst);
        });

        *self.state.write().await = WsState::Connected;
        Ok(())
    }

    pub async fn disconnect(&self) {
        *self.state.write().await = WsState::Closing;
        self.shutdown.store(true, Ordering::SeqCst);
        tracing::info!(url = %self.url, "WebSocket disconnecting");
        *self.state.write().await = WsState::Disconnected;
    }

    pub async fn send(&self, message: WsMessage) -> anyhow::Result<()> {
        let state = *self.state.read().await;
        if state != WsState::Connected {
            anyhow::bail!("WebSocket not connected (state: {:?})", state);
        }

        let payload = serde_json::to_string(&message)?;
        self.outbox
            .send(payload)
            .await
            .map_err(|_| anyhow::anyhow!("WebSocket outbox closed"))?;
        tracing::debug!(msg_type = %message.msg_type, "WebSocket sending");
        Ok(())
    }
}
