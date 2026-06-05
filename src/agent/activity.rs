// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static ACTIVE_TURNS: AtomicUsize = AtomicUsize::new(0);
static LAST_ACTIVITY_MS: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[must_use]
pub struct TurnActivityGuard {
    _private: (),
}

pub fn begin_turn() -> TurnActivityGuard {
    ACTIVE_TURNS.fetch_add(1, Ordering::SeqCst);
    LAST_ACTIVITY_MS.store(now_ms(), Ordering::SeqCst);
    TurnActivityGuard { _private: () }
}

impl Drop for TurnActivityGuard {
    fn drop(&mut self) {
        ACTIVE_TURNS.fetch_sub(1, Ordering::SeqCst);
        LAST_ACTIVITY_MS.store(now_ms(), Ordering::SeqCst);
    }
}

pub fn active_turns() -> usize {
    ACTIVE_TURNS.load(Ordering::SeqCst)
}

pub fn idle_for_ms() -> u64 {
    if ACTIVE_TURNS.load(Ordering::SeqCst) > 0 {
        return 0;
    }
    let last = LAST_ACTIVITY_MS.load(Ordering::SeqCst);
    if last == 0 {
        return u64::MAX;
    }
    now_ms().saturating_sub(last)
}

pub fn is_idle(threshold_ms: u64) -> bool {
    ACTIVE_TURNS.load(Ordering::SeqCst) == 0 && idle_for_ms() >= threshold_ms
}
