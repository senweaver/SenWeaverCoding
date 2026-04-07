// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Token budget management — mirrors claude-code-typescript-src`query/tokenBudget.ts`.

use serde::{Deserialize, Serialize};

/// Rough token estimate for budgeting (≈4 characters per token for Latin text).
#[must_use]
pub fn estimate_tokens(text: &str) -> u32 {
    let chars = text.chars().count() as u64;
    if chars == 0 {
        return 0;
    }
    ((chars + 3) / 4) as u32
}

/// Tracks how much of the context window is consumed and how much remains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBudget {
    /// Total context window size (tokens).
    pub context_window: u32,
    /// Maximum output tokens the model may produce.
    pub max_output_tokens: u32,
    /// Tokens reserved for the system prompt (cached).
    pub system_prompt_tokens: u32,
    /// Tokens consumed by conversation history so far.
    pub history_tokens: u32,
    /// Tokens consumed by tool definitions.
    pub tool_definition_tokens: u32,
    /// Input tokens recorded for the most recent turn (tool loop iteration).
    #[serde(default)]
    pub last_turn_input_tokens: u32,
    /// Output tokens recorded for the most recent turn.
    #[serde(default)]
    pub last_turn_output_tokens: u32,
}

impl TokenBudget {
    pub fn new(context_window: u32, max_output_tokens: u32) -> Self {
        Self {
            context_window,
            max_output_tokens,
            system_prompt_tokens: 0,
            history_tokens: 0,
            tool_definition_tokens: 0,
            last_turn_input_tokens: 0,
            last_turn_output_tokens: 0,
        }
    }

    /// Record token usage for the last completed turn (e.g. after a model response).
    pub fn record_turn(&mut self, input_tokens: u32, output_tokens: u32) {
        self.last_turn_input_tokens = input_tokens;
        self.last_turn_output_tokens = output_tokens;
    }

    /// Tokens already consumed (system + history + tool defs).
    pub fn consumed(&self) -> u32 {
        self.system_prompt_tokens + self.history_tokens + self.tool_definition_tokens
    }

    /// Tokens remaining for new conversation content (before output reservation).
    pub fn remaining_input(&self) -> u32 {
        self.context_window
            .saturating_sub(self.consumed())
            .saturating_sub(self.max_output_tokens)
    }

    /// Whether the budget is exhausted (no room for meaningful input).
    pub fn is_exhausted(&self) -> bool {
        self.remaining_input() < 1000
    }

    /// Fraction of context window consumed (0.0–1.0).
    pub fn utilization(&self) -> f64 {
        if self.context_window == 0 {
            return 0.0;
        }
        self.consumed() as f64 / self.context_window as f64
    }

    /// Whether compaction should be triggered (> threshold utilization).
    pub fn should_compact(&self, threshold: f64) -> bool {
        self.utilization() > threshold
    }

    /// Update the system prompt token count.
    pub fn set_system_prompt_tokens(&mut self, tokens: u32) {
        self.system_prompt_tokens = tokens;
    }

    /// Update the history token count.
    pub fn set_history_tokens(&mut self, tokens: u32) {
        self.history_tokens = tokens;
    }

    /// Update the tool definition token count.
    pub fn set_tool_definition_tokens(&mut self, tokens: u32) {
        self.tool_definition_tokens = tokens;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_budget_basics() {
        let mut budget = TokenBudget::new(200_000, 16_384);
        budget.set_system_prompt_tokens(5000);
        budget.set_history_tokens(50_000);
        budget.set_tool_definition_tokens(10_000);

        assert_eq!(budget.consumed(), 65_000);
        assert_eq!(budget.remaining_input(), 200_000 - 65_000 - 16_384);
        assert!(!budget.is_exhausted());
        assert!(!budget.should_compact(0.8));
    }

    #[test]
    fn test_budget_exhausted() {
        let mut budget = TokenBudget::new(200_000, 16_384);
        budget.set_system_prompt_tokens(5000);
        budget.set_history_tokens(178_000);
        budget.set_tool_definition_tokens(10_000);

        assert!(budget.is_exhausted());
        assert!(budget.should_compact(0.8));
    }

    #[test]
    fn estimate_tokens_empty_and_short() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcdefghij"), 3);
    }

    #[test]
    fn record_turn_updates_last_turn() {
        let mut budget = TokenBudget::new(100_000, 4096);
        budget.record_turn(100, 50);
        assert_eq!(budget.last_turn_input_tokens, 100);
        assert_eq!(budget.last_turn_output_tokens, 50);
    }
}
