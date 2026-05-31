// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
struct Bucket {
    window: Duration,
    max_requests: u32,
    timestamps: VecDeque<u64>,
}

impl Bucket {
    fn new(window: Duration, max_requests: u32) -> Self {
        Self {
            window,
            max_requests,
            timestamps: VecDeque::new(),
        }
    }

    fn prune(&mut self, now_ms: u64) {
        let cutoff = now_ms.saturating_sub(self.window.as_millis() as u64);
        while let Some(&front) = self.timestamps.front() {
            if front > cutoff {
                break;
            }
            self.timestamps.pop_front();
        }
    }

    fn try_acquire(&mut self, now_ms: u64) -> bool {
        self.prune(now_ms);
        if self.timestamps.len() < self.max_requests as usize {
            self.timestamps.push_back(now_ms);
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
            .front()
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
    inner: Arc<DashMap<String, parking_lot::Mutex<Bucket>>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
        }
    }

    pub async fn register(&self, key: &str, window: Duration, max_requests: u32) {
        self.inner.insert(
            key.to_string(),
            parking_lot::Mutex::new(Bucket::new(window, max_requests)),
        );
    }

    pub async fn try_acquire(&self, key: &str) -> bool {
        let now = now_ms();
        if let Some(bucket) = self.inner.get(key) {
            bucket.lock().try_acquire(now)
        } else {
            true
        }
    }

    pub async fn status(&self, key: &str) -> Option<RateLimitStatus> {
        let now = now_ms();
        self.inner.get(key).map(|entry| {
            let b = entry.lock();
            RateLimitStatus {
                key: key.to_string(),
                remaining: b.remaining(now),
                limit: b.max_requests,
                retry_after_ms: b.retry_after_ms(now),
                window_secs: b.window.as_secs(),
            }
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
        let now = now_ms();
        self.inner
            .iter()
            .map(|entry| {
                let key = entry.key().clone();
                let b = entry.value().lock();
                RateLimitStatus {
                    key,
                    remaining: b.remaining(now),
                    limit: b.max_requests,
                    retry_after_ms: b.retry_after_ms(now),
                    window_secs: b.window.as_secs(),
                }
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
