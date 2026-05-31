// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};

use super::types::{BridgeStatus, PollConfig};

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct TransportState {
    status: BridgeStatus,
    retry_count: u32,
    last_error: Option<String>,
}

#[derive(Clone)]
pub struct BridgeTransport {
    state: Arc<RwLock<TransportState>>,
    poll_config: PollConfig,
    status_tx: broadcast::Sender<BridgeStatus>,
}

impl BridgeTransport {
    pub fn new(poll_config: PollConfig) -> Self {
        let (status_tx, _) = broadcast::channel(16);
        Self {
            state: Arc::new(RwLock::new(TransportState {
                status: BridgeStatus::Disconnected,
                retry_count: 0,
                last_error: None,
            })),
            poll_config,
            status_tx,
        }
    }

    pub fn subscribe_status(&self) -> broadcast::Receiver<BridgeStatus> {
        self.status_tx.subscribe()
    }

    pub async fn status(&self) -> BridgeStatus {
        self.state.read().await.status
    }

    pub async fn connect(&self, url: &str) -> anyhow::Result<()> {
        self.set_status(BridgeStatus::Connecting).await;
        tracing::info!(url = url, "Bridge transport connecting via WebSocket");

        let (ws_stream, _) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|e| anyhow::anyhow!("WebSocket connect failed: {e}"))?;

        tracing::info!("Bridge WebSocket connected");
        drop(ws_stream);
        self.set_status(BridgeStatus::Connected).await;
        Ok(())
    }

    pub async fn disconnect(&self) {
        self.set_status(BridgeStatus::Disconnected).await;
        let mut state = self.state.write().await;
        state.retry_count = 0;
    }

    pub async fn reconnect(&self, url: &str) -> anyhow::Result<()> {
        let delay = self.next_retry_delay().await;
        tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
        self.connect(url).await
    }

    async fn next_retry_delay(&self) -> u64 {
        let mut state = self.state.write().await;
        state.retry_count += 1;

        let base = self.poll_config.initial_delay_ms as f64
            * self
                .poll_config
                .backoff_multiplier
                .powi(state.retry_count as i32 - 1);
        let capped = base.min(self.poll_config.max_delay_ms as f64);

        let jitter_range = capped * self.poll_config.jitter_fraction;
        let jitter = rand::random::<f64>() * jitter_range * 2.0 - jitter_range;

        ((capped + jitter).max(0.0)) as u64
    }

    async fn set_status(&self, status: BridgeStatus) {
        let mut state = self.state.write().await;
        state.status = status;
        let _ = self.status_tx.send(status);
    }
}
