// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct SubsystemMetrics {
    pub inline_completion_requests: AtomicU64,
    pub inline_completion_cache_hits: AtomicU64,
    pub inline_completion_cache_misses: AtomicU64,
    pub inline_completion_latency_ms_sum: AtomicU64,
    pub inline_completion_latency_count: AtomicU64,
    pub inline_completion_accepted: AtomicU64,
    pub inline_completion_throttled: AtomicU64,

    pub inline_edit_runs: AtomicU64,
    pub inline_edit_hunks_applied: AtomicU64,
    pub inline_edit_validator_failures: AtomicU64,

    pub apply_model_hunks_exact: AtomicU64,
    pub apply_model_hunks_fuzzy: AtomicU64,
    pub apply_model_hunks_failed: AtomicU64,

    pub context_resolutions: AtomicU64,
    pub context_budget_clips: AtomicU64,

    pub evals_pass: AtomicU64,
    pub evals_fail: AtomicU64,

    pub evals_pass_at_1_micros: AtomicU64,

    pub symbol_graph_incremental_rebuilds: AtomicU64,
    pub symbol_graph_persist_skipped: AtomicU64,

    pub lsp_rename_via_lsp: AtomicU64,
    pub lsp_rename_via_regex: AtomicU64,

    pub verification_pipeline_run: AtomicU64,
    pub verification_pipeline_pass: AtomicU64,
    pub verification_pipeline_fail: AtomicU64,

    pub verification_stage_pass_syntactic: AtomicU64,
    pub verification_stage_fail_syntactic: AtomicU64,
    pub verification_stage_pass_test_runner: AtomicU64,
    pub verification_stage_fail_test_runner: AtomicU64,
    pub verification_stage_pass_lsp_diag: AtomicU64,
    pub verification_stage_fail_lsp_diag: AtomicU64,

    pub prompt_cache_hits: AtomicU64,
    pub prompt_cache_misses: AtomicU64,
    pub prompt_cache_read_tokens: AtomicU64,
    pub prompt_cache_creation_tokens: AtomicU64,

    pub self_consistency_runs: AtomicU64,
    pub self_consistency_overrides: AtomicU64,
    pub self_consistency_failures: AtomicU64,
    pub self_consistency_agreement_micros_sum: AtomicU64,
    pub self_consistency_agreement_count: AtomicU64,
}

