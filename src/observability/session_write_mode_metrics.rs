// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

pub const EVALS_HISTOGRAM_BUCKETS: [f64; 10] =
    [0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0, 180.0, 600.0, 900.0];

#[derive(Debug, Default)]
pub struct SessionWriteModeMetrics {
    pub write_mode_plans: AtomicU64,
    pub write_mode_plan_ok: AtomicU64,
    pub write_mode_plan_steps: AtomicU64,
    pub write_mode_step_executions: AtomicU64,
    pub write_mode_verify_pass: AtomicU64,
    pub write_mode_verify_fail: AtomicU64,

    pub write_mode_apply_verify_pass: AtomicU64,

    pub write_mode_apply_verify_refine: AtomicU64,

    pub write_mode_apply_verify_rollback: AtomicU64,
    pub diff_session_applied: AtomicU64,
    pub diff_session_rollbacks: AtomicU64,

    pub evals_suite_seconds: Mutex<std::collections::BTreeMap<String, EvalsHistogram>>,

    pub session_event_persisted: AtomicU64,

    pub session_snapshot_written: AtomicU64,

    pub session_replayed: AtomicU64,

    pub session_apply_failed: AtomicU64,

    pub chat_view_reduce_cli: AtomicU64,
    pub chat_view_reduce_tui: AtomicU64,
    pub chat_view_reduce_gui: AtomicU64,

    pub session_hub_subscribers: AtomicU64,

    pub session_hub_active_sessions: AtomicU64,

    pub checkpoint_persisted: AtomicU64,

    pub checkpoint_rollback_via_edit_history: AtomicU64,

    pub checkpoint_backend_error: AtomicU64,

    pub approval_routed_via_session: AtomicU64,

    pub approval_responded_via_session: AtomicU64,

    pub session_rpc_send_total: AtomicU64,

    pub session_rpc_recv_total: AtomicU64,

    pub session_rpc_conflict_resolved_total: AtomicU64,

    pub token_budget_project_loc: AtomicU64,
}

#[derive(Debug, Clone, Default)]
pub struct EvalsHistogram {
    pub bucket_counts: [u64; EVALS_HISTOGRAM_BUCKETS.len()],
    pub inf_count: u64,
    pub sum_seconds: f64,
    pub count: u64,
}

impl EvalsHistogram {
    pub fn observe(&mut self, seconds: f64) {
        for (idx, boundary) in EVALS_HISTOGRAM_BUCKETS.iter().enumerate() {
            if seconds <= *boundary {
                self.bucket_counts[idx] += 1;
            }
        }
        self.inf_count += 1;
        self.sum_seconds += seconds;
        self.count += 1;
    }
}

