// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Provider health signal published periodically by
//! [`crate::providers::reliable`] and consumed by
//! [`crate::agent::task_router::TaskRouter`] (for routing score
//! penalties) and [`crate::agent::supervisor::Supervisor`] (for
//! back-off decisions).
//!
//! C.3 — the struct and broadcast hub are intentionally
//! decoupled from the provider-specific aggregation code.  Producers
//! call [`HealthBroadcaster::publish`]; consumers `subscribe()` and
//! receive signals over a bounded `tokio::sync::broadcast` channel.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast;

#[derive(Debug, Clone, PartialEq)]
pub struct HealthSignal {

    pub provider: String,

    pub model: String,

    pub window_secs: u64,

    pub success_rate: f64,

    pub p95_latency_ms: u64,

    pub retries_per_req: f64,

    pub cost_per_1k_tok: f64,
}

impl HealthSignal {

    pub fn is_unhealthy(&self) -> bool {
        self.success_rate < 0.80
    }

    pub fn health_penalty(&self) -> f64 {
        (1.0 - self.success_rate).clamp(0.0, 1.0)
    }

    pub fn key(&self) -> (String, String) {
        (self.provider.clone(), self.model.clone())
    }
}

pub const HEALTH_WINDOW: Duration = Duration::from_secs(30);

const CHANNEL_CAPACITY: usize = 64;

#[derive(Clone)]
pub struct HealthBroadcaster {
    inner: Arc<broadcast::Sender<HealthSignal>>,
}

impl HealthBroadcaster {

    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(CHANNEL_CAPACITY);
        Self {
            inner: Arc::new(tx),
        }
    }

    pub fn publish(&self, signal: HealthSignal) -> usize {
        self.inner.send(signal).unwrap_or(0)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<HealthSignal> {
        self.inner.subscribe()
    }

    pub fn receiver_count(&self) -> usize {
        self.inner.receiver_count()
    }
}

impl Default for HealthBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for HealthBroadcaster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HealthBroadcaster")
            .field("receiver_count", &self.receiver_count())
            .finish()
    }
}
