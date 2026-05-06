// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Code-intelligence metrics — apply-model refine loop, CodeEditFlow,
//! context build, symbol-graph sync, LSP didChange/hover, apply-hunk
//! anchors, context preserve tags.
//!
//! | # | Metric                                                | Type    |
//! |---|-------------------------------------------------------|---------|
//! | 1 | `sen_apply_model_refine_attempts_total`               | counter |
//! | 2 | `sen_apply_model_refine_success_total`                | counter |
//! | 3 | `sen_apply_model_refine_failed_total`                 | counter |
//! | 4 | `sen_apply_model_refine_recursive_attempt_total`      | counter |
//!
//! These are incremented by [`crate::apply_model::llm_refine::HttpLlmRefiner`]
//! and [`crate::inline_edit::runner::InlineEditRunner`] along the
//! validator-failure → refine retry path.
//!
//! ## `CodeEditFlow` counters
//!
//! Twelve additional counters expose the rewritten `CodeEditFlow`
//! lifecycle (planner attempts / fallback, diff vs full-file
//! executor branches, fix-loop attempts, layered runner activity,
//! review-noop short-circuits, batch verification verdicts).  All
//! follow the `sen_code_edit_*_total` naming convention so the
//! Prometheus scrape can filter them with a single regex.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct CodeIntelMetrics {
    pub apply_model_refine_attempts: AtomicU64,
    pub apply_model_refine_success: AtomicU64,
    pub apply_model_refine_failed: AtomicU64,

    pub apply_model_refine_recursive_attempts: AtomicU64,

    pub code_edit_plan_attempts: AtomicU64,

    pub code_edit_plan_retry: AtomicU64,

    pub code_edit_plan_degraded: AtomicU64,

    pub code_edit_auto_expanded_steps: AtomicU64,

    pub code_edit_diff_applied: AtomicU64,

    pub code_edit_full_file_fallback: AtomicU64,

    pub code_edit_fix_attempt: AtomicU64,

    pub code_edit_parallel_layer_run: AtomicU64,

    pub code_edit_parallel_step_run: AtomicU64,

    pub code_edit_review_noop: AtomicU64,

    pub code_edit_batch_verify_pass: AtomicU64,

    pub code_edit_batch_verify_fail: AtomicU64,

    pub context_build_success: AtomicU64,

    pub context_build_empty_sources: AtomicU64,

    pub context_budget_compression_step: AtomicU64,

    pub symbol_graph_sync_scheduled: AtomicU64,

    pub symbol_graph_sync_executed: AtomicU64,

    pub symbol_graph_sync_debounced: AtomicU64,

    pub lsp_did_change_sent: AtomicU64,

    pub lsp_hover_fetched: AtomicU64,

    pub lsp_hover_timeout: AtomicU64,

    pub apply_hunk_with_anchor: AtomicU64,

    pub apply_hunk_anchor_hit_named_scope: AtomicU64,

    pub apply_hunk_anchor_fallback_full_scan: AtomicU64,

    pub context_preserve_hit: AtomicU64,

    pub context_preserve_skip_compress: AtomicU64,
}

