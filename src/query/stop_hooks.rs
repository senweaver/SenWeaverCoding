// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Stop hooks — mirrors claude-code-typescript-src`query/stopHooks.ts`.
// Evaluates conditions that may halt query execution mid-turn.

use serde::{Deserialize, Serialize};

/// Result of evaluating a stop hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StopHookResult {
    /// Continue processing.
    Continue,
    /// Stop with a reason message injected back to the model.
    Stop { reason: String },
    /// Pause and wait for user input.
    Pause { reason: String },
}

/// A stop hook that can interrupt query execution.
#[async_trait::async_trait]
pub trait StopHook: Send + Sync {
    /// Human-readable name for logging.
    fn name(&self) -> &str;

    /// Evaluate whether the query should stop after this tool-use turn.
    async fn evaluate(&self, ctx: &StopHookContext) -> StopHookResult;
}

/// Context passed to stop hooks for evaluation.
#[derive(Debug, Clone)]
pub struct StopHookContext {
    /// Number of tool-use turns completed so far.
    pub tool_turn_count: u32,
    /// Total tokens consumed so far (input + output).
    pub total_tokens_used: u64,
    /// Context window size.
    pub context_window: u32,
    /// Whether the model requested to stop.
    pub model_stop_reason: Option<String>,
    /// Current cost in USD.
    pub current_cost_usd: f64,
    /// Budget limit in USD (if set).
    pub budget_limit_usd: Option<f64>,
    /// Maximum allowed tool turns.
    pub max_tool_turns: Option<u32>,
}

// ---------------------------------------------------------------------------
// Built-in stop hooks
// ---------------------------------------------------------------------------

/// Stop when token usage exceeds a fraction of the context window.
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

/// Stop when tool turn count exceeds max.
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

/// Stop when cost exceeds budget.
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

/// Evaluate all stop hooks; returns the first non-Continue result.
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

/// Standard hooks: token limit (fraction of context), max tool turns, and USD budget.
/// Order is token limit → max turns → budget (first stop wins).
pub fn standard_stop_hooks(token_threshold_fraction: f64) -> Vec<Box<dyn StopHook>> {
    vec![
        Box::new(TokenLimitStopHook {
            threshold_fraction: token_threshold_fraction,
        }),
        Box::new(MaxTurnsStopHook),
        Box::new(BudgetStopHook),
    ]
}
