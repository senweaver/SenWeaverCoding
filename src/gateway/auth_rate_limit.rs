// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Sliding-window rate limiter for authentication attempts.
//!
//! Protects pairing and bearer-token validation endpoints against
//! brute-force attacks.  Tracks per-IP attempt timestamps and enforces
//! a lockout period after too many failures within the sliding window.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

pub const MAX_ATTEMPTS: u32 = 10;

pub const WINDOW_SECS: u64 = 60;

pub const LOCKOUT_SECS: u64 = 300;

const SWEEP_INTERVAL_SECS: u64 = 300;

#[derive(Debug, Clone)]
pub struct RateLimitError {

    pub retry_after_secs: u64,
}

#[derive(Debug)]
pub struct AuthRateLimiter {
    inner: Mutex<Inner>,
}

#[derive(Debug)]
struct Inner {

    attempts: HashMap<String, Vec<Instant>>,

    lockouts: HashMap<String, Instant>,
    last_sweep: Instant,
}

impl AuthRateLimiter {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                attempts: HashMap::new(),
                lockouts: HashMap::new(),
                last_sweep: Instant::now(),
            }),
        }
    }

    fn is_loopback(key: &str) -> bool {
        matches!(key, "127.0.0.1" | "::1")
            || key
                .parse::<IpAddr>()
                .map(|ip| ip.is_loopback())
                .unwrap_or(false)
    }

    pub fn check_rate_limit(&self, key: &str) -> Result<(), RateLimitError> {
        if Self::is_loopback(key) {
            return Ok(());
        }

        let now = Instant::now();
        let mut inner = self.inner.lock();
        Self::maybe_sweep(&mut inner, now);

        if let Some(&locked_at) = inner.lockouts.get(key) {
            let elapsed = now.duration_since(locked_at).as_secs();
            if elapsed < LOCKOUT_SECS {
                return Err(RateLimitError {
                    retry_after_secs: LOCKOUT_SECS - elapsed,
                });
            }

            inner.lockouts.remove(key);
            inner.attempts.remove(key);
        }

        let window = Duration::from_secs(WINDOW_SECS);
        if let Some(timestamps) = inner.attempts.get_mut(key) {
            timestamps.retain(|t| now.duration_since(*t) < window);
            if timestamps.len() >= MAX_ATTEMPTS as usize {

                inner.lockouts.insert(key.to_owned(), now);
                return Err(RateLimitError {
                    retry_after_secs: LOCKOUT_SECS,
                });
            }
        }

        Ok(())
    }

    pub fn record_attempt(&self, key: &str) {
        if Self::is_loopback(key) {
            return;
        }

        let now = Instant::now();
        let mut inner = self.inner.lock();
        inner.attempts.entry(key.to_owned()).or_default().push(now);
    }

    pub fn is_locked_out(&self, key: &str) -> bool {
        if Self::is_loopback(key) {
            return false;
        }

        let now = Instant::now();
        let inner = self.inner.lock();
        if let Some(&locked_at) = inner.lockouts.get(key) {
            return now.duration_since(locked_at).as_secs() < LOCKOUT_SECS;
        }
        false
    }

    fn maybe_sweep(inner: &mut Inner, now: Instant) {
        if inner.last_sweep.elapsed() < Duration::from_secs(SWEEP_INTERVAL_SECS) {
            return;
        }
        inner.last_sweep = now;

        let lockout_dur = Duration::from_secs(LOCKOUT_SECS);
        let window_dur = Duration::from_secs(WINDOW_SECS);

        inner
            .lockouts
            .retain(|_, locked_at| now.duration_since(*locked_at) < lockout_dur);

        inner.attempts.retain(|_, timestamps| {
            timestamps.retain(|t| now.duration_since(*t) < window_dur);
            !timestamps.is_empty()
        });
    }
}

impl Default for AuthRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}