impl CodeIntelMetrics {
    pub fn snapshot(&self) -> CodeIntelSnapshot {
        CodeIntelSnapshot {
            apply_model_refine_attempts: self.apply_model_refine_attempts.load(Ordering::Relaxed),
            apply_model_refine_success: self.apply_model_refine_success.load(Ordering::Relaxed),
            apply_model_refine_failed: self.apply_model_refine_failed.load(Ordering::Relaxed),
            apply_model_refine_recursive_attempts: self
                .apply_model_refine_recursive_attempts
                .load(Ordering::Relaxed),
            code_edit_plan_attempts: self.code_edit_plan_attempts.load(Ordering::Relaxed),
            code_edit_plan_retry: self.code_edit_plan_retry.load(Ordering::Relaxed),
            code_edit_plan_degraded: self.code_edit_plan_degraded.load(Ordering::Relaxed),
            code_edit_auto_expanded_steps: self
                .code_edit_auto_expanded_steps
                .load(Ordering::Relaxed),
            code_edit_diff_applied: self.code_edit_diff_applied.load(Ordering::Relaxed),
            code_edit_full_file_fallback: self
                .code_edit_full_file_fallback
                .load(Ordering::Relaxed),
            code_edit_fix_attempt: self.code_edit_fix_attempt.load(Ordering::Relaxed),
            code_edit_parallel_layer_run: self
                .code_edit_parallel_layer_run
                .load(Ordering::Relaxed),
            code_edit_parallel_step_run: self
                .code_edit_parallel_step_run
                .load(Ordering::Relaxed),
            code_edit_review_noop: self.code_edit_review_noop.load(Ordering::Relaxed),
            code_edit_batch_verify_pass: self
                .code_edit_batch_verify_pass
                .load(Ordering::Relaxed),
            code_edit_batch_verify_fail: self
                .code_edit_batch_verify_fail
                .load(Ordering::Relaxed),
            context_build_success: self.context_build_success.load(Ordering::Relaxed),
            context_build_empty_sources: self
                .context_build_empty_sources
                .load(Ordering::Relaxed),
            context_budget_compression_step: self
                .context_budget_compression_step
                .load(Ordering::Relaxed),
            symbol_graph_sync_scheduled: self
                .symbol_graph_sync_scheduled
                .load(Ordering::Relaxed),
            symbol_graph_sync_executed: self
                .symbol_graph_sync_executed
                .load(Ordering::Relaxed),
            symbol_graph_sync_debounced: self
                .symbol_graph_sync_debounced
                .load(Ordering::Relaxed),
            lsp_did_change_sent: self.lsp_did_change_sent.load(Ordering::Relaxed),
            lsp_hover_fetched: self.lsp_hover_fetched.load(Ordering::Relaxed),
            lsp_hover_timeout: self.lsp_hover_timeout.load(Ordering::Relaxed),
            apply_hunk_with_anchor: self.apply_hunk_with_anchor.load(Ordering::Relaxed),
            apply_hunk_anchor_hit_named_scope: self
                .apply_hunk_anchor_hit_named_scope
                .load(Ordering::Relaxed),
            apply_hunk_anchor_fallback_full_scan: self
                .apply_hunk_anchor_fallback_full_scan
                .load(Ordering::Relaxed),
            context_preserve_hit: self.context_preserve_hit.load(Ordering::Relaxed),
            context_preserve_skip_compress: self
                .context_preserve_skip_compress
                .load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CodeIntelSnapshot {
    pub apply_model_refine_attempts: u64,
    pub apply_model_refine_success: u64,
    pub apply_model_refine_failed: u64,
    pub apply_model_refine_recursive_attempts: u64,
    pub code_edit_plan_attempts: u64,
    pub code_edit_plan_retry: u64,
    pub code_edit_plan_degraded: u64,
    pub code_edit_auto_expanded_steps: u64,
    pub code_edit_diff_applied: u64,
    pub code_edit_full_file_fallback: u64,
    pub code_edit_fix_attempt: u64,
    pub code_edit_parallel_layer_run: u64,
    pub code_edit_parallel_step_run: u64,
    pub code_edit_review_noop: u64,
    pub code_edit_batch_verify_pass: u64,
    pub code_edit_batch_verify_fail: u64,
    pub context_build_success: u64,
    pub context_build_empty_sources: u64,
    pub context_budget_compression_step: u64,
    pub symbol_graph_sync_scheduled: u64,
    pub symbol_graph_sync_executed: u64,
    pub symbol_graph_sync_debounced: u64,
    pub lsp_did_change_sent: u64,
    pub lsp_hover_fetched: u64,
    pub lsp_hover_timeout: u64,
    pub apply_hunk_with_anchor: u64,
    pub apply_hunk_anchor_hit_named_scope: u64,
    pub apply_hunk_anchor_fallback_full_scan: u64,
    pub context_preserve_hit: u64,
    pub context_preserve_skip_compress: u64,
}

impl CodeIntelSnapshot {
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
        counter!(
            "sen_apply_model_refine_attempts_total",
            self.apply_model_refine_attempts
        );
        counter!(
            "sen_apply_model_refine_success_total",
            self.apply_model_refine_success
        );
        counter!(
            "sen_apply_model_refine_failed_total",
            self.apply_model_refine_failed
        );
        counter!(
            "sen_apply_model_refine_recursive_attempt_total",
            self.apply_model_refine_recursive_attempts
        );

        counter!(
            "sen_code_edit_plan_attempts_total",
            self.code_edit_plan_attempts
        );
        counter!(
            "sen_code_edit_plan_retry_total",
            self.code_edit_plan_retry
        );
        counter!(
            "sen_code_edit_plan_degraded_total",
            self.code_edit_plan_degraded
        );
        counter!(
            "sen_code_edit_auto_expanded_steps_total",
            self.code_edit_auto_expanded_steps
        );
        counter!(
            "sen_code_edit_diff_applied_total",
            self.code_edit_diff_applied
        );
        counter!(
            "sen_code_edit_full_file_fallback_total",
            self.code_edit_full_file_fallback
        );
        counter!(
            "sen_code_edit_fix_attempt_total",
            self.code_edit_fix_attempt
        );
        counter!(
            "sen_code_edit_parallel_layer_run_total",
            self.code_edit_parallel_layer_run
        );
        counter!(
            "sen_code_edit_parallel_step_run_total",
            self.code_edit_parallel_step_run
        );
        counter!(
            "sen_code_edit_review_noop_total",
            self.code_edit_review_noop
        );
        counter!(
            "sen_code_edit_batch_verify_pass_total",
            self.code_edit_batch_verify_pass
        );
        counter!(
            "sen_code_edit_batch_verify_fail_total",
            self.code_edit_batch_verify_fail
        );

        counter!("sen_context_build_success_total", self.context_build_success);
        counter!(
            "sen_context_build_empty_sources_total",
            self.context_build_empty_sources
        );
        counter!(
            "sen_context_budget_compression_step_total",
            self.context_budget_compression_step
        );
        counter!(
            "sen_symbol_graph_sync_scheduled_total",
            self.symbol_graph_sync_scheduled
        );
        counter!(
            "sen_symbol_graph_sync_executed_total",
            self.symbol_graph_sync_executed
        );
        counter!(
            "sen_symbol_graph_sync_debounced_total",
            self.symbol_graph_sync_debounced
        );
        counter!("sen_lsp_did_change_sent_total", self.lsp_did_change_sent);
        counter!("sen_lsp_hover_fetched_total", self.lsp_hover_fetched);
        counter!("sen_lsp_hover_timeout_total", self.lsp_hover_timeout);
        counter!(
            "sen_apply_hunk_with_anchor_total",
            self.apply_hunk_with_anchor
        );
        counter!(
            "sen_apply_hunk_anchor_hit_named_scope_total",
            self.apply_hunk_anchor_hit_named_scope
        );
        counter!(
            "sen_apply_hunk_anchor_fallback_full_scan_total",
            self.apply_hunk_anchor_fallback_full_scan
        );
        counter!("sen_context_preserve_hit_total", self.context_preserve_hit);
        counter!(
            "sen_context_preserve_skip_compress_total",
            self.context_preserve_skip_compress
        );
        out
    }
}

static METRICS: OnceLock<CodeIntelMetrics> = OnceLock::new();

pub fn global() -> &'static CodeIntelMetrics {
    METRICS.get_or_init(CodeIntelMetrics::default)
}

pub fn incr_apply_model_refine_attempt() {
    global()
        .apply_model_refine_attempts
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_apply_model_refine_success() {
    global()
        .apply_model_refine_success
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_apply_model_refine_failed() {
    global()
        .apply_model_refine_failed
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_apply_model_refine_recursive_attempt() {
    global()
        .apply_model_refine_recursive_attempts
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_code_edit_plan_attempt() {
    global()
        .code_edit_plan_attempts
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_code_edit_plan_retry() {
    global().code_edit_plan_retry.fetch_add(1, Ordering::Relaxed);
}

pub fn incr_code_edit_plan_degraded() {
    global()
        .code_edit_plan_degraded
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_code_edit_auto_expanded_steps(n: u64) {
    if n == 0 {
        return;
    }
    global()
        .code_edit_auto_expanded_steps
        .fetch_add(n, Ordering::Relaxed);
}

pub fn incr_code_edit_diff_applied() {
    global()
        .code_edit_diff_applied
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_code_edit_full_file_fallback() {
    global()
        .code_edit_full_file_fallback
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_code_edit_fix_attempt() {
    global()
        .code_edit_fix_attempt
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_code_edit_parallel_layer_run() {
    global()
        .code_edit_parallel_layer_run
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_code_edit_parallel_step_run() {
    global()
        .code_edit_parallel_step_run
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_code_edit_review_noop() {
    global()
        .code_edit_review_noop
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_code_edit_batch_verify_pass() {
    global()
        .code_edit_batch_verify_pass
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_code_edit_batch_verify_fail() {
    global()
        .code_edit_batch_verify_fail
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_context_build_success() {
    global()
        .context_build_success
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_context_build_empty_sources() {
    global()
        .context_build_empty_sources
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_context_budget_compression_step() {
    global()
        .context_budget_compression_step
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_symbol_graph_sync_scheduled() {
    global()
        .symbol_graph_sync_scheduled
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_symbol_graph_sync_executed() {
    global()
        .symbol_graph_sync_executed
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_symbol_graph_sync_debounced() {
    global()
        .symbol_graph_sync_debounced
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_lsp_did_change_sent() {
    global()
        .lsp_did_change_sent
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_lsp_hover_fetched() {
    global().lsp_hover_fetched.fetch_add(1, Ordering::Relaxed);
}

pub fn incr_lsp_hover_timeout() {
    global().lsp_hover_timeout.fetch_add(1, Ordering::Relaxed);
}

pub fn incr_apply_hunk_with_anchor() {
    global()
        .apply_hunk_with_anchor
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_apply_hunk_anchor_hit_named_scope() {
    global()
        .apply_hunk_anchor_hit_named_scope
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_apply_hunk_anchor_fallback_full_scan() {
    global()
        .apply_hunk_anchor_fallback_full_scan
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_context_preserve_hit(_tag: &str) {
    global()
        .context_preserve_hit
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_context_preserve_skip_compress() {
    global()
        .context_preserve_skip_compress
        .fetch_add(1, Ordering::Relaxed);
}
