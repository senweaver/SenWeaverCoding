// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};

use super::compact::{
    CompactionConfig, CompactionStrategy, create_collapse_marker, should_compact,
    sliding_window_compact,
};
use super::stop_hooks::{StopHook, StopHookContext, StopHookResult, evaluate_stop_hooks};
use super::token_budget::TokenBudget;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {

    pub response_text: String,

    pub tool_calls: Vec<ToolCallRequest>,

    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,

    pub cost_usd: f64,

    pub api_duration_ms: u64,

    pub stop_reason: Option<String>,

    pub aborted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRequest {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

pub struct QueryEngine {
    stop_hooks: Vec<Box<dyn StopHook>>,
    budget: TokenBudget,
    compaction: CompactionConfig,
}

impl QueryEngine {
    pub fn new(context_window: u32, max_output_tokens: u32) -> Self {
        Self {
            stop_hooks: Vec::new(),
            budget: TokenBudget::new(context_window, max_output_tokens),
            compaction: CompactionConfig::default(),
        }
    }

    pub fn compaction_config(&self) -> &CompactionConfig {
        &self.compaction
    }

    pub fn set_compaction_config(&mut self, config: CompactionConfig) {
        self.compaction = config;
    }

    pub fn should_auto_compact(&self, current_estimated_tokens: usize) -> bool {
        let max = self.budget.context_window as usize;
        should_compact(current_estimated_tokens, max, &self.compaction)
    }

    pub fn compact_messages(
        &self,
        messages: &[serde_json::Value],
        current_estimated_tokens: usize,
        summary: Option<&str>,
    ) -> Vec<serde_json::Value> {
        let max = self.budget.context_window as usize;
        if !should_compact(current_estimated_tokens, max, &self.compaction) {
            return messages.to_vec();
        }

        match self.compaction.strategy {
            CompactionStrategy::None => messages.to_vec(),
            CompactionStrategy::Summarize
            | CompactionStrategy::SlidingWindow
            | CompactionStrategy::Auto => {
                let before = messages.len();
                let keep = self.compaction.min_keep_messages.max(1);
                let mut out =
                    sliding_window_compact(messages, keep, self.compaction.preserve_system);
                let dropped = before.saturating_sub(out.len());
                if dropped > 0 {
                    let text = summary.unwrap_or("[Earlier conversation truncated for length]");
                    let marker = create_collapse_marker(dropped, text);
                    let insert_at = out
                        .iter()
                        .position(|m| m.get("role").and_then(|r| r.as_str()) != Some("system"))
                        .unwrap_or(out.len());
                    out.insert(insert_at, marker);
                }
                out
            }
        }
    }

    pub fn add_stop_hook(&mut self, hook: Box<dyn StopHook>) {
        self.stop_hooks.push(hook);
    }

    pub fn budget(&self) -> &TokenBudget {
        &self.budget
    }

    pub fn budget_mut(&mut self) -> &mut TokenBudget {
        &mut self.budget
    }

    pub async fn check_stop_hooks(
        &self,
        tool_turn_count: u32,
        total_tokens_used: u64,
        current_cost_usd: f64,
        budget_limit_usd: Option<f64>,
        max_tool_turns: Option<u32>,
        model_stop_reason: Option<String>,
    ) -> StopHookResult {
        let ctx = StopHookContext {
            tool_turn_count,
            total_tokens_used,
            context_window: self.budget.context_window,
            model_stop_reason,
            current_cost_usd,
            budget_limit_usd,
            max_tool_turns,
        };
        evaluate_stop_hooks(&self.stop_hooks, &ctx).await
    }

    pub fn needs_compaction(&self, threshold: f64) -> bool {
        self.budget.should_compact(threshold)
    }
}
