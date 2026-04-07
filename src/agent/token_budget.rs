// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Centralized token budget management and allocation.
//!
//! Provides real-time token estimation, budget allocation per component
//! (system prompt, history, tool results, RAG context), and auto-triggers
//! compression when approaching limits. Inspired by RTK's token tracking
//! combined with SenWeaverCoding's existing context window management.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TokenBudgetConfig {
    #[serde(default)]
    pub enabled: bool,

    /// Total context window size in tokens (overrides provider default if set).
    #[serde(default = "default_context_window")]
    pub context_window: usize,

    /// Fraction of context window reserved for system prompt (0.0-1.0).
    #[serde(default = "default_system_prompt_ratio")]
    pub system_prompt_ratio: f32,

    /// Fraction of context window reserved for generation output.
    #[serde(default = "default_output_ratio")]
    pub output_ratio: f32,

    /// Trigger compression when history exceeds this fraction of available budget.
    #[serde(default = "default_compression_threshold")]
    pub compression_threshold: f32,

    /// Maximum tokens for a single tool result.
    #[serde(default = "default_max_tool_result_tokens")]
    pub max_tool_result_tokens: usize,

    /// Maximum tokens for RAG context injection.
    #[serde(default = "default_max_rag_tokens")]
    pub max_rag_tokens: usize,
}

fn default_context_window() -> usize {
    128_000
}
fn default_system_prompt_ratio() -> f32 {
    0.15
}
fn default_output_ratio() -> f32 {
    0.15
}
fn default_compression_threshold() -> f32 {
    0.75
}
fn default_max_tool_result_tokens() -> usize {
    12_000
}
fn default_max_rag_tokens() -> usize {
    8_000
}

