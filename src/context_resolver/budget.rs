// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    High,
    Normal,
    Low,
}

#[derive(Debug)]
pub struct ContextBudget {
    total: AtomicUsize,
    used: AtomicUsize,
}

impl ContextBudget {
    pub fn new(tokens: usize) -> Self {
        Self {
            total: AtomicUsize::new(tokens),
            used: AtomicUsize::new(0),
        }
    }

    pub fn total(&self) -> usize {
        self.total.load(Ordering::Relaxed)
    }

    pub fn used(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }

    pub fn remaining(&self) -> usize {
        self.total().saturating_sub(self.used())
    }

    pub fn reserve(&self, want: usize) -> bool {
        loop {
            let used = self.used.load(Ordering::Acquire);
            if used + want > self.total.load(Ordering::Relaxed) {
                return false;
            }
            if self
                .used
                .compare_exchange(used, used + want, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return true;
            }
        }
    }

    pub fn reserve_at_most(&self, want: usize) -> usize {
        if want == 0 {
            return 0;
        }
        let total = self.total.load(Ordering::Relaxed);
        loop {
            let used = self.used.load(Ordering::Acquire);
            let take = want.min(total.saturating_sub(used));
            if take == 0 {
                return 0;
            }
            if self
                .used
                .compare_exchange(used, used + take, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return take;
            }
        }
    }

    pub fn release(&self, amount: usize) {
        let _ = self
            .used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                Some(used.saturating_sub(amount))
            });
    }
}

impl Default for ContextBudget {
    fn default() -> Self {

        Self::new(8_192)
    }
}
