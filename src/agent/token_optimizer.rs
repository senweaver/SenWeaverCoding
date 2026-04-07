// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Token optimization integration layer.
//!
//! Wires together the ToolOutputCompressor, TokenBudgetManager,
//! and the existing context compression / history pruning into
//! a unified optimization pipeline. This is the single entry point
//! for all token-saving operations inspired by RTK.

use super::token_budget::{TokenBudgetConfig, TokenBudgetManager};
use super::tool_output_compressor::{
    CompressionResult, ToolOutputCompressor, ToolOutputCompressorConfig,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Unified token optimization engine.
pub struct TokenOptimizer {
    compressor: ToolOutputCompressor,
    budget: TokenBudgetManager,
    stats: Arc<OptimizerStats>,
}

struct OptimizerStats {
    total_tool_calls: AtomicU64,
    compressed_tool_calls: AtomicU64,
    total_chars_in: AtomicU64,
    total_chars_out: AtomicU64,
}

impl OptimizerStats {
    fn new() -> Self {
        Self {
            total_tool_calls: AtomicU64::new(0),
            compressed_tool_calls: AtomicU64::new(0),
            total_chars_in: AtomicU64::new(0),
            total_chars_out: AtomicU64::new(0),
        }
    }
}

/// Summary of optimization activity for reporting.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OptimizationReport {
    pub total_tool_calls: u64,
    pub compressed_tool_calls: u64,
    pub total_chars_in: u64,
    pub total_chars_out: u64,
    pub chars_saved: u64,
    pub estimated_tokens_saved: u64,
    pub savings_pct: f64,
    pub budget_utilization_pct: f64,
}

impl TokenOptimizer {
    pub fn new(
        compressor_config: ToolOutputCompressorConfig,
        budget_config: TokenBudgetConfig,
    ) -> Self {
        Self {
            compressor: ToolOutputCompressor::new(compressor_config),
            budget: TokenBudgetManager::new(budget_config),
            stats: Arc::new(OptimizerStats::new()),
        }
    }

    /// Compress a tool's output before it enters the conversation history.
    /// This is the primary RTK-inspired optimization: shrink tool results.
    pub fn compress_tool_output(&self, tool_name: &str, output: &str) -> String {
        self.stats.total_tool_calls.fetch_add(1, Ordering::Relaxed);
        self.stats
            .total_chars_in
            .fetch_add(output.len() as u64, Ordering::Relaxed);

        let result: CompressionResult = self.compressor.compress(tool_name, output);

        self.stats
            .total_chars_out
            .fetch_add(result.compressed_chars as u64, Ordering::Relaxed);

        if !result.strategies_applied.is_empty() {
            self.stats
                .compressed_tool_calls
                .fetch_add(1, Ordering::Relaxed);
            self.budget.record_savings(result.estimated_tokens_saved());

            if result.savings_pct() > 5.0 {
                tracing::debug!(
                    tool = tool_name,
                    original = result.original_chars,
                    compressed = result.compressed_chars,
                    savings_pct = format!("{:.1}%", result.savings_pct()),
                    strategies = ?result.strategies_applied,
                    "tool output compressed"
                );
            }
        } else if self.compressor.is_disabled() {
            // Log why no compression was applied — useful for debugging compression config
            tracing::trace!(tool = tool_name, "tool output compression disabled or no strategies active; passing through unchanged");
        }

        result.output
    }

    /// Check if history compression should be triggered based on token budget.
    pub fn should_compress_history(&self, system_prompt: &str, history_text_total: usize) -> bool {
        let sys_tokens = TokenBudgetManager::estimate_tokens(system_prompt);
        let status = self.budget.check_status(sys_tokens, history_text_total / 4);
        status.should_compress
    }

    /// Get the maximum characters recommended for a single tool result.
    pub fn max_tool_result_chars(&self) -> usize {
        self.budget.max_tool_result_chars()
    }

    /// Get the maximum characters recommended for RAG context.
    pub fn max_rag_chars(&self) -> usize {
        self.budget.max_rag_chars()
    }

    /// Record API token usage from provider response.
    pub fn record_api_usage(&self, input_tokens: usize, output_tokens: usize) {
        self.budget.record_usage(input_tokens, output_tokens);
    }

    /// Suggest how many messages to keep in history.
    pub fn suggest_max_messages(&self, current_tokens: usize, message_count: usize) -> usize {
        self.budget
            .suggest_max_messages(current_tokens, message_count)
    }

