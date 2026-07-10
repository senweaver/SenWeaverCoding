// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use std::collections::HashMap;
use std::path::PathBuf;
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
    persist_dir: Option<PathBuf>,
    persistence_started: bool,
}

const MAX_BUFFERED_EVENTS: usize = 10_000;

impl AnalyticsService {
    pub fn new(enabled: bool) -> Self {
        Self::new_with_persistence(enabled, None)
    }

    pub fn new_with_persistence(enabled: bool, persist_dir: Option<PathBuf>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(AnalyticsInner {
                events: Vec::new(),
                feature_gates: HashMap::new(),
                enabled,
                flush_interval_ms: 60_000,
                persist_dir,
                persistence_started: false,
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
        if inner.events.len() > MAX_BUFFERED_EVENTS {
            let overflow = inner.events.len() - MAX_BUFFERED_EVENTS;
            inner.events.drain(0..overflow);
        }
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

    pub async fn flush_to_disk(&self) -> usize {
        let (dir, events) = {
            let mut inner = self.inner.write().await;
            match inner.persist_dir.clone() {
                Some(dir) if !inner.events.is_empty() => {
                    (dir, std::mem::take(&mut inner.events))
                }
                _ => return 0,
            }
        };

        let count = events.len();
        let write_result = tokio::task::spawn_blocking(move || persist_events(&dir, &events))
            .await
            .unwrap_or_else(|e| Err(std::io::Error::other(e.to_string())));

        match write_result {
            Ok(()) => count,
            Err(e) => {
                tracing::warn!(
                    target: "analytics",
                    error = %e,
                    "failed to persist analytics events to disk; dropping this batch"
                );
                0
            }
        }
    }

    pub fn start_persistence_loop(&self) {
        let interval_ms = {
            let mut inner = match self.inner.try_write() {
                Ok(g) => g,
                Err(_) => return,
            };
            if inner.persist_dir.is_none() || inner.persistence_started {
                return;
            }
            inner.persistence_started = true;
            inner.flush_interval_ms
        };

        if tokio::runtime::Handle::try_current().is_err() {
            tracing::debug!(
                target: "analytics",
                "no tokio runtime available; analytics persistence loop not started"
            );
            return;
        }

        let service = self.clone();
        crate::runtime::spawn_supervised("services.analytics.flush", async move {
            let mut ticker =
                tokio::time::interval(std::time::Duration::from_millis(interval_ms.max(1_000)));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                let written = service.flush_to_disk().await;
                if written > 0 {
                    tracing::debug!(
                        target: "analytics",
                        written,
                        "flushed analytics events to disk"
                    );
                }
            }
        });
    }
}

fn persist_events(dir: &std::path::Path, events: &[AnalyticsEvent]) -> std::io::Result<()> {
    use std::io::Write as _;

    std::fs::create_dir_all(dir)?;
    let day = chrono::Utc::now().format("%Y%m%d");
    let path = dir.join(format!("events-{day}.jsonl"));
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let mut buf = String::with_capacity(events.len() * 96);
    for event in events {
        match serde_json::to_string(event) {
            Ok(line) => {
                buf.push_str(&line);
                buf.push('\n');
            }
            Err(e) => {
                tracing::warn!(
                    target: "analytics",
                    error = %e,
                    "failed to serialize analytics event; skipping"
                );
            }
        }
    }
    file.write_all(buf.as_bytes())?;
    Ok(())
}
