// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::hash::Hash;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use parking_lot::Mutex;

const MAX_BUCKET_WAIT: Duration = Duration::from_secs(120);

#[derive(Debug)]
pub struct TokenBucket {
    capacity: f64,
    refill_per_sec: f64,
    tokens: f64,
    last_refill: Instant,
}

impl TokenBucket {
    pub fn new(capacity: f64, refill_per_sec: f64) -> Self {
        let capacity = if capacity.is_finite() {
            capacity.max(0.0)
        } else {
            0.0
        };
        let refill_per_sec = if refill_per_sec.is_finite() {
            refill_per_sec.max(0.0)
        } else {
            0.0
        };
        Self {
            capacity,
            refill_per_sec,
            tokens: capacity,
            last_refill: Instant::now(),
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now
            .saturating_duration_since(self.last_refill)
            .as_secs_f64();
        if elapsed > 0.0 {
            self.tokens = (self.tokens + elapsed * self.refill_per_sec).min(self.capacity);
            self.last_refill = now;
        }
    }

    pub fn try_acquire(&mut self, n: f64) -> bool {
        self.refill();
        if self.tokens >= n {
            self.tokens -= n;
            true
        } else {
            false
        }
    }

    pub fn eta_for(&mut self, n: f64) -> Duration {
        self.refill();
        if self.tokens >= n {
            return Duration::ZERO;
        }
        if self.refill_per_sec <= 0.0 {
            return Duration::MAX;
        }
        let deficit = n - self.tokens;
        Duration::from_secs_f64(deficit / self.refill_per_sec)
    }

    pub async fn wait(&mut self, n: f64) {
        if !n.is_finite() || n <= 0.0 {
            return;
        }
        if self.try_acquire(n) {
            return;
        }
        if n > self.capacity || self.refill_per_sec <= 0.0 {
            tracing::warn!(
                requested = n,
                capacity = self.capacity,
                refill_per_sec = self.refill_per_sec,
                "token bucket can never satisfy request; proceeding without acquiring"
            );
            return;
        }
        let start = Instant::now();
        loop {
            let eta = self.eta_for(n);
            if eta.is_zero() && self.try_acquire(n) {
                return;
            }
            if start.elapsed() >= MAX_BUCKET_WAIT {
                tracing::warn!(
                    requested = n,
                    waited_secs = start.elapsed().as_secs(),
                    "token bucket wait exceeded upper bound; proceeding without acquiring"
                );
                return;
            }
            tokio::time::sleep(eta.min(Duration::from_secs(1))).await;
        }
    }

    pub fn available_tokens(&mut self) -> f64 {
        self.refill();
        self.tokens
    }
}

pub struct RateLimiterMap<K: Eq + Hash + Clone> {
    capacity: f64,
    refill_per_sec: f64,
    buckets: Arc<DashMap<K, Arc<Mutex<TokenBucket>>>>,
}

impl<K: Eq + Hash + Clone> RateLimiterMap<K> {
    pub fn new(capacity: f64, refill_per_sec: f64) -> Self {
        Self {
            capacity,
            refill_per_sec,
            buckets: Arc::new(DashMap::new()),
        }
    }

    fn get_or_create(&self, key: &K) -> Arc<Mutex<TokenBucket>> {
        if let Some(entry) = self.buckets.get(key) {
            return Arc::clone(&entry);
        }
        let bucket = Arc::new(Mutex::new(TokenBucket::new(
            self.capacity,
            self.refill_per_sec,
        )));
        self.buckets.insert(key.clone(), Arc::clone(&bucket));
        bucket
    }

    pub fn try_acquire(&self, key: &K, n: f64) -> bool {
        let b = self.get_or_create(key);
        let mut g = b.lock();
        g.try_acquire(n)
    }

    pub async fn wait(&self, key: &K, n: f64) {
        if !n.is_finite() || n <= 0.0 {
            return;
        }
        let b = self.get_or_create(key);
        if b.lock().try_acquire(n) {
            return;
        }
        if n > self.capacity || self.refill_per_sec <= 0.0 {
            tracing::warn!(
                requested = n,
                capacity = self.capacity,
                refill_per_sec = self.refill_per_sec,
                "token bucket can never satisfy request; proceeding without acquiring"
            );
            return;
        }
        let start = Instant::now();
        loop {
            let eta = { b.lock().eta_for(n) };
            if eta.is_zero() {
                let mut g = b.lock();
                if g.try_acquire(n) {
                    return;
                }
            }
            if start.elapsed() >= MAX_BUCKET_WAIT {
                tracing::warn!(
                    requested = n,
                    waited_secs = start.elapsed().as_secs(),
                    "token bucket wait exceeded upper bound; proceeding without acquiring"
                );
                return;
            }
            tokio::time::sleep(eta.min(Duration::from_secs(1))).await;
        }
    }

    pub fn len(&self) -> usize {
        self.buckets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buckets.is_empty()
    }
}
