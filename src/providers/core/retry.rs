// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryClass {

    Transient,

    RateLimited,

    Permanent,
}

impl RetryClass {
    pub const fn is_retryable(self) -> bool {
        matches!(self, Self::Transient | Self::RateLimited)
    }
}

#[derive(Debug, Clone)]
pub struct RetryBudget {
    pub max_attempts: u32,
    pub max_elapsed: Duration,
    started: Instant,
    attempts: u32,
}

impl RetryBudget {
    pub fn new(max_attempts: u32, max_elapsed: Duration) -> Self {
        Self {
            max_attempts,
            max_elapsed,
            started: Instant::now(),
            attempts: 0,
        }
    }

    pub fn try_consume(&mut self) -> bool {
        if self.attempts >= self.max_attempts {
            return false;
        }
        if self.started.elapsed() >= self.max_elapsed {
            return false;
        }
        self.attempts += 1;
        true
    }

    pub fn attempts(&self) -> u32 {
        self.attempts
    }
}

pub fn exp_backoff(attempt: u32, base_ms: u64, max_ms: u64, jitter: f64) -> Duration {
    let shift = attempt.min(20);
    let raw = base_ms.saturating_mul(1u64 << shift);
    let capped = raw.min(max_ms);
    let jitter_ratio = 1.0 + (pseudo_jitter(attempt) - 0.5) * 2.0 * jitter;
    let millis = (capped as f64 * jitter_ratio).max(0.0) as u64;
    Duration::from_millis(millis.min(max_ms))
}

fn pseudo_jitter(seed: u32) -> f64 {
    let x = seed.wrapping_mul(2_654_435_761);
    (x as f64 / u32::MAX as f64).clamp(0.0, 1.0)
}

#[derive(Debug, Default)]
pub struct ReliabilityCounter {
    successes: AtomicU64,
    failures: AtomicU64,
    retries: AtomicU64,
    latency_ms_sum: AtomicU64,
    latency_samples: AtomicU64,
}

impl ReliabilityCounter {
    pub const fn new() -> Self {
        Self {
            successes: AtomicU64::new(0),
            failures: AtomicU64::new(0),
            retries: AtomicU64::new(0),
            latency_ms_sum: AtomicU64::new(0),
            latency_samples: AtomicU64::new(0),
        }
    }

    pub fn record_success(&self, latency: Duration) {
        self.successes.fetch_add(1, Ordering::Relaxed);
        self.latency_ms_sum
            .fetch_add(latency.as_millis() as u64, Ordering::Relaxed);
        self.latency_samples.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_failure(&self, retries: u32) {
        self.failures.fetch_add(1, Ordering::Relaxed);
        self.retries.fetch_add(retries as u64, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> ReliabilitySnapshot {
        let s = self.successes.load(Ordering::Relaxed);
        let f = self.failures.load(Ordering::Relaxed);
        let total = s + f;
        let samples = self.latency_samples.load(Ordering::Relaxed).max(1);
        ReliabilitySnapshot {
            successes: s,
            failures: f,
            retries: self.retries.load(Ordering::Relaxed),
            avg_latency_ms: self.latency_ms_sum.load(Ordering::Relaxed) / samples,
            success_rate: if total == 0 {
                1.0
            } else {
                s as f64 / total as f64
            },
        }
    }

    pub fn reset(&self) {
        self.successes.store(0, Ordering::Relaxed);
        self.failures.store(0, Ordering::Relaxed);
        self.retries.store(0, Ordering::Relaxed);
        self.latency_ms_sum.store(0, Ordering::Relaxed);
        self.latency_samples.store(0, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ReliabilitySnapshot {
    pub successes: u64,
    pub failures: u64,
    pub retries: u64,
    pub avg_latency_ms: u64,
    pub success_rate: f64,
}
