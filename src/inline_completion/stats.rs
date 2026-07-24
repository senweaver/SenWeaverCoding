// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use parking_lot::Mutex;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy)]
pub enum AcceptanceEvent {
    Shown,
    Accepted,
    AcceptedPartial,
    Rejected,
    TimedOut,
}

#[derive(Debug, Default)]
pub struct CompletionStats {
    inner: Mutex<StatsInner>,
}

#[derive(Debug, Default)]
struct StatsInner {
    shown: u64,
    accepted: u64,
    accepted_partial: u64,
    rejected: u64,
    timed_out: u64,
    latency_sum_ms: u64,
    latency_count: u64,
}

impl CompletionStats {
    pub fn record(&self, ev: AcceptanceEvent) {
        let mut inner = self.inner.lock();
        match ev {
            AcceptanceEvent::Shown => {
                inner.shown += 1;
                crate::observability::subsystem_metrics::incr_inline_completion_shown();
            }
            AcceptanceEvent::Accepted => {
                inner.accepted += 1;
                crate::observability::subsystem_metrics::incr_inline_completion_accepted();
            }
            AcceptanceEvent::AcceptedPartial => {
                inner.accepted_partial += 1;
                crate::observability::subsystem_metrics::incr_inline_completion_accepted();
            }
            AcceptanceEvent::Rejected => {
                inner.rejected += 1;
                crate::observability::subsystem_metrics::incr_inline_completion_rejected();
            }
            AcceptanceEvent::TimedOut => {
                inner.timed_out += 1;
                crate::observability::subsystem_metrics::incr_inline_completion_timed_out();
            }
        }
    }

    pub fn record_latency_ms(&self, ms: u64) {
        let mut inner = self.inner.lock();
        inner.latency_sum_ms = inner.latency_sum_ms.saturating_add(ms);
        inner.latency_count = inner.latency_count.saturating_add(1);
    }

    pub fn acceptance_rate(&self) -> f64 {
        let inner = self.inner.lock();
        let shown = inner.shown;
        if shown == 0 {
            return 0.0;
        }
        (inner.accepted + inner.accepted_partial) as f64 / shown as f64
    }

    pub fn average_latency_ms(&self) -> f64 {
        let inner = self.inner.lock();
        if inner.latency_count == 0 {
            0.0
        } else {
            inner.latency_sum_ms as f64 / inner.latency_count as f64
        }
    }

    pub fn snapshot(&self) -> (u64, u64, u64, u64, u64) {
        let inner = self.inner.lock();
        (
            inner.shown,
            inner.accepted,
            inner.accepted_partial,
            inner.rejected,
            inner.timed_out,
        )
    }
}

static GLOBAL: OnceLock<CompletionStats> = OnceLock::new();

pub fn global_stats() -> &'static CompletionStats {
    GLOBAL.get_or_init(CompletionStats::default)
}
