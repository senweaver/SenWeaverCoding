// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Token-bucket rate limiter primitives shared across providers.
//!
//! Supports per-key limiting (e.g. per `(provider, model)` or per API key)
//! through [`RateLimiterMap`], backed by `DashMap` for lock-free lookup.

use std::hash::Hash;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use parking_lot::Mutex;

#[derive(Debug)]
pub struct TokenBucket {
    capacity: f64,
    refill_per_sec: f64,
    tokens: f64,
    last_refill: Instant,
}

impl TokenBucket {
    pub fn new(capacity: f64, refill_per_sec: f64) -> Self {
        assert!(capacity >= 0.0, "capacity must be non-negative");
        assert!(refill_per_sec >= 0.0, "refill_per_sec must be non-negative");
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
        loop {
            let eta = self.eta_for(n);
            if eta.is_zero() && self.try_acquire(n) {
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
        let b = self.get_or_create(key);
        loop {
            let eta = { b.lock().eta_for(n) };
            if eta.is_zero() {
                let mut g = b.lock();
                if g.try_acquire(n) {
                    return;
                }
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