impl Default for TokenBudgetConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            context_window: default_context_window(),
            system_prompt_ratio: default_system_prompt_ratio(),
            output_ratio: default_output_ratio(),
            compression_threshold: default_compression_threshold(),
            max_tool_result_tokens: default_max_tool_result_tokens(),
            max_rag_tokens: default_max_rag_tokens(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BudgetAllocation {
    pub total_tokens: usize,
    pub system_prompt_budget: usize,
    pub output_budget: usize,
    pub history_budget: usize,
    pub tool_result_budget: usize,
    pub rag_budget: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct BudgetStatus {
    pub allocation: BudgetAllocation,
    pub system_prompt_used: usize,
    pub history_used: usize,
    pub available_for_history: usize,
    pub utilization_pct: f64,
    pub should_compress: bool,
    pub cumulative_tokens_saved: usize,
}

pub struct TokenBudgetManager {
    config: TokenBudgetConfig,
    allocation: BudgetAllocation,
    cumulative_saved: Arc<AtomicUsize>,
    cumulative_input: Arc<AtomicUsize>,
    cumulative_output: Arc<AtomicUsize>,
}

impl TokenBudgetManager {
    pub fn new(config: TokenBudgetConfig) -> Self {
        let allocation = Self::compute_allocation(&config);
        Self {
            config,
            allocation,
            cumulative_saved: Arc::new(AtomicUsize::new(0)),
            cumulative_input: Arc::new(AtomicUsize::new(0)),
            cumulative_output: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn compute_allocation(config: &TokenBudgetConfig) -> BudgetAllocation {
        let total = config.context_window;
        let system_prompt_budget = (total as f64 * config.system_prompt_ratio as f64) as usize;
        let output_budget = (total as f64 * config.output_ratio as f64) as usize;
        let rag_budget = config.max_rag_tokens.min(total / 10);
        let history_budget = total
            .saturating_sub(system_prompt_budget)
            .saturating_sub(output_budget)
            .saturating_sub(rag_budget);

        BudgetAllocation {
            total_tokens: total,
            system_prompt_budget,
            output_budget,
            history_budget,
            tool_result_budget: config.max_tool_result_tokens,
            rag_budget,
        }
    }

    /// Estimate tokens from text using the ~4 chars/token heuristic.
    pub fn estimate_tokens(text: &str) -> usize {
        text.len().div_ceil(4).saturating_add(4)
    }

    /// Estimate tokens for a list of messages.
    pub fn estimate_messages_tokens(messages: &[impl AsRef<str>]) -> usize {
        messages
            .iter()
            .map(|m| Self::estimate_tokens(m.as_ref()))
            .sum()
    }

    /// Check current budget status given system prompt and history sizes.
    pub fn check_status(&self, system_prompt_tokens: usize, history_tokens: usize) -> BudgetStatus {
        let available = self.allocation.history_budget.saturating_sub(
            system_prompt_tokens.saturating_sub(self.allocation.system_prompt_budget),
        );

        let utilization = if available > 0 {
            history_tokens as f64 / available as f64
        } else {
            1.0
        };

        let should_compress = utilization > self.config.compression_threshold as f64;

        BudgetStatus {
            allocation: self.allocation.clone(),
            system_prompt_used: system_prompt_tokens,
            history_used: history_tokens,
            available_for_history: available,
            utilization_pct: utilization * 100.0,
            should_compress,
            cumulative_tokens_saved: self.cumulative_saved.load(Ordering::Relaxed),
        }
    }

    /// Record token savings from compression.
    pub fn record_savings(&self, tokens_saved: usize) {
        self.cumulative_saved
            .fetch_add(tokens_saved, Ordering::Relaxed);
    }

    /// Record API token usage.
    pub fn record_usage(&self, input_tokens: usize, output_tokens: usize) {
        self.cumulative_input
            .fetch_add(input_tokens, Ordering::Relaxed);
        self.cumulative_output
            .fetch_add(output_tokens, Ordering::Relaxed);
    }

    /// Get the maximum chars allowed for a tool result.
    pub fn max_tool_result_chars(&self) -> usize {
        self.allocation.tool_result_budget * 4
    }

    /// Get the maximum chars allowed for RAG context.
    pub fn max_rag_chars(&self) -> usize {
        self.allocation.rag_budget * 4
    }

    /// Get cumulative usage statistics.
    pub fn usage_stats(&self) -> TokenUsageStats {
        TokenUsageStats {
            cumulative_input_tokens: self.cumulative_input.load(Ordering::Relaxed),
            cumulative_output_tokens: self.cumulative_output.load(Ordering::Relaxed),
            cumulative_tokens_saved: self.cumulative_saved.load(Ordering::Relaxed),
            context_window: self.config.context_window,
        }
    }

    /// Get the budget allocation.
    pub fn allocation(&self) -> &BudgetAllocation {
        &self.allocation
    }

    /// Suggest how many messages to keep based on remaining budget.
    pub fn suggest_max_messages(&self, current_tokens: usize, message_count: usize) -> usize {
        if message_count == 0 || current_tokens == 0 {
            return message_count;
        }

        let avg_per_message = current_tokens / message_count;
        if avg_per_message == 0 {
            return message_count;
        }

        let budget = self.allocation.history_budget;
        let target = (budget as f64 * self.config.compression_threshold as f64) as usize;
        let suggested = target / avg_per_message;

        suggested.max(4).min(message_count)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TokenUsageStats {
    pub cumulative_input_tokens: usize,
    pub cumulative_output_tokens: usize,
    pub cumulative_tokens_saved: usize,
    pub context_window: usize,
}

impl TokenUsageStats {
    pub fn total_tokens(&self) -> usize {
        self.cumulative_input_tokens + self.cumulative_output_tokens
    }

    pub fn savings_pct(&self) -> f64 {
        let total_possible = self.total_tokens() + self.cumulative_tokens_saved;
        if total_possible == 0 {
            return 0.0;
        }
        (self.cumulative_tokens_saved as f64 / total_possible as f64) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_allocation() {
        let config = TokenBudgetConfig::default();
        let manager = TokenBudgetManager::new(config);
        let alloc = manager.allocation();

        assert_eq!(alloc.total_tokens, 128_000);
        assert!(alloc.system_prompt_budget > 0);
        assert!(alloc.output_budget > 0);
        assert!(alloc.history_budget > 0);
        assert_eq!(
            alloc.system_prompt_budget
                + alloc.output_budget
                + alloc.history_budget
                + alloc.rag_budget,
            alloc.total_tokens
        );
    }

    #[test]
    fn token_estimation() {
        assert_eq!(TokenBudgetManager::estimate_tokens(""), 4);
        assert_eq!(TokenBudgetManager::estimate_tokens("hello"), 6);
        assert_eq!(
            TokenBudgetManager::estimate_tokens("a".repeat(100).as_str()),
            29
        );
    }

    #[test]
    fn status_below_threshold() {
        let config = TokenBudgetConfig {
            enabled: true,
            context_window: 10_000,
            compression_threshold: 0.75,
            ..Default::default()
        };
        let manager = TokenBudgetManager::new(config);
        let status = manager.check_status(1000, 2000);
        assert!(!status.should_compress);
    }

    #[test]
    fn status_above_threshold() {
        let config = TokenBudgetConfig {
            enabled: true,
            context_window: 10_000,
            compression_threshold: 0.5,
            ..Default::default()
        };
        let manager = TokenBudgetManager::new(config);
        let alloc = manager.allocation();
        let high_usage = alloc.history_budget; // 100% usage
        let status = manager.check_status(0, high_usage);
        assert!(status.should_compress);
    }

    #[test]
    fn record_savings() {
        let manager = TokenBudgetManager::new(Default::default());
        manager.record_savings(100);
        manager.record_savings(200);
        assert_eq!(manager.cumulative_saved.load(Ordering::Relaxed), 300);
    }

    #[test]
    fn usage_stats() {
        let manager = TokenBudgetManager::new(Default::default());
        manager.record_usage(1000, 500);
        manager.record_savings(200);
        let stats = manager.usage_stats();
        assert_eq!(stats.cumulative_input_tokens, 1000);
        assert_eq!(stats.cumulative_output_tokens, 500);
        assert_eq!(stats.cumulative_tokens_saved, 200);
        assert_eq!(stats.total_tokens(), 1500);
    }

    #[test]
    fn suggest_messages() {
        let config = TokenBudgetConfig {
            enabled: true,
            context_window: 10_000,
            compression_threshold: 0.75,
            ..Default::default()
        };
        let manager = TokenBudgetManager::new(config);
        let suggested = manager.suggest_max_messages(5000, 50);
        assert!(suggested < 50);
        assert!(suggested >= 4);
    }

    #[test]
    fn max_tool_result_chars() {
        let config = TokenBudgetConfig {
            max_tool_result_tokens: 3000,
            ..Default::default()
        };
        let manager = TokenBudgetManager::new(config);
        assert_eq!(manager.max_tool_result_chars(), 12_000);
    }
}
