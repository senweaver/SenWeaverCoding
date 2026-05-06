// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Background GC task for memory subsystems.
//!
//! Spawns a `tokio::interval` that periodically:
//!   1. Evicts expired blackboard entries via `Blackboard::evict_expired`.
//!   2. Runs `MultiAgentRuntime::maintenance` (supervisor health checks,
//!      task queue overdue-expiry, coordinator locks/barriers).
//!   3. Records the GC run as a metric so operators can confirm it's alive.
//!
//! Driven by `MemoryRuntimeExtras::gc_interval_secs`.  Call
//! `spawn_memory_gc_task()` once at startup from the CLI entrypoint; the
//! returned `JoinHandle` can be awaited during graceful shutdown.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

use crate::config::domain::MemoryRuntimeExtras;
use crate::runtime::TaskHandle;

#[derive(Debug, Clone, Default)]
pub struct GcStats {
    pub runs: u64,
    pub blackboard_entries_evicted: usize,
    pub tasks_expired: usize,
    pub locks_expired: usize,
    pub last_run_epoch_secs: u64,
}

impl GcStats {
    pub fn is_running(&self) -> bool {
        self.runs > 0
    }
}

pub fn spawn_memory_gc_task(
    config: MemoryRuntimeExtras,
    metrics: Option<Arc<crate::observability::agent_metrics::AgentMetrics>>,
) -> (TaskHandle, CancellationToken) {
    let token = CancellationToken::new();
    let child_token = token.clone();
    let interval_duration = config.gc_interval();

    let handle = crate::runtime::spawn_supervised("memory.gc.main_loop", async move {
        info!(
            interval_secs = config.gc_interval_secs,
            "memory GC task started"
        );
        let mut ticker = tokio::time::interval(interval_duration);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        ticker.tick().await;

        let mut runs = 0u64;
        loop {
            tokio::select! {
                _ = child_token.cancelled() => {
                    info!(runs, "memory GC task stopping on cancellation");
                    break;
                }
                _ = ticker.tick() => {
                    runs += 1;
                    let stats = run_single_cycle();
                    let epoch = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);

                    debug!(
                        run = runs,
                        entries_evicted = stats.entries_evicted,
                        tasks_expired = stats.tasks_expired,
                        locks_expired = stats.locks_expired,
                        "memory GC cycle complete"
                    );

                    if let Some(ref m) = metrics {
                        use crate::observability::agent_metrics::LabelSet;
                        m.inc_by("sen_gc_runs_total", LabelSet::new(vec![]), 1);
                        m.inc_by(
                            "sen_gc_blackboard_entries_evicted_total",
                            LabelSet::new(vec![]),
                            stats.entries_evicted as u64,
                        );
                        m.inc_by(
                            "sen_gc_tasks_expired_total",
                            LabelSet::new(vec![]),
                            stats.tasks_expired as u64,
                        );
                        m.set_gauge(
                            "sen_gc_last_run_epoch_secs",
                            LabelSet::new(vec![]),
                            epoch as f64,
                        );
                    }
                }
            }
        }
    });

    (handle, token)
}

struct CycleStats {
    entries_evicted: usize,
    tasks_expired: usize,
    locks_expired: usize,
}

fn run_single_cycle() -> CycleStats {
    let rt = crate::agent::multi_agent_runtime::global_runtime();
    match rt {
        Some(rt) => {
            let report = rt.maintenance();
            CycleStats {
                entries_evicted: report.expired_entries,
                tasks_expired: report.expired_tasks,
                locks_expired: report.expired_locks,
            }
        }
        None => CycleStats {
            entries_evicted: 0,
            tasks_expired: 0,
            locks_expired: 0,
        },
    }
}
