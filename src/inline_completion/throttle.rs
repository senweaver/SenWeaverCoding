// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use parking_lot::Mutex;
use std::time::{Duration, Instant};

pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(150);
pub const DEFAULT_MIN_PREFIX_CHARS: usize = 3;
pub const DEFAULT_REJECT_LOCK: Duration = Duration::from_millis(600);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrottlerDecision {

    Allow,

    TooShort,

    Debounced,

    RejectedRecently,
}

#[derive(Debug)]
pub struct Throttler {
    inner: Mutex<Inner>,
}

#[derive(Debug)]
struct Inner {
    debounce: Duration,
    min_prefix: usize,
    reject_lock: Duration,
    last_request: Option<Instant>,
    last_reject: Option<(Instant, u64)>,
}

impl Throttler {
    pub fn with_defaults() -> Self {
        Self::new(
            DEFAULT_DEBOUNCE,
            DEFAULT_MIN_PREFIX_CHARS,
            DEFAULT_REJECT_LOCK,
        )
    }

    pub fn new(debounce: Duration, min_prefix: usize, reject_lock: Duration) -> Self {
        Self {
            inner: Mutex::new(Inner {
                debounce,
                min_prefix,
                reject_lock,
                last_request: None,
                last_reject: None,
            }),
        }
    }

    pub fn try_acquire(&self, prefix_chars: usize, prefix_hash: u64) -> ThrottlerDecision {
        let mut inner = self.inner.lock();
        let now = Instant::now();
        if prefix_chars < inner.min_prefix {
            return ThrottlerDecision::TooShort;
        }
        if let Some(last) = inner.last_request
            && now.duration_since(last) < inner.debounce
        {
            return ThrottlerDecision::Debounced;
        }
        if let Some((at, h)) = inner.last_reject
            && h == prefix_hash
            && now.duration_since(at) < inner.reject_lock
        {
            return ThrottlerDecision::RejectedRecently;
        }
        inner.last_request = Some(now);
        ThrottlerDecision::Allow
    }

    pub fn record_reject(&self, prefix_hash: u64) {
        self.inner.lock().last_reject = Some((Instant::now(), prefix_hash));
    }

    pub fn reset(&self) {
        let mut inner = self.inner.lock();
        inner.last_request = None;
        inner.last_reject = None;
    }
}
