// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Analytics service — mirrors claude-code-typescript-src`services/analytics/`.
// Provides event logging, feature-gate evaluation, and usage telemetry.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsEvent {
    pub name: String,
    pub properties: HashMap<String, serde_json::Value>,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateValue {
    On,
    Off,
    Unknown,
}

#[derive(Clone)]
pub struct AnalyticsService {
    inner: Arc<RwLock<AnalyticsInner>>,
}

struct AnalyticsInner {
    events: Vec<AnalyticsEvent>,
    feature_gates: HashMap<String, GateValue>,
    enabled: bool,
    flush_interval_ms: u64,
}

impl AnalyticsService {
    pub fn new(enabled: bool) -> Self {
        Self {
            inner: Arc::new(RwLock::new(AnalyticsInner {
                events: Vec::new(),
                feature_gates: HashMap::new(),
                enabled,
                flush_interval_ms: 60_000,
            })),
        }
    }

    pub async fn log_event(&self, name: &str, properties: HashMap<String, serde_json::Value>) {
        let mut inner = self.inner.write().await;
        if !inner.enabled {
            return;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        inner.events.push(AnalyticsEvent {
            name: name.to_string(),
            properties,
            timestamp_ms: now,
        });
    }

    pub async fn check_feature_gate(&self, gate: &str) -> GateValue {
        let inner = self.inner.read().await;
        inner
            .feature_gates
            .get(gate)
            .copied()
            .unwrap_or(GateValue::Unknown)
    }

    pub async fn set_feature_gate(&self, gate: &str, value: GateValue) {
        let mut inner = self.inner.write().await;
        inner.feature_gates.insert(gate.to_string(), value);
    }

    pub async fn flush(&self) -> Vec<AnalyticsEvent> {
        let mut inner = self.inner.write().await;
        let events = std::mem::take(&mut inner.events);

        events
    }

    pub async fn pending_count(&self) -> usize {
        let inner = self.inner.read().await;
        inner.events.len()
    }
}