impl SessionWriteModeMetrics {
    pub fn snapshot(&self) -> SessionWriteModeSnapshot {
        SessionWriteModeSnapshot {
            write_mode_plans: self.write_mode_plans.load(Ordering::Relaxed),
            write_mode_plan_ok: self.write_mode_plan_ok.load(Ordering::Relaxed),
            write_mode_plan_steps: self.write_mode_plan_steps.load(Ordering::Relaxed),
            write_mode_step_executions: self.write_mode_step_executions.load(Ordering::Relaxed),
            write_mode_verify_pass: self.write_mode_verify_pass.load(Ordering::Relaxed),
            write_mode_verify_fail: self.write_mode_verify_fail.load(Ordering::Relaxed),
            write_mode_apply_verify_pass: self
                .write_mode_apply_verify_pass
                .load(Ordering::Relaxed),
            write_mode_apply_verify_refine: self
                .write_mode_apply_verify_refine
                .load(Ordering::Relaxed),
            write_mode_apply_verify_rollback: self
                .write_mode_apply_verify_rollback
                .load(Ordering::Relaxed),
            diff_session_applied: self.diff_session_applied.load(Ordering::Relaxed),
            diff_session_rollbacks: self.diff_session_rollbacks.load(Ordering::Relaxed),
            evals_suite_seconds: self
                .evals_suite_seconds
                .lock()
                .map(|guard| guard.clone())
                .unwrap_or_default(),
            session_event_persisted: self.session_event_persisted.load(Ordering::Relaxed),
            session_snapshot_written: self.session_snapshot_written.load(Ordering::Relaxed),
            session_replayed: self.session_replayed.load(Ordering::Relaxed),
            session_apply_failed: self.session_apply_failed.load(Ordering::Relaxed),
            chat_view_reduce_cli: self.chat_view_reduce_cli.load(Ordering::Relaxed),
            chat_view_reduce_tui: self.chat_view_reduce_tui.load(Ordering::Relaxed),
            chat_view_reduce_gui: self.chat_view_reduce_gui.load(Ordering::Relaxed),
            session_hub_subscribers: self.session_hub_subscribers.load(Ordering::Relaxed),
            session_hub_active_sessions: self.session_hub_active_sessions.load(Ordering::Relaxed),
            checkpoint_persisted: self.checkpoint_persisted.load(Ordering::Relaxed),
            checkpoint_rollback_via_edit_history: self
                .checkpoint_rollback_via_edit_history
                .load(Ordering::Relaxed),
            checkpoint_backend_error: self.checkpoint_backend_error.load(Ordering::Relaxed),
            approval_routed_via_session: self.approval_routed_via_session.load(Ordering::Relaxed),
            approval_responded_via_session: self
                .approval_responded_via_session
                .load(Ordering::Relaxed),
            session_rpc_send_total: self.session_rpc_send_total.load(Ordering::Relaxed),
            session_rpc_recv_total: self.session_rpc_recv_total.load(Ordering::Relaxed),
            session_rpc_conflict_resolved_total: self
                .session_rpc_conflict_resolved_total
                .load(Ordering::Relaxed),
            token_budget_project_loc: self.token_budget_project_loc.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SessionWriteModeSnapshot {
    pub write_mode_plans: u64,
    pub write_mode_plan_ok: u64,
    pub write_mode_plan_steps: u64,
    pub write_mode_step_executions: u64,
    pub write_mode_verify_pass: u64,
    pub write_mode_verify_fail: u64,
    pub write_mode_apply_verify_pass: u64,
    pub write_mode_apply_verify_refine: u64,
    pub write_mode_apply_verify_rollback: u64,
    pub diff_session_applied: u64,
    pub diff_session_rollbacks: u64,
    pub evals_suite_seconds: std::collections::BTreeMap<String, EvalsHistogram>,

    pub session_event_persisted: u64,
    pub session_snapshot_written: u64,
    pub session_replayed: u64,
    pub session_apply_failed: u64,
    pub chat_view_reduce_cli: u64,
    pub chat_view_reduce_tui: u64,
    pub chat_view_reduce_gui: u64,
    pub session_hub_subscribers: u64,
    pub session_hub_active_sessions: u64,
    pub checkpoint_persisted: u64,
    pub checkpoint_rollback_via_edit_history: u64,
    pub checkpoint_backend_error: u64,
    pub approval_routed_via_session: u64,
    pub approval_responded_via_session: u64,

    pub session_rpc_send_total: u64,
    pub session_rpc_recv_total: u64,
    pub session_rpc_conflict_resolved_total: u64,

    pub token_budget_project_loc: u64,
}

impl SessionWriteModeSnapshot {
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
        counter!("sen_write_mode_plans_total", self.write_mode_plans);
        counter!("sen_write_mode_plan_ok_total", self.write_mode_plan_ok);
        counter!("sen_write_mode_steps_total", self.write_mode_plan_steps);
        counter!(
            "sen_write_mode_step_executions_total",
            self.write_mode_step_executions
        );
        counter!(
            "sen_write_mode_verify_pass_total",
            self.write_mode_verify_pass
        );
        counter!(
            "sen_write_mode_verify_fail_total",
            self.write_mode_verify_fail
        );
        counter!(
            "sen_write_mode_apply_verify_pass_total",
            self.write_mode_apply_verify_pass
        );
        counter!(
            "sen_write_mode_apply_verify_refine_total",
            self.write_mode_apply_verify_refine
        );
        counter!(
            "sen_write_mode_apply_verify_rollback_total",
            self.write_mode_apply_verify_rollback
        );
        counter!("sen_diff_session_applied_total", self.diff_session_applied);
        counter!(
            "sen_diff_session_rollbacks_total",
            self.diff_session_rollbacks
        );

        counter!(
            "sen_session_event_persisted_total",
            self.session_event_persisted
        );
        counter!(
            "sen_session_snapshot_written_total",
            self.session_snapshot_written
        );
        counter!("sen_session_replayed_total", self.session_replayed);
        counter!("sen_session_apply_failed_total", self.session_apply_failed);
        counter!("sen_chat_view_reduce_cli_total", self.chat_view_reduce_cli);
        counter!("sen_chat_view_reduce_tui_total", self.chat_view_reduce_tui);
        counter!("sen_chat_view_reduce_gui_total", self.chat_view_reduce_gui);
        counter!(
            "sen_session_hub_subscribers_total",
            self.session_hub_subscribers
        );
        out.push_str(&format!(
            "# TYPE sen_session_hub_active_sessions gauge\nsen_session_hub_active_sessions {}\n",
            self.session_hub_active_sessions
        ));
        counter!(
            "sen_checkpoint_persisted_total",
            self.checkpoint_persisted
        );
        counter!(
            "sen_checkpoint_rollback_via_edit_history_total",
            self.checkpoint_rollback_via_edit_history
        );
        counter!(
            "sen_checkpoint_backend_error_total",
            self.checkpoint_backend_error
        );
        counter!(
            "sen_approval_routed_via_session_total",
            self.approval_routed_via_session
        );
        counter!(
            "sen_approval_responded_via_session_total",
            self.approval_responded_via_session
        );
        counter!(
            "sen_session_rpc_send_total",
            self.session_rpc_send_total
        );
        counter!(
            "sen_session_rpc_recv_total",
            self.session_rpc_recv_total
        );
        counter!(
            "sen_session_rpc_conflict_resolved_total",
            self.session_rpc_conflict_resolved_total
        );

        out.push_str(&format!(
            "# TYPE sen_token_budget_project_loc gauge\nsen_token_budget_project_loc {}\n",
            self.token_budget_project_loc
        ));

        if !self.evals_suite_seconds.is_empty() {
            out.push_str("# TYPE sen_evals_suite_seconds histogram\n");
            for (suite, h) in &self.evals_suite_seconds {
                for (idx, boundary) in EVALS_HISTOGRAM_BUCKETS.iter().enumerate() {
                    out.push_str(&format!(
                        "sen_evals_suite_seconds_bucket{{suite=\"{suite}\",le=\"{boundary}\"}} {}\n",
                        h.bucket_counts[idx]
                    ));
                }
                out.push_str(&format!(
                    "sen_evals_suite_seconds_bucket{{suite=\"{suite}\",le=\"+Inf\"}} {}\n",
                    h.inf_count
                ));
                out.push_str(&format!(
                    "sen_evals_suite_seconds_sum{{suite=\"{suite}\"}} {}\n",
                    h.sum_seconds
                ));
                out.push_str(&format!(
                    "sen_evals_suite_seconds_count{{suite=\"{suite}\"}} {}\n",
                    h.count
                ));
            }
        }

        out
    }
}

static METRICS: OnceLock<SessionWriteModeMetrics> = OnceLock::new();

pub fn global() -> &'static SessionWriteModeMetrics {
    METRICS.get_or_init(SessionWriteModeMetrics::default)
}

pub fn incr_write_mode_plan() {
    global().write_mode_plans.fetch_add(1, Ordering::Relaxed);
}
pub fn incr_write_mode_plan_ok() {
    global().write_mode_plan_ok.fetch_add(1, Ordering::Relaxed);
}
pub fn add_write_mode_steps(n: u64) {
    global()
        .write_mode_plan_steps
        .fetch_add(n, Ordering::Relaxed);
}
pub fn incr_write_mode_step() {
    global()
        .write_mode_step_executions
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_write_mode_verify_pass() {
    global()
        .write_mode_verify_pass
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_write_mode_verify_fail() {
    global()
        .write_mode_verify_fail
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_write_mode_apply_verify_pass() {
    global()
        .write_mode_apply_verify_pass
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_write_mode_apply_verify_refine() {
    global()
        .write_mode_apply_verify_refine
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_write_mode_apply_verify_rollback() {
    global()
        .write_mode_apply_verify_rollback
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_diff_session_applied() {
    global()
        .diff_session_applied
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_diff_session_rollback() {
    global()
        .diff_session_rollbacks
        .fetch_add(1, Ordering::Relaxed);
}

pub fn observe_evals_suite_seconds(suite: &str, seconds: f64) {
    if let Ok(mut guard) = global().evals_suite_seconds.lock() {
        guard.entry(suite.to_string()).or_default().observe(seconds);
    }
}

pub fn incr_session_event_persisted() {
    global()
        .session_event_persisted
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_session_snapshot_written() {
    global()
        .session_snapshot_written
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_session_replayed() {
    global().session_replayed.fetch_add(1, Ordering::Relaxed);
}

pub fn incr_session_apply_failed() {
    global()
        .session_apply_failed
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_chat_view_reduce_cli() {
    global()
        .chat_view_reduce_cli
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_chat_view_reduce_tui() {
    global()
        .chat_view_reduce_tui
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_chat_view_reduce_gui() {
    global()
        .chat_view_reduce_gui
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_session_hub_subscribers() {
    global()
        .session_hub_subscribers
        .fetch_add(1, Ordering::Relaxed);
}

pub fn set_session_hub_active_sessions(n: u64) {
    global()
        .session_hub_active_sessions
        .store(n, Ordering::Relaxed);
}

pub fn incr_checkpoint_persisted() {
    global()
        .checkpoint_persisted
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_checkpoint_rollback_via_edit_history() {
    global()
        .checkpoint_rollback_via_edit_history
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_checkpoint_backend_error() {
    global()
        .checkpoint_backend_error
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_approval_routed_via_session() {
    global()
        .approval_routed_via_session
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_approval_responded_via_session() {
    global()
        .approval_responded_via_session
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_session_rpc_send() {
    global()
        .session_rpc_send_total
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_session_rpc_recv() {
    global()
        .session_rpc_recv_total
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_session_rpc_conflict_resolved() {
    global()
        .session_rpc_conflict_resolved_total
        .fetch_add(1, Ordering::Relaxed);
}

pub fn set_token_budget_project_loc(v: u64) {
    global()
        .token_budget_project_loc
        .store(v, Ordering::Relaxed);
}
