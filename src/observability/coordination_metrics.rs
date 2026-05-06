// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Multi-agent coordination metrics.
//!
//! Covers four observable event families introduced for the
//! multi-agent coordination subsystem:
//!
//! 1. **`LockManager` region acquisitions** — every
//!    [`crate::agent::coordination::LockManager::acquire_region`]
//!    invocation classifies the outcome as `ok / conflict / deadlock /
//!    timeout` and bumps the matching counter.
//! 2. **Blackboard message-bus** — broadcasts that landed
//!    (`published`), subscribers that received them (`delivered`),
//!    subscribers that fell behind (`lagged`), and journal-replay
//!    deliveries that recovered missing events (`replayed`).
//! 3. **`delegate_parallel`** — explicit fallback semantics expose
//!    three new failure modes: `no_runtime`, `no_capability`,
//!    `fallback`.
//! 4. **CRDT proof-of-concept** — only populated when the
//!    `crdt-coordination` feature is on, but the counters are always
//!    declared so the Prometheus surface is stable.
//!
//! The module mirrors the [`super::subsystem_metrics`] /
//! [`super::code_intel_metrics`] structure: a process-global
//! [`OnceLock`]-backed registry of [`AtomicU64`] counters, an
//! `incr_*` helper per counter, and a
//! [`CoordinationSnapshot::render_prometheus_text`] method that the
//! main Prometheus encoder appends to its output.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy)]
pub enum LockAcquireOutcome {
    Ok,
    Conflict,
    Deadlock,
    Timeout,
}

#[derive(Debug, Default)]
pub struct CoordinationMetrics {

    pub lockmgr_region_acquire_ok: AtomicU64,
    pub lockmgr_region_acquire_conflict: AtomicU64,
    pub lockmgr_region_acquire_deadlock: AtomicU64,
    pub lockmgr_region_acquire_timeout: AtomicU64,
    pub lockmgr_region_release: AtomicU64,
    pub lockmgr_deadlock_detected: AtomicU64,

    pub blackboard_published: AtomicU64,
    pub blackboard_delivered: AtomicU64,
    pub blackboard_lagged: AtomicU64,
    pub blackboard_replayed: AtomicU64,

    pub delegate_parallel_no_runtime: AtomicU64,
    pub delegate_parallel_no_capability: AtomicU64,
    pub delegate_parallel_fallback: AtomicU64,

    pub crdt_local_ops: AtomicU64,
    pub crdt_remote_updates: AtomicU64,
}

