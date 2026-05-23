// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::Transport;
use anyhow::Result;
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Mutex, mpsc};

#[derive(Debug, Clone)]
pub struct WebSocketConfig {
    pub url: String,
    pub auth_headers: Vec<(String, String)>,
    pub ping_interval_secs: u64,
    pub reconnect_delay_ms: u64,
    pub max_reconnect_attempts: u32,
    pub proxy_url: Option<String>,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            auth_headers: Vec::new(),
            ping_interval_secs: 30,
            reconnect_delay_ms: 1000,
            max_reconnect_attempts: 10,
            proxy_url: None,
        }
    }
}

pub struct WebSocketTransport {
    config: WebSocketConfig,
    connected: AtomicBool,
    outbox: mpsc::Sender<String>,
    inbox: Arc<Mutex<mpsc::Receiver<String>>>,
    in_tx: mpsc::Sender<String>,
    out_rx: Arc<Mutex<mpsc::Receiver<String>>>,
}

impl WebSocketTransport {
    pub fn new(config: WebSocketConfig) -> Self {
        let (out_tx, out_rx) = mpsc::channel(128);
        let (in_tx, in_rx) = mpsc::channel(128);

        Self {
            config,
            connected: AtomicBool::new(false),
            outbox: out_tx,
            inbox: Arc::new(Mutex::new(in_rx)),
            in_tx,
            out_rx: Arc::new(Mutex::new(out_rx)),
        }
    }

    pub async fn connect(&self) -> Result<()> {
        tracing::info!(url = %self.config.url, "Connecting WebSocket transport");

        let url = &self.config.url;
        let (ws_stream, _) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|e| anyhow::anyhow!("WebSocket connect failed: {e}"))?;

        let (mut ws_sink, mut ws_stream_rx) = ws_stream.split();

        self.connected.store(true, Ordering::SeqCst);

        let in_tx = self.in_tx.clone();
        let connected_flag = Arc::new(AtomicBool::new(true));
        let conn_read = connected_flag.clone();
        let _reader_task =
            crate::runtime::spawn_supervised("cli.transports.websocket.reader", async move {
                while let Some(msg) = ws_stream_rx.next().await {
                    if !conn_read.load(Ordering::SeqCst) {
                        break;
                    }
                    match msg {
                        Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                            if in_tx.send(text.to_string()).await.is_err() {
                                break;
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
            });

        let out_rx = self.out_rx.clone();
        let conn_write = connected_flag.clone();
        let _writer_task =
            crate::runtime::spawn_supervised("cli.transports.websocket.writer", async move {
                let mut rx = out_rx.lock().await;
                while let Some(msg) = rx.recv().await {
                    if !conn_write.load(Ordering::SeqCst) {
                        break;
                    }
                    if let Err(e) = ws_sink
                        .send(tokio_tungstenite::tungstenite::Message::Text(msg.into()))
                        .await
                    {
                        tracing::warn!(error = %e, "WebSocket write error");
                        break;
                    }
                }
            });

        tracing::info!("WebSocket transport connected");
        Ok(())
    }
}

#[async_trait]
impl Transport for WebSocketTransport {
    async fn send(&self, data: &str) -> Result<()> {
        if !self.connected.load(Ordering::SeqCst) {
            anyhow::bail!("WebSocket not connected");
        }
        self.outbox
            .send(data.to_string())
            .await
            .map_err(|_| anyhow::anyhow!("WebSocket outbox closed"))
    }

    async fn recv(&self) -> Result<Option<String>> {
        let mut inbox = self.inbox.lock().await;
        match tokio::time::timeout(std::time::Duration::from_millis(100), inbox.recv()).await {
            Ok(msg) => Ok(msg),
            Err(_) => Ok(None),
        }
    }

    async fn close(&self) -> Result<()> {
        self.connected.store(false, Ordering::SeqCst);
        tracing::info!("WebSocket transport closed");
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    fn name(&self) -> &str {
        "websocket"
    }
}