    /// Get a report of all optimization activity.
    pub fn report(&self, system_prompt_tokens: usize, history_tokens: usize) -> OptimizationReport {
        let total_in = self.stats.total_chars_in.load(Ordering::Relaxed);
        let total_out = self.stats.total_chars_out.load(Ordering::Relaxed);
        let chars_saved = total_in.saturating_sub(total_out);
        let savings_pct = if total_in > 0 {
            (chars_saved as f64 / total_in as f64) * 100.0
        } else {
            0.0
        };

        let budget_status = self
            .budget
            .check_status(system_prompt_tokens, history_tokens);

        OptimizationReport {
            total_tool_calls: self.stats.total_tool_calls.load(Ordering::Relaxed),
            compressed_tool_calls: self.stats.compressed_tool_calls.load(Ordering::Relaxed),
            total_chars_in: total_in,
            total_chars_out: total_out,
            chars_saved,
            estimated_tokens_saved: chars_saved / 4,
            savings_pct,
            budget_utilization_pct: budget_status.utilization_pct,
        }
    }

    /// Access the underlying budget manager.
    pub fn budget(&self) -> &TokenBudgetManager {
        &self.budget
    }
}

/// Convenience: create an optimizer from the unified config.
pub fn create_optimizer(
    compressor_config: ToolOutputCompressorConfig,
    budget_config: TokenBudgetConfig,
) -> Arc<TokenOptimizer> {
    Arc::new(TokenOptimizer::new(compressor_config, budget_config))
}

static GLOBAL_OPTIMIZER: std::sync::OnceLock<Arc<TokenOptimizer>> = std::sync::OnceLock::new();

/// Ensure the global token optimizer exists (first config wins).
/// Safe to call from gateway startup and from [`crate::agent::Agent::from_config`].
pub fn ensure_global_optimizer(
    compressor_config: ToolOutputCompressorConfig,
    budget_config: TokenBudgetConfig,
) {
    let _ = GLOBAL_OPTIMIZER.get_or_init(|| create_optimizer(compressor_config, budget_config));
}

/// Convenience: ensure optimizer from full runtime config.
pub fn ensure_global_optimizer_from_config(config: &crate::config::Config) {
    ensure_global_optimizer(
        config.tool_output_compressor.clone(),
        config.token_budget.clone(),
    );
}

/// Get the global token optimizer, if initialized.
pub fn global_optimizer() -> Option<&'static Arc<TokenOptimizer>> {
    GLOBAL_OPTIMIZER.get()
}

/// Compress tool output using the global optimizer.
/// Returns the original output unchanged if the optimizer is not initialized.
pub fn compress_output(tool_name: &str, output: &str) -> String {
    match GLOBAL_OPTIMIZER.get() {
        Some(opt) => opt.compress_tool_output(tool_name, output),
        None => output.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_optimizer() -> TokenOptimizer {
        TokenOptimizer::new(
            ToolOutputCompressorConfig {
                enabled: true,
                max_output_chars: 500,
                ..Default::default()
            },
            TokenBudgetConfig {
                enabled: true,
                context_window: 10_000,
                ..Default::default()
            },
        )
    }

    #[test]
    fn compress_small_output_unchanged() {
        let opt = test_optimizer();
        let result = opt.compress_tool_output("shell", "hello");
        assert_eq!(result, "hello");
    }

    #[test]
    fn compress_large_output_truncated() {
        let opt = test_optimizer();
        let large = "x\n".repeat(500);
        let result = opt.compress_tool_output("shell", &large);
        assert!(result.len() < large.len());
    }

    #[test]
    fn stats_tracked() {
        let opt = test_optimizer();
        opt.compress_tool_output("shell", "small");
        opt.compress_tool_output("file_read", &"y".repeat(1000));

        let report = opt.report(0, 0);
        assert_eq!(report.total_tool_calls, 2);
        assert!(report.total_chars_in > 0);
    }

    #[test]
    fn budget_integration() {
        let opt = test_optimizer();
        assert!(opt.max_tool_result_chars() > 0);
        assert!(opt.max_rag_chars() > 0);
    }

    #[test]
    fn api_usage_recording() {
        let opt = test_optimizer();
        opt.record_api_usage(1000, 500);
        let stats = opt.budget().usage_stats();
        assert_eq!(stats.cumulative_input_tokens, 1000);
        assert_eq!(stats.cumulative_output_tokens, 500);
    }

    #[test]
    fn history_compression_trigger() {
        let opt = test_optimizer();
        let small_sys = "system prompt";
        assert!(!opt.should_compress_history(small_sys, 1000));

        // Large history should trigger
        assert!(opt.should_compress_history(small_sys, 100_000));
    }
}
