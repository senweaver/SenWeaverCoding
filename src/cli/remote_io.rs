// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::structured_io::{StdinMessage, StdoutMessage};
use super::transports::Transport;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;

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

pub struct RemoteIO {
    transport: Arc<dyn Transport>,
    config: RemoteConfig,
    outbox: mpsc::Sender<StdoutMessage>,
    inbox: mpsc::Receiver<StdinMessage>,
    _writer_handle: crate::runtime::TaskHandle,
    _reader_handle: crate::runtime::TaskHandle,
}

impl RemoteIO {

    pub async fn new(transport: Arc<dyn Transport>, config: RemoteConfig) -> Result<Self> {
        let (in_tx, in_rx) = mpsc::channel::<StdinMessage>(64);
        let (out_tx, mut out_rx) = mpsc::channel::<StdoutMessage>(64);

        let read_transport = Arc::clone(&transport);
        let reader_handle = crate::runtime::spawn_supervised("cli.remote_reader", async move {
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
        let writer_handle = crate::runtime::spawn_supervised("cli.remote_writer", async move {
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

    pub async fn recv(&mut self) -> Option<StdinMessage> {
        self.inbox.recv().await
    }

    pub async fn write(&self, msg: StdoutMessage) -> Result<()> {
        self.outbox
            .send(msg)
            .await
            .map_err(|_| anyhow::anyhow!("Remote outbox closed"))
    }

    pub fn config(&self) -> &RemoteConfig {
        &self.config
    }

    pub fn transport(&self) -> &Arc<dyn Transport> {
        &self.transport
    }

    pub fn start_heartbeat(&self) -> tokio::task::JoinHandle<()> {
        let transport = Arc::clone(&self.transport);
        let interval_secs = self.config.heartbeat_interval_secs.max(5);
        crate::runtime::spawn_supervised("cli.remote_io.heartbeat", async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
            loop {
                interval.tick().await;
                if transport.send(r#"{"type":"heartbeat"}"#).await.is_err() {
                    tracing::debug!("Heartbeat send failed, transport likely closed");
                    break;
                }
            }
        })
        .into_inner()
    }
}
