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
        let rem = self.remaining();
        let take = want.min(rem);
        if take == 0 {
            return 0;
        }
        self.used.fetch_add(take, Ordering::AcqRel);
        take
    }

    pub fn release(&self, amount: usize) {
        self.used
            .fetch_sub(amount.min(self.used()), Ordering::AcqRel);
    }
}

impl Default for ContextBudget {
    fn default() -> Self {

        Self::new(8_192)
    }
}
