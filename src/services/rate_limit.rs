// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Rate limiter — mirrors claude-code-typescript-src`services/claudeAiLimits.ts`,
// `services/rateLimitMessages.ts`, `services/policyLimits/`.
// Tracks API rate limits, policy limits, and displays user-facing messages.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
struct Bucket {
    window: Duration,
    max_requests: u32,
    timestamps: Vec<u64>,
}

impl Bucket {
    fn new(window: Duration, max_requests: u32) -> Self {
        Self {
            window,
            max_requests,
            timestamps: Vec::new(),
        }
    }

    fn prune(&mut self, now_ms: u64) {
        let cutoff = now_ms.saturating_sub(self.window.as_millis() as u64);
        self.timestamps.retain(|&t| t > cutoff);
    }

    fn try_acquire(&mut self, now_ms: u64) -> bool {
        self.prune(now_ms);
        if self.timestamps.len() < self.max_requests as usize {
            self.timestamps.push(now_ms);
            true
        } else {
            false
        }
    }

    fn remaining(&self, now_ms: u64) -> u32 {
        let cutoff = now_ms.saturating_sub(self.window.as_millis() as u64);
        let active = self.timestamps.iter().filter(|&&t| t > cutoff).count() as u32;
        self.max_requests.saturating_sub(active)
    }

    fn retry_after_ms(&self, now_ms: u64) -> Option<u64> {
        if self.remaining(now_ms) > 0 {
            return None;
        }
        self.timestamps
            .first()
            .map(|&first| first + self.window.as_millis() as u64 - now_ms)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitStatus {
    pub key: String,
    pub remaining: u32,
    pub limit: u32,
    pub retry_after_ms: Option<u64>,
    pub window_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitMessage {
    pub key: String,
    pub message: String,
    pub severity: RateLimitSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateLimitSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<RwLock<HashMap<String, Bucket>>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register(&self, key: &str, window: Duration, max_requests: u32) {
        let mut inner = self.inner.write().await;
        inner.insert(key.to_string(), Bucket::new(window, max_requests));
    }

    pub async fn try_acquire(&self, key: &str) -> bool {
        let mut inner = self.inner.write().await;
        let now = now_ms();
        if let Some(bucket) = inner.get_mut(key) {
            bucket.try_acquire(now)
        } else {
            true
        }
    }

    pub async fn status(&self, key: &str) -> Option<RateLimitStatus> {
        let inner = self.inner.read().await;
        let now = now_ms();
        inner.get(key).map(|b| RateLimitStatus {
            key: key.to_string(),
            remaining: b.remaining(now),
            limit: b.max_requests,
            retry_after_ms: b.retry_after_ms(now),
            window_secs: b.window.as_secs(),
        })
    }

    pub async fn message(&self, key: &str) -> Option<RateLimitMessage> {
        let status = self.status(key).await?;
        if status.remaining > 0 {
            return None;
        }
        let retry_secs = status.retry_after_ms.unwrap_or(0) / 1000;
        Some(RateLimitMessage {
            key: key.to_string(),
            message: format!(
                "Rate limit reached for {key}. Please wait ~{retry_secs}s before retrying."
            ),
            severity: RateLimitSeverity::Warning,
        })
    }

    pub async fn all_statuses(&self) -> Vec<RateLimitStatus> {
        let inner = self.inner.read().await;
        let now = now_ms();
        inner
            .iter()
            .map(|(key, b)| RateLimitStatus {
                key: key.clone(),
                remaining: b.remaining(now),
                limit: b.max_requests,
                retry_after_ms: b.retry_after_ms(now),
                window_secs: b.window.as_secs(),
            })
            .collect()
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