impl CoordinationMetrics {
    pub fn snapshot(&self) -> CoordinationSnapshot {
        CoordinationSnapshot {
            lockmgr_region_acquire_ok: self.lockmgr_region_acquire_ok.load(Ordering::Relaxed),
            lockmgr_region_acquire_conflict: self
                .lockmgr_region_acquire_conflict
                .load(Ordering::Relaxed),
            lockmgr_region_acquire_deadlock: self
                .lockmgr_region_acquire_deadlock
                .load(Ordering::Relaxed),
            lockmgr_region_acquire_timeout: self
                .lockmgr_region_acquire_timeout
                .load(Ordering::Relaxed),
            lockmgr_region_release: self.lockmgr_region_release.load(Ordering::Relaxed),
            lockmgr_deadlock_detected: self.lockmgr_deadlock_detected.load(Ordering::Relaxed),
            blackboard_published: self.blackboard_published.load(Ordering::Relaxed),
            blackboard_delivered: self.blackboard_delivered.load(Ordering::Relaxed),
            blackboard_lagged: self.blackboard_lagged.load(Ordering::Relaxed),
            blackboard_replayed: self.blackboard_replayed.load(Ordering::Relaxed),
            delegate_parallel_no_runtime: self
                .delegate_parallel_no_runtime
                .load(Ordering::Relaxed),
            delegate_parallel_no_capability: self
                .delegate_parallel_no_capability
                .load(Ordering::Relaxed),
            delegate_parallel_fallback: self.delegate_parallel_fallback.load(Ordering::Relaxed),
            crdt_local_ops: self.crdt_local_ops.load(Ordering::Relaxed),
            crdt_remote_updates: self.crdt_remote_updates.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CoordinationSnapshot {
    pub lockmgr_region_acquire_ok: u64,
    pub lockmgr_region_acquire_conflict: u64,
    pub lockmgr_region_acquire_deadlock: u64,
    pub lockmgr_region_acquire_timeout: u64,
    pub lockmgr_region_release: u64,
    pub lockmgr_deadlock_detected: u64,
    pub blackboard_published: u64,
    pub blackboard_delivered: u64,
    pub blackboard_lagged: u64,
    pub blackboard_replayed: u64,
    pub delegate_parallel_no_runtime: u64,
    pub delegate_parallel_no_capability: u64,
    pub delegate_parallel_fallback: u64,
    pub crdt_local_ops: u64,
    pub crdt_remote_updates: u64,
}

impl CoordinationSnapshot {

    pub fn render_prometheus_text(&self) -> String {
        let mut out = String::new();
        macro_rules! counter_label {
            ($metric:literal, $label:literal, $value:literal, $val:expr) => {
                out.push_str(&format!(
                    "# TYPE {name} counter\n{name}{{{lbl}=\"{lv}\"}} {val}\n",
                    name = $metric,
                    lbl = $label,
                    lv = $value,
                    val = $val
                ));
            };
        }
        macro_rules! counter {
            ($metric:literal, $val:expr) => {
                out.push_str(&format!(
                    "# TYPE {name} counter\n{name} {val}\n",
                    name = $metric,
                    val = $val
                ));
            };
        }
        counter_label!(
            "sen_lockmgr_region_acquire_total",
            "outcome",
            "ok",
            self.lockmgr_region_acquire_ok
        );
        counter_label!(
            "sen_lockmgr_region_acquire_total",
            "outcome",
            "conflict",
            self.lockmgr_region_acquire_conflict
        );
        counter_label!(
            "sen_lockmgr_region_acquire_total",
            "outcome",
            "deadlock",
            self.lockmgr_region_acquire_deadlock
        );
        counter_label!(
            "sen_lockmgr_region_acquire_total",
            "outcome",
            "timeout",
            self.lockmgr_region_acquire_timeout
        );
        counter!(
            "sen_lockmgr_region_release_total",
            self.lockmgr_region_release
        );
        counter!(
            "sen_lockmgr_deadlock_detected_total",
            self.lockmgr_deadlock_detected
        );
        counter!("sen_blackboard_published_total", self.blackboard_published);
        counter!("sen_blackboard_delivered_total", self.blackboard_delivered);
        counter!("sen_blackboard_lagged_total", self.blackboard_lagged);
        counter!("sen_blackboard_replayed_total", self.blackboard_replayed);
        counter!(
            "sen_delegate_parallel_no_runtime_total",
            self.delegate_parallel_no_runtime
        );
        counter!(
            "sen_delegate_parallel_no_capability_total",
            self.delegate_parallel_no_capability
        );
        counter!(
            "sen_delegate_parallel_fallback_total",
            self.delegate_parallel_fallback
        );
        counter!("sen_crdt_local_ops_total", self.crdt_local_ops);
        counter!("sen_crdt_remote_updates_total", self.crdt_remote_updates);
        out
    }
}

static METRICS: OnceLock<CoordinationMetrics> = OnceLock::new();

pub fn global() -> &'static CoordinationMetrics {
    METRICS.get_or_init(CoordinationMetrics::default)
}

pub fn incr_lockmgr_acquire(outcome: LockAcquireOutcome) {
    let m = global();
    let counter = match outcome {
        LockAcquireOutcome::Ok => &m.lockmgr_region_acquire_ok,
        LockAcquireOutcome::Conflict => &m.lockmgr_region_acquire_conflict,
        LockAcquireOutcome::Deadlock => &m.lockmgr_region_acquire_deadlock,
        LockAcquireOutcome::Timeout => &m.lockmgr_region_acquire_timeout,
    };
    counter.fetch_add(1, Ordering::Relaxed);
}

pub fn incr_lockmgr_release() {
    global()
        .lockmgr_region_release
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_lockmgr_deadlock_detected() {
    global()
        .lockmgr_deadlock_detected
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_blackboard_published() {
    global().blackboard_published.fetch_add(1, Ordering::Relaxed);
}
pub fn incr_blackboard_delivered() {
    global().blackboard_delivered.fetch_add(1, Ordering::Relaxed);
}
pub fn incr_blackboard_lagged(skipped: u64) {
    global()
        .blackboard_lagged
        .fetch_add(skipped, Ordering::Relaxed);
}
pub fn incr_blackboard_replayed(replayed: u64) {
    global()
        .blackboard_replayed
        .fetch_add(replayed, Ordering::Relaxed);
}

pub fn incr_delegate_parallel_no_runtime() {
    global()
        .delegate_parallel_no_runtime
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_delegate_parallel_no_capability() {
    global()
        .delegate_parallel_no_capability
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_delegate_parallel_fallback() {
    global()
        .delegate_parallel_fallback
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_crdt_local_ops(n: u64) {
    global().crdt_local_ops.fetch_add(n, Ordering::Relaxed);
}
pub fn incr_crdt_remote_updates(n: u64) {
    global().crdt_remote_updates.fetch_add(n, Ordering::Relaxed);
}