impl SubsystemMetrics {
    pub fn snapshot(&self) -> SubsystemSnapshot {
        SubsystemSnapshot {
            inline_completion_requests: self.inline_completion_requests.load(Ordering::Relaxed),
            inline_completion_cache_hits: self.inline_completion_cache_hits.load(Ordering::Relaxed),
            inline_completion_cache_misses: self
                .inline_completion_cache_misses
                .load(Ordering::Relaxed),
            inline_completion_latency_ms_avg: {
                let count = self.inline_completion_latency_count.load(Ordering::Relaxed);
                if count == 0 {
                    0.0
                } else {
                    self.inline_completion_latency_ms_sum
                        .load(Ordering::Relaxed) as f64
                        / count as f64
                }
            },
            inline_completion_accepted: self.inline_completion_accepted.load(Ordering::Relaxed),
            inline_completion_throttled: self.inline_completion_throttled.load(Ordering::Relaxed),
            inline_edit_runs: self.inline_edit_runs.load(Ordering::Relaxed),
            inline_edit_hunks_applied: self.inline_edit_hunks_applied.load(Ordering::Relaxed),
            inline_edit_validator_failures: self
                .inline_edit_validator_failures
                .load(Ordering::Relaxed),
            apply_model_hunks_exact: self.apply_model_hunks_exact.load(Ordering::Relaxed),
            apply_model_hunks_fuzzy: self.apply_model_hunks_fuzzy.load(Ordering::Relaxed),
            apply_model_hunks_failed: self.apply_model_hunks_failed.load(Ordering::Relaxed),
            context_resolutions: self.context_resolutions.load(Ordering::Relaxed),
            context_budget_clips: self.context_budget_clips.load(Ordering::Relaxed),
            evals_pass: self.evals_pass.load(Ordering::Relaxed),
            evals_fail: self.evals_fail.load(Ordering::Relaxed),
            evals_pass_at_1: self.evals_pass_at_1_micros.load(Ordering::Relaxed) as f64 / 1e6,
            symbol_graph_incremental_rebuilds: self
                .symbol_graph_incremental_rebuilds
                .load(Ordering::Relaxed),
            symbol_graph_persist_skipped: self.symbol_graph_persist_skipped.load(Ordering::Relaxed),
            lsp_rename_via_lsp: self.lsp_rename_via_lsp.load(Ordering::Relaxed),
            lsp_rename_via_regex: self.lsp_rename_via_regex.load(Ordering::Relaxed),
            verification_pipeline_run: self.verification_pipeline_run.load(Ordering::Relaxed),
            verification_pipeline_pass: self.verification_pipeline_pass.load(Ordering::Relaxed),
            verification_pipeline_fail: self.verification_pipeline_fail.load(Ordering::Relaxed),
            verification_stage_pass_syntactic: self
                .verification_stage_pass_syntactic
                .load(Ordering::Relaxed),
            verification_stage_fail_syntactic: self
                .verification_stage_fail_syntactic
                .load(Ordering::Relaxed),
            verification_stage_pass_test_runner: self
                .verification_stage_pass_test_runner
                .load(Ordering::Relaxed),
            verification_stage_fail_test_runner: self
                .verification_stage_fail_test_runner
                .load(Ordering::Relaxed),
            verification_stage_pass_lsp_diag: self
                .verification_stage_pass_lsp_diag
                .load(Ordering::Relaxed),
            verification_stage_fail_lsp_diag: self
                .verification_stage_fail_lsp_diag
                .load(Ordering::Relaxed),
            prompt_cache_hits: self.prompt_cache_hits.load(Ordering::Relaxed),
            prompt_cache_misses: self.prompt_cache_misses.load(Ordering::Relaxed),
            prompt_cache_read_tokens: self.prompt_cache_read_tokens.load(Ordering::Relaxed),
            prompt_cache_creation_tokens: self
                .prompt_cache_creation_tokens
                .load(Ordering::Relaxed),
            self_consistency_runs: self.self_consistency_runs.load(Ordering::Relaxed),
            self_consistency_overrides: self.self_consistency_overrides.load(Ordering::Relaxed),
            self_consistency_failures: self.self_consistency_failures.load(Ordering::Relaxed),
            self_consistency_agreement_avg: {
                let count = self
                    .self_consistency_agreement_count
                    .load(Ordering::Relaxed);
                if count == 0 {
                    0.0
                } else {
                    self.self_consistency_agreement_micros_sum
                        .load(Ordering::Relaxed) as f64
                        / (count as f64 * 1e6)
                }
            },
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SubsystemSnapshot {
    pub inline_completion_requests: u64,
    pub inline_completion_cache_hits: u64,
    pub inline_completion_cache_misses: u64,
    pub inline_completion_latency_ms_avg: f64,
    pub inline_completion_accepted: u64,
    pub inline_completion_throttled: u64,
    pub inline_edit_runs: u64,
    pub inline_edit_hunks_applied: u64,
    pub inline_edit_validator_failures: u64,
    pub apply_model_hunks_exact: u64,
    pub apply_model_hunks_fuzzy: u64,
    pub apply_model_hunks_failed: u64,
    pub context_resolutions: u64,
    pub context_budget_clips: u64,
    pub evals_pass: u64,
    pub evals_fail: u64,
    pub evals_pass_at_1: f64,
    pub symbol_graph_incremental_rebuilds: u64,
    pub symbol_graph_persist_skipped: u64,
    pub lsp_rename_via_lsp: u64,
    pub lsp_rename_via_regex: u64,
    pub verification_pipeline_run: u64,
    pub verification_pipeline_pass: u64,
    pub verification_pipeline_fail: u64,
    pub verification_stage_pass_syntactic: u64,
    pub verification_stage_fail_syntactic: u64,
    pub verification_stage_pass_test_runner: u64,
    pub verification_stage_fail_test_runner: u64,
    pub verification_stage_pass_lsp_diag: u64,
    pub verification_stage_fail_lsp_diag: u64,
    pub prompt_cache_hits: u64,
    pub prompt_cache_misses: u64,
    pub prompt_cache_read_tokens: u64,
    pub prompt_cache_creation_tokens: u64,
    pub self_consistency_runs: u64,
    pub self_consistency_overrides: u64,
    pub self_consistency_failures: u64,
    pub self_consistency_agreement_avg: f64,
}

impl SubsystemSnapshot {

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
        counter!(
            "sen_inline_completion_requests_total",
            self.inline_completion_requests
        );
        counter!(
            "sen_inline_completion_cache_hits_total",
            self.inline_completion_cache_hits
        );
        counter!(
            "sen_inline_completion_cache_misses_total",
            self.inline_completion_cache_misses
        );
        gauge!(
            "sen_inline_completion_latency_ms_avg",
            self.inline_completion_latency_ms_avg
        );
        counter!(
            "sen_inline_completion_acceptance_total",
            self.inline_completion_accepted
        );
        counter!(
            "sen_inline_completion_throttled_total",
            self.inline_completion_throttled
        );
        counter!("sen_inline_edit_runs_total", self.inline_edit_runs);
        counter!(
            "sen_inline_edit_hunks_applied_total",
            self.inline_edit_hunks_applied
        );
        counter!(
            "sen_inline_edit_validator_failures_total",
            self.inline_edit_validator_failures
        );
        counter!(
            "sen_apply_model_hunks_exact_total",
            self.apply_model_hunks_exact
        );
        counter!(
            "sen_apply_model_hunks_fuzzy_total",
            self.apply_model_hunks_fuzzy
        );
        counter!(
            "sen_apply_model_hunks_failed_total",
            self.apply_model_hunks_failed
        );
        counter!(
            "sen_context_resolver_resolutions_total",
            self.context_resolutions
        );
        counter!(
            "sen_context_resolver_budget_clips_total",
            self.context_budget_clips
        );
        counter!("sen_evals_pass_total", self.evals_pass);
        counter!("sen_evals_fail_total", self.evals_fail);
        gauge!("sen_evals_pass_at_1", self.evals_pass_at_1);
        counter!(
            "sen_symbol_graph_incremental_rebuilds_total",
            self.symbol_graph_incremental_rebuilds
        );
        counter!(
            "sen_symbol_graph_persist_skipped_total",
            self.symbol_graph_persist_skipped
        );
        counter!("sen_lsp_rename_via_lsp_total", self.lsp_rename_via_lsp);
        counter!("sen_lsp_rename_via_regex_total", self.lsp_rename_via_regex);
        counter!(
            "sen_verification_pipeline_run_total",
            self.verification_pipeline_run
        );
        counter!(
            "sen_verification_pipeline_pass_total",
            self.verification_pipeline_pass
        );
        counter!(
            "sen_verification_pipeline_fail_total",
            self.verification_pipeline_fail
        );
        counter!(
            "sen_verification_stage_pass_syntactic_total",
            self.verification_stage_pass_syntactic
        );
        counter!(
            "sen_verification_stage_fail_syntactic_total",
            self.verification_stage_fail_syntactic
        );
        counter!(
            "sen_verification_stage_pass_test_runner_total",
            self.verification_stage_pass_test_runner
        );
        counter!(
            "sen_verification_stage_fail_test_runner_total",
            self.verification_stage_fail_test_runner
        );
        counter!(
            "sen_verification_stage_pass_lsp_diag_total",
            self.verification_stage_pass_lsp_diag
        );
        counter!(
            "sen_verification_stage_fail_lsp_diag_total",
            self.verification_stage_fail_lsp_diag
        );
        counter!("sen_prompt_cache_hits_total", self.prompt_cache_hits);
        counter!("sen_prompt_cache_misses_total", self.prompt_cache_misses);
        counter!(
            "sen_prompt_cache_read_tokens_total",
            self.prompt_cache_read_tokens
        );
        counter!(
            "sen_prompt_cache_creation_tokens_total",
            self.prompt_cache_creation_tokens
        );
        counter!("sen_self_consistency_runs_total", self.self_consistency_runs);
        counter!(
            "sen_self_consistency_overrides_total",
            self.self_consistency_overrides
        );
        counter!(
            "sen_self_consistency_failures_total",
            self.self_consistency_failures
        );
        gauge!(
            "sen_self_consistency_agreement_avg",
            self.self_consistency_agreement_avg
        );
        out
    }
}

static METRICS: OnceLock<SubsystemMetrics> = OnceLock::new();

pub fn global() -> &'static SubsystemMetrics {
    METRICS.get_or_init(SubsystemMetrics::default)
}

pub fn incr_inline_completion_request() {
    global()
        .inline_completion_requests
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_inline_completion_cache_hit() {
    global()
        .inline_completion_cache_hits
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_inline_completion_cache_miss() {
    global()
        .inline_completion_cache_misses
        .fetch_add(1, Ordering::Relaxed);
}
pub fn observe_inline_completion_latency_ms(ms: u64) {
    global()
        .inline_completion_latency_ms_sum
        .fetch_add(ms, Ordering::Relaxed);
    global()
        .inline_completion_latency_count
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_inline_completion_accepted() {
    global()
        .inline_completion_accepted
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_inline_completion_throttled() {
    global()
        .inline_completion_throttled
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_inline_edit_run() {
    global().inline_edit_runs.fetch_add(1, Ordering::Relaxed);
}
pub fn incr_inline_edit_hunks_applied(n: u64) {
    global()
        .inline_edit_hunks_applied
        .fetch_add(n, Ordering::Relaxed);
}
pub fn incr_inline_edit_validator_failure() {
    global()
        .inline_edit_validator_failures
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_apply_model_exact(n: u64) {
    global()
        .apply_model_hunks_exact
        .fetch_add(n, Ordering::Relaxed);
}
pub fn incr_apply_model_fuzzy(n: u64) {
    global()
        .apply_model_hunks_fuzzy
        .fetch_add(n, Ordering::Relaxed);
}
pub fn incr_apply_model_failed(n: u64) {
    global()
        .apply_model_hunks_failed
        .fetch_add(n, Ordering::Relaxed);
}

pub fn incr_apply_model_locate_strategy(strategy: &str) {
    tracing::trace!(
        target: "apply_model.locate",
        strategy = %strategy,
        "apply_model_locate_strategy",
    );
}
pub fn incr_context_resolution() {
    global().context_resolutions.fetch_add(1, Ordering::Relaxed);
}
pub fn incr_context_budget_clip() {
    global()
        .context_budget_clips
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_evals_pass() {
    global().evals_pass.fetch_add(1, Ordering::Relaxed);
}
pub fn incr_evals_fail() {
    global().evals_fail.fetch_add(1, Ordering::Relaxed);
}
pub fn set_evals_pass_at_1(value: f64) {
    let micros = (value.clamp(0.0, 1.0) * 1e6) as u64;
    global()
        .evals_pass_at_1_micros
        .store(micros, Ordering::Relaxed);
}
pub fn incr_symbol_graph_rebuild() {
    global()
        .symbol_graph_incremental_rebuilds
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_symbol_graph_persist_skipped() {
    global()
        .symbol_graph_persist_skipped
        .fetch_add(1, Ordering::Relaxed);
}
pub fn incr_lsp_rename_via_lsp() {
    global().lsp_rename_via_lsp.fetch_add(1, Ordering::Relaxed);
}
pub fn incr_lsp_rename_via_regex() {
    global()
        .lsp_rename_via_regex
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_verification_pipeline_run() {
    global()
        .verification_pipeline_run
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_verification_pipeline_pass() {
    global()
        .verification_pipeline_pass
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_verification_pipeline_fail() {
    global()
        .verification_pipeline_fail
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_verification_stage_pass(stage: &str) {
    let m = global();
    let counter = match stage {
        "syntactic" => &m.verification_stage_pass_syntactic,
        "test_runner" => &m.verification_stage_pass_test_runner,
        "lsp_diag" => &m.verification_stage_pass_lsp_diag,
        _ => return,
    };
    counter.fetch_add(1, Ordering::Relaxed);
}

pub fn incr_verification_stage_fail(stage: &str) {
    let m = global();
    let counter = match stage {
        "syntactic" => &m.verification_stage_fail_syntactic,
        "test_runner" => &m.verification_stage_fail_test_runner,
        "lsp_diag" => &m.verification_stage_fail_lsp_diag,
        _ => return,
    };
    counter.fetch_add(1, Ordering::Relaxed);
}

pub fn observe_self_consistency_run(agreement: f64, overridden: bool) {
    let m = global();
    m.self_consistency_runs.fetch_add(1, Ordering::Relaxed);
    if overridden {
        m.self_consistency_overrides.fetch_add(1, Ordering::Relaxed);
    }
    let micros = (agreement.clamp(0.0, 1.0) * 1e6) as u64;
    m.self_consistency_agreement_micros_sum
        .fetch_add(micros, Ordering::Relaxed);
    m.self_consistency_agreement_count
        .fetch_add(1, Ordering::Relaxed);
}

pub fn incr_self_consistency_failure() {
    global()
        .self_consistency_failures
        .fetch_add(1, Ordering::Relaxed);
}

pub fn observe_prompt_cache_usage(
    cached_input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
) {
    let m = global();
    let read = cached_input_tokens.unwrap_or(0);
    if read > 0 {
        m.prompt_cache_hits.fetch_add(1, Ordering::Relaxed);
        m.prompt_cache_read_tokens.fetch_add(read, Ordering::Relaxed);
    } else {
        m.prompt_cache_misses.fetch_add(1, Ordering::Relaxed);
    }
    if let Some(created) = cache_creation_input_tokens {
        if created > 0 {
            m.prompt_cache_creation_tokens
                .fetch_add(created, Ordering::Relaxed);
        }
    }
}
