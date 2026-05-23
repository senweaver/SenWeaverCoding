// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::Instant;

#[derive(Debug, Clone, Copy)]
pub enum TaskTerminalStatus {
    Succeeded,
    Failed,
    Cancelled,
}

impl TaskTerminalStatus {
    fn label(self) -> &'static str {
        match self {
            TaskTerminalStatus::Succeeded => "succeeded",
            TaskTerminalStatus::Failed => "failed",
            TaskTerminalStatus::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TaskPriorityLabel {
    Critical,
    High,
    Normal,
    Low,
    Background,
}

impl TaskPriorityLabel {
    fn label(self) -> &'static str {
        match self {
            TaskPriorityLabel::Critical => "critical",
            TaskPriorityLabel::High => "high",
            TaskPriorityLabel::Normal => "normal",
            TaskPriorityLabel::Low => "low",
            TaskPriorityLabel::Background => "background",
        }
    }
}

pub const MAX_TRACKED_WORKERS: usize = 64;

pub struct SchedulerMetrics {

    pub dag_nodes_total: AtomicU64,
    pub try_claim_miss_total: AtomicU64,
    pub broadcast_lagged_total: AtomicU64,

    pub task_started_critical: AtomicU64,
    pub task_started_high: AtomicU64,
    pub task_started_normal: AtomicU64,
    pub task_started_low: AtomicU64,
    pub task_started_background: AtomicU64,

    pub duration_ms_sum_succeeded: AtomicU64,
    pub duration_ms_sum_failed: AtomicU64,
    pub duration_ms_sum_cancelled: AtomicU64,
    pub duration_count_succeeded: AtomicU64,
    pub duration_count_failed: AtomicU64,
    pub duration_count_cancelled: AtomicU64,

    pub ready_queue_depth: AtomicI64,

