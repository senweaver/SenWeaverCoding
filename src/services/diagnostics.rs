// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Diagnostics service — mirrors claude-code-typescript-src`services/diagnosticTracking.ts`.
// Tracks diagnostic events for debugging and performance analysis.

use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;

use serde::{Deserialize, Serialize};

/// A diagnostic event with timing information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticEvent {
    pub level: DiagnosticLevel,
    pub category: String,
    pub message: String,
    pub properties: Option<serde_json::Value>,
    pub timestamp_ms: u64,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// Tracks diagnostic events with a bounded buffer.
#[derive(Clone)]
pub struct DiagnosticsTracker {
    inner: Arc<RwLock<DiagnosticsInner>>,
}

struct DiagnosticsInner {
    events: VecDeque<DiagnosticEvent>,
    max_events: usize,
    slow_operations: VecDeque<SlowOperation>,
    max_slow_ops: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlowOperation {
    pub operation: String,
    pub duration_ms: u64,
    pub timestamp_ms: u64,
}

impl DiagnosticsTracker {
    pub fn new(max_events: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(DiagnosticsInner {
                events: VecDeque::new(),
                max_events,
                slow_operations: VecDeque::new(),
                max_slow_ops: 50,
            })),
        }
    }

    /// Log a diagnostic event.
    pub async fn log(
        &self,
        level: DiagnosticLevel,
        category: &str,
        message: &str,
        properties: Option<serde_json::Value>,
    ) {
        let mut inner = self.inner.write().await;
        if inner.events.len() >= inner.max_events {
            inner.events.pop_front();
        }
        inner.events.push_back(DiagnosticEvent {
            level,
            category: category.to_string(),
            message: message.to_string(),
            properties,
            timestamp_ms: now_ms(),
            duration_ms: None,
        });
    }

    /// Record a slow operation for dev-bar display.
    pub async fn record_slow_op(&self, operation: &str, duration_ms: u64) {
        let mut inner = self.inner.write().await;
        if inner.slow_operations.len() >= inner.max_slow_ops {
            inner.slow_operations.pop_front();
        }
        inner.slow_operations.push_back(SlowOperation {
            operation: operation.to_string(),
            duration_ms,
            timestamp_ms: now_ms(),
        });
    }

    /// Get recent events, optionally filtered by level.
    pub async fn recent_events(
        &self,
        limit: usize,
        min_level: Option<DiagnosticLevel>,
    ) -> Vec<DiagnosticEvent> {
        let inner = self.inner.read().await;
        inner
            .events
            .iter()
            .rev()
            .filter(|e| min_level.map_or(true, |ml| level_ord(e.level) >= level_ord(ml)))
            .take(limit)
            .cloned()
            .collect()
    }

    /// Get recent slow operations.
    pub async fn slow_operations(&self, limit: usize) -> Vec<SlowOperation> {
        let inner = self.inner.read().await;
        inner
            .slow_operations
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }
}

fn level_ord(l: DiagnosticLevel) -> u8 {
    match l {
        DiagnosticLevel::Debug => 0,
        DiagnosticLevel::Info => 1,
        DiagnosticLevel::Warn => 2,
        DiagnosticLevel::Error => 3,
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
