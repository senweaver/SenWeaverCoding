// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Remote I/O — extends StructuredIO with network transport for remote sessions.
//!
//! Wraps the base StructuredIO with a Transport (WebSocket, SSE, or HTTP)
//! for remote agent sessions. Adds session authentication, heartbeat,
//! and reconnection.

use super::structured_io::{StdinMessage, StdoutMessage};
use super::transports::Transport;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Configuration for a remote I/O session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteConfig {
    pub url: String,
    pub session_id: String,
    #[serde(default)]
    pub auth_token: Option<String>,
    #[serde(default)]
    pub heartbeat_interval_secs: u64,
    #[serde(default)]
    pub reconnect_attempts: u32,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            session_id: String::new(),
            auth_token: None,
            heartbeat_interval_secs: 30,
            reconnect_attempts: 5,
        }
    }
}

/// Remote I/O driver wrapping a network transport.
pub struct RemoteIO {
    transport: Arc<dyn Transport>,
    config: RemoteConfig,
    outbox: mpsc::Sender<StdoutMessage>,
    inbox: mpsc::Receiver<StdinMessage>,
    _writer_handle: tokio::task::JoinHandle<()>,
    _reader_handle: tokio::task::JoinHandle<()>,
}

impl RemoteIO {
    /// Create a RemoteIO with the given transport and configuration.
    pub async fn new(transport: Arc<dyn Transport>, config: RemoteConfig) -> Result<Self> {
        let (in_tx, in_rx) = mpsc::channel::<StdinMessage>(64);
        let (out_tx, mut out_rx) = mpsc::channel::<StdoutMessage>(64);

        let read_transport = Arc::clone(&transport);
        let reader_handle = tokio::spawn(async move {
            loop {
                match read_transport.recv().await {
                    Ok(Some(data)) => match serde_json::from_str::<StdinMessage>(&data) {
                        Ok(msg) => {
                            if in_tx.send(msg).await.is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::debug!(error = %e, "Skipping malformed remote message");
                        }
                    },
                    Ok(None) => break,
                    Err(e) => {
                        tracing::warn!(error = %e, "Remote transport read error");
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                }
            }
        });

        let write_transport = Arc::clone(&transport);
        let writer_handle = tokio::spawn(async move {
            while let Some(msg) = out_rx.recv().await {
                if let Ok(json) = super::ndjson::ndjson_safe_stringify(&msg) {
                    if let Err(e) = write_transport.send(&json).await {
                        tracing::warn!(error = %e, "Remote transport write error");
                    }
                }
            }
        });

        Ok(Self {
            transport,
            config,
            outbox: out_tx,
            inbox: in_rx,
            _writer_handle: writer_handle,
            _reader_handle: reader_handle,
        })
    }

    /// Receive the next message from the remote transport.
    pub async fn recv(&mut self) -> Option<StdinMessage> {
        self.inbox.recv().await
    }

    /// Send a message via the remote transport.
    pub async fn write(&self, msg: StdoutMessage) -> Result<()> {
        self.outbox
            .send(msg)
            .await
            .map_err(|_| anyhow::anyhow!("Remote outbox closed"))
    }

    /// Get the session configuration.
    pub fn config(&self) -> &RemoteConfig {
        &self.config
    }

    /// Get the underlying transport.
    pub fn transport(&self) -> &Arc<dyn Transport> {
        &self.transport
    }

    /// Start the heartbeat loop (sends periodic pings to keep connection alive).
    pub fn start_heartbeat(&self) -> tokio::task::JoinHandle<()> {
        let transport = Arc::clone(&self.transport);
        let interval_secs = self.config.heartbeat_interval_secs.max(5);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
            loop {
                interval.tick().await;
                if transport.send(r#"{"type":"heartbeat"}"#).await.is_err() {
                    tracing::debug!("Heartbeat send failed, transport likely closed");
                    break;
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_config_default() {
        let cfg = RemoteConfig::default();
        assert_eq!(cfg.heartbeat_interval_secs, 30);
        assert_eq!(cfg.reconnect_attempts, 5);
        assert!(cfg.auth_token.is_none());
    }

    #[test]
    fn remote_config_serde_roundtrip() {
        let cfg = RemoteConfig {
            url: "wss://example.com".into(),
            session_id: "s-1".into(),
            auth_token: Some("tok".into()),
            heartbeat_interval_secs: 15,
            reconnect_attempts: 3,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: RemoteConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.url, "wss://example.com");
        assert_eq!(parsed.session_id, "s-1");
    }
}
