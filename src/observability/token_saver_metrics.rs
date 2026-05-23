// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct TokenSaverMetrics {
    pub invocations: AtomicU64,
    pub passthrough: AtomicU64,
    pub failures_tee: AtomicU64,
    pub raw_bytes: AtomicU64,
    pub compacted_bytes: AtomicU64,
    pub tokens_saved: AtomicU64,
}

impl TokenSaverMetrics {
    pub fn snapshot(&self) -> TokenSaverSnapshot {
        TokenSaverSnapshot {
            invocations: self.invocations.load(Ordering::Relaxed),
            passthrough: self.passthrough.load(Ordering::Relaxed),
            failures_tee: self.failures_tee.load(Ordering::Relaxed),
            raw_bytes: self.raw_bytes.load(Ordering::Relaxed),
            compacted_bytes: self.compacted_bytes.load(Ordering::Relaxed),
            tokens_saved: self.tokens_saved.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct TokenSaverSnapshot {
    pub invocations: u64,
    pub passthrough: u64,
    pub failures_tee: u64,
    pub raw_bytes: u64,
    pub compacted_bytes: u64,
    pub tokens_saved: u64,
}

impl TokenSaverSnapshot {
    pub fn render_prometheus_text(&self) -> String {
        let mut out = String::new();
        for (name, val, help) in [
            (
                "sen_token_saver_invocations_total",
                self.invocations,
                "Compaction calls (FastPath + TOML + Passthrough)",
            ),
            (
                "sen_token_saver_passthrough_total",
                self.passthrough,
                "Compaction calls that hit the passthrough branch",
            ),
            (
                "sen_token_saver_failures_tee_total",
                self.failures_tee,
                "Failed commands whose raw output was teed to disk",
            ),
            (
                "sen_token_saver_raw_bytes_total",
                self.raw_bytes,
                "Sum of raw stdout+stderr byte counts seen by the compactor",
            ),
            (
                "sen_token_saver_compacted_bytes_total",
                self.compacted_bytes,
                "Sum of compacted stdout+stderr byte counts emitted by the compactor",
            ),
            (
                "sen_token_saver_tokens_saved_total",
                self.tokens_saved,
                "Estimated tokens reclaimed by the compactor (raw - compacted)",
            ),
        ] {
            out.push_str(&format!("# HELP {name} {help}\n"));
            out.push_str(&format!("# TYPE {name} counter\n"));
            out.push_str(&format!("{name} {val}\n"));
        }
        out
    }
}

static METRICS: OnceLock<TokenSaverMetrics> = OnceLock::new();

pub fn global() -> &'static TokenSaverMetrics {
    METRICS.get_or_init(TokenSaverMetrics::default)
}

pub fn record_compaction(
    raw_bytes: u64,
    compacted_bytes: u64,
    tokens_saved: u64,
    is_passthrough: bool,
    tee_written: bool,
) {
    let m = global();
    m.invocations.fetch_add(1, Ordering::Relaxed);
    if is_passthrough {
        m.passthrough.fetch_add(1, Ordering::Relaxed);
    }
    if tee_written {
        m.failures_tee.fetch_add(1, Ordering::Relaxed);
    }
    m.raw_bytes.fetch_add(raw_bytes, Ordering::Relaxed);
    m.compacted_bytes.fetch_add(compacted_bytes, Ordering::Relaxed);
    m.tokens_saved.fetch_add(tokens_saved, Ordering::Relaxed);
}
