// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Stop hooks — mirrors claude-code-typescript-src`query/stopHooks.ts`.
// Evaluates conditions that may halt query execution mid-turn.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StopHookResult {

    Continue,

    Stop { reason: String },

    Pause { reason: String },
}

#[async_trait::async_trait]
pub trait StopHook: Send + Sync {

    fn name(&self) -> &str;

    async fn evaluate(&self, ctx: &StopHookContext) -> StopHookResult;
}

#[derive(Debug, Clone)]
pub struct StopHookContext {

    pub tool_turn_count: u32,

    pub total_tokens_used: u64,

    pub context_window: u32,

    pub model_stop_reason: Option<String>,

    pub current_cost_usd: f64,

    pub budget_limit_usd: Option<f64>,

    pub max_tool_turns: Option<u32>,
}

pub struct TokenLimitStopHook {
    pub threshold_fraction: f64,
}

#[async_trait::async_trait]
impl StopHook for TokenLimitStopHook {
    fn name(&self) -> &str {
        "token_limit"
    }

    async fn evaluate(&self, ctx: &StopHookContext) -> StopHookResult {
        let limit = (ctx.context_window as f64 * self.threshold_fraction) as u64;
        if ctx.total_tokens_used >= limit {
            StopHookResult::Stop {
                reason: format!(
                    "Token usage ({}) exceeded {}% of context window ({})",
                    ctx.total_tokens_used,
                    (self.threshold_fraction * 100.0) as u32,
                    ctx.context_window
                ),
            }
        } else {
            StopHookResult::Continue
        }
    }
}

pub struct MaxTurnsStopHook;

#[async_trait::async_trait]
impl StopHook for MaxTurnsStopHook {
    fn name(&self) -> &str {
        "max_turns"
    }

    async fn evaluate(&self, ctx: &StopHookContext) -> StopHookResult {
        if let Some(max) = ctx.max_tool_turns {
            if ctx.tool_turn_count >= max {
                return StopHookResult::Stop {
                    reason: format!("Reached maximum tool turns ({max})"),
                };
            }
        }
        StopHookResult::Continue
    }
}

pub struct BudgetStopHook;

#[async_trait::async_trait]
impl StopHook for BudgetStopHook {
    fn name(&self) -> &str {
        "budget"
    }

    async fn evaluate(&self, ctx: &StopHookContext) -> StopHookResult {
        if let Some(limit) = ctx.budget_limit_usd {
            if ctx.current_cost_usd >= limit {
                return StopHookResult::Stop {
                    reason: format!(
                        "Cost (${:.4}) exceeded budget limit (${:.4})",
                        ctx.current_cost_usd, limit
                    ),
                };
            }
        }
        StopHookResult::Continue
    }
}

pub async fn evaluate_stop_hooks(
    hooks: &[Box<dyn StopHook>],
    ctx: &StopHookContext,
) -> StopHookResult {
    for hook in hooks {
        let result = hook.evaluate(ctx).await;
        match &result {
            StopHookResult::Continue => continue,
            _ => return result,
        }
    }
    StopHookResult::Continue
}

pub fn standard_stop_hooks(token_threshold_fraction: f64) -> Vec<Box<dyn StopHook>> {
    vec![
        Box::new(TokenLimitStopHook {
            threshold_fraction: token_threshold_fraction,
        }),
        Box::new(MaxTurnsStopHook),
        Box::new(BudgetStopHook),
    ]
}