    pub steal_events_total: AtomicU64,
    pub worker_busy_nanos: [AtomicU64; MAX_TRACKED_WORKERS],
    pub process_start: Instant,
}

impl Default for SchedulerMetrics {
    fn default() -> Self {
        Self {
            dag_nodes_total: AtomicU64::new(0),
            try_claim_miss_total: AtomicU64::new(0),
            broadcast_lagged_total: AtomicU64::new(0),
            task_started_critical: AtomicU64::new(0),
            task_started_high: AtomicU64::new(0),
            task_started_normal: AtomicU64::new(0),
            task_started_low: AtomicU64::new(0),
            task_started_background: AtomicU64::new(0),
            duration_ms_sum_succeeded: AtomicU64::new(0),
            duration_ms_sum_failed: AtomicU64::new(0),
            duration_ms_sum_cancelled: AtomicU64::new(0),
            duration_count_succeeded: AtomicU64::new(0),
            duration_count_failed: AtomicU64::new(0),
            duration_count_cancelled: AtomicU64::new(0),
            ready_queue_depth: AtomicI64::new(0),
            steal_events_total: AtomicU64::new(0),
            worker_busy_nanos: std::array::from_fn(|_| AtomicU64::new(0)),
            process_start: Instant::now(),
        }
    }
}

impl SchedulerMetrics {
    pub fn snapshot(&self) -> SchedulerMetricsSnapshot {
        let elapsed_ns = self.process_start.elapsed().as_nanos().max(1) as u64;
        let mut worker_utilization = Vec::new();
        for (idx, slot) in self.worker_busy_nanos.iter().enumerate() {
            let busy = slot.load(Ordering::Relaxed);
            if busy == 0 {
                continue;
            }
            let ratio = (busy as f64 / elapsed_ns as f64).min(1.0);
            worker_utilization.push((idx, ratio));
        }
        SchedulerMetricsSnapshot {
            dag_nodes_total: self.dag_nodes_total.load(Ordering::Relaxed),
            try_claim_miss_total: self.try_claim_miss_total.load(Ordering::Relaxed),
            broadcast_lagged_total: self.broadcast_lagged_total.load(Ordering::Relaxed),
            task_started_critical: self.task_started_critical.load(Ordering::Relaxed),
            task_started_high: self.task_started_high.load(Ordering::Relaxed),
            task_started_normal: self.task_started_normal.load(Ordering::Relaxed),
            task_started_low: self.task_started_low.load(Ordering::Relaxed),
            task_started_background: self.task_started_background.load(Ordering::Relaxed),
            duration_ms_sum_succeeded: self.duration_ms_sum_succeeded.load(Ordering::Relaxed),
            duration_ms_sum_failed: self.duration_ms_sum_failed.load(Ordering::Relaxed),
            duration_ms_sum_cancelled: self.duration_ms_sum_cancelled.load(Ordering::Relaxed),
            duration_count_succeeded: self.duration_count_succeeded.load(Ordering::Relaxed),
            duration_count_failed: self.duration_count_failed.load(Ordering::Relaxed),
            duration_count_cancelled: self.duration_count_cancelled.load(Ordering::Relaxed),
            ready_queue_depth: self.ready_queue_depth.load(Ordering::Relaxed),
            steal_events_total: self.steal_events_total.load(Ordering::Relaxed),
            worker_utilization,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SchedulerMetricsSnapshot {
    pub dag_nodes_total: u64,
    pub try_claim_miss_total: u64,
    pub broadcast_lagged_total: u64,
    pub task_started_critical: u64,
    pub task_started_high: u64,
    pub task_started_normal: u64,
    pub task_started_low: u64,
    pub task_started_background: u64,
    pub duration_ms_sum_succeeded: u64,
    pub duration_ms_sum_failed: u64,
    pub duration_ms_sum_cancelled: u64,
    pub duration_count_succeeded: u64,
    pub duration_count_failed: u64,
    pub duration_count_cancelled: u64,
    pub ready_queue_depth: i64,
    pub steal_events_total: u64,

    pub worker_utilization: Vec<(usize, f64)>,
}

impl SchedulerMetricsSnapshot {

    pub fn render_prometheus_text(&self) -> String {
        let mut out = String::new();
        macro_rules! counter {
            ($metric:literal, $val:expr) => {
                out.push_str(&format!(
                    "# TYPE {name} counter\n{name} {val}\n",
                    name = $metric,
                    val = $val
                ));
            };
        }
        macro_rules! gauge {
            ($metric:literal, $val:expr) => {
                out.push_str(&format!(
                    "# TYPE {name} gauge\n{name} {val}\n",
                    name = $metric,
                    val = $val
                ));
            };
        }
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

        counter!("sen_scheduler_dag_nodes_total", self.dag_nodes_total);
        counter!(
            "sen_scheduler_try_claim_miss_total",
            self.try_claim_miss_total
        );
        counter!(
            "sen_scheduler_broadcast_lagged_total",
            self.broadcast_lagged_total
        );

        counter_label!(
            "sen_scheduler_task_started_total",
            "priority",
            "critical",
            self.task_started_critical
        );
        counter_label!(
            "sen_scheduler_task_started_total",
            "priority",
            "high",
            self.task_started_high
        );
        counter_label!(
            "sen_scheduler_task_started_total",
            "priority",
            "normal",
            self.task_started_normal
        );
        counter_label!(
            "sen_scheduler_task_started_total",
            "priority",
            "low",
            self.task_started_low
        );
        counter_label!(
            "sen_scheduler_task_started_total",
            "priority",
            "background",
            self.task_started_background
        );

        counter_label!(
            "sen_scheduler_task_duration_ms_sum",
            "status",
            "succeeded",
            self.duration_ms_sum_succeeded
        );
        counter_label!(
            "sen_scheduler_task_duration_ms_sum",
            "status",
            "failed",
            self.duration_ms_sum_failed
        );
        counter_label!(
            "sen_scheduler_task_duration_ms_sum",
            "status",
            "cancelled",
            self.duration_ms_sum_cancelled
        );
        counter_label!(
            "sen_scheduler_task_duration_count",
            "status",
            "succeeded",
            self.duration_count_succeeded
        );
        counter_label!(
            "sen_scheduler_task_duration_count",
            "status",
            "failed",
            self.duration_count_failed
        );
        counter_label!(
            "sen_scheduler_task_duration_count",
            "status",
            "cancelled",
            self.duration_count_cancelled
        );

        gauge!("sen_scheduler_ready_queue_depth", self.ready_queue_depth);

        counter!("sen_executor_steal_events_total", self.steal_events_total);

        for (idx, ratio) in &self.worker_utilization {
            out.push_str(&format!(
                "# TYPE sen_executor_worker_utilization gauge\nsen_executor_worker_utilization{{worker=\"{idx}\"}} {ratio:.6}\n",
                idx = idx,
                ratio = ratio
            ));
        }

        out
    }
}

static METRICS: OnceLock<SchedulerMetrics> = OnceLock::new();

pub fn global() -> &'static SchedulerMetrics {
    METRICS.get_or_init(SchedulerMetrics::default)
}

pub fn incr_dag_nodes(count: u64) {
    global().dag_nodes_total.fetch_add(count, Ordering::Relaxed);
}

pub fn incr_try_claim_miss() {
    global()
        .try_claim_miss_total
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_broadcast_lagged(skipped: u64) {
    global()
        .broadcast_lagged_total
        .fetch_add(skipped, Ordering::Relaxed);
}

pub fn incr_task_started(priority: TaskPriorityLabel) {
    let m = global();
    let counter = match priority {
        TaskPriorityLabel::Critical => &m.task_started_critical,
        TaskPriorityLabel::High => &m.task_started_high,
        TaskPriorityLabel::Normal => &m.task_started_normal,
        TaskPriorityLabel::Low => &m.task_started_low,
        TaskPriorityLabel::Background => &m.task_started_background,
    };
    counter.fetch_add(1, Ordering::Relaxed);
    let _ = priority.label();
}

pub fn record_task_duration_ms(status: TaskTerminalStatus, ms: u64) {
    let m = global();
    let (sum, count) = match status {
        TaskTerminalStatus::Succeeded => {
            (&m.duration_ms_sum_succeeded, &m.duration_count_succeeded)
        }
        TaskTerminalStatus::Failed => (&m.duration_ms_sum_failed, &m.duration_count_failed),
        TaskTerminalStatus::Cancelled => {
            (&m.duration_ms_sum_cancelled, &m.duration_count_cancelled)
        }
    };
    sum.fetch_add(ms, Ordering::Relaxed);
    count.fetch_add(1, Ordering::Relaxed);
    let _ = status.label();
}

pub fn set_ready_queue_depth(depth: i64) {
    global().ready_queue_depth.store(depth, Ordering::Relaxed);
}

pub fn add_ready_queue_depth(delta: i64) {
    global()
        .ready_queue_depth
        .fetch_add(delta, Ordering::Relaxed);
}

pub fn incr_steal_events(n: u64) {
    global()
        .steal_events_total
        .fetch_add(n, Ordering::Relaxed);
}

pub fn add_worker_busy_nanos(worker_idx: usize, nanos: u64) {
    let idx = worker_idx.min(MAX_TRACKED_WORKERS - 1);
    global().worker_busy_nanos[idx].fetch_add(nanos, Ordering::Relaxed);
}
