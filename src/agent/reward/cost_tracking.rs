// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use crate::config::schema::ModelPricing;
use crate::cost::CostTracker;
use crate::cost::types::{BudgetCheck, TokenUsage as CostTokenUsage};

#[derive(Clone)]
pub struct ToolLoopCostTrackingContext {
    pub tracker: Arc<CostTracker>,
    pub prices: Arc<std::collections::HashMap<String, ModelPricing>>,

    pub chat_session_id: Option<String>,

    pub coding_mode: Option<String>,
}

impl ToolLoopCostTrackingContext {
    pub fn new(
        tracker: Arc<CostTracker>,
        prices: Arc<std::collections::HashMap<String, ModelPricing>>,
    ) -> Self {
        Self {
            tracker,
            prices,
            chat_session_id: None,
            coding_mode: None,
        }
    }

    pub fn with_chat_session_id(mut self, chat_session_id: impl Into<String>) -> Self {
        self.chat_session_id = Some(chat_session_id.into());
        self
    }

    pub fn with_coding_mode(mut self, coding_mode: impl Into<String>) -> Self {
        self.coding_mode = Some(coding_mode.into());
        self
    }
}

tokio::task_local! {
    pub static TOOL_LOOP_COST_TRACKING_CONTEXT: Option<ToolLoopCostTrackingContext>;
}

pub async fn scope_tool_loop_cost_tracking<F, R>(
    ctx: Option<ToolLoopCostTrackingContext>,
    f: F,
) -> R
where
    F: std::future::Future<Output = R>,
{
    TOOL_LOOP_COST_TRACKING_CONTEXT.scope(ctx, f).await
}

pub(crate) fn lookup_model_pricing<'a>(
    prices: &'a std::collections::HashMap<String, ModelPricing>,
    provider_name: &str,
    model: &str,
) -> Option<&'a ModelPricing> {
    prices
        .get(model)
        .or_else(|| prices.get(&format!("{provider_name}/{model}")))
        .or_else(|| {
            model
                .rsplit_once('/')
                .and_then(|(_, suffix)| prices.get(suffix))
        })
}

pub(crate) fn record_tool_loop_cost_usage(
    provider_name: &str,
    model: &str,
    usage: &crate::providers::traits::TokenUsage,
) -> Option<(u64, f64)> {
    let input_tokens = usage.input_tokens.unwrap_or(0);
    let output_tokens = usage.output_tokens.unwrap_or(0);
    let total_tokens = input_tokens.saturating_add(output_tokens);
    if total_tokens == 0 {
        return None;
    }

    let ctx = TOOL_LOOP_COST_TRACKING_CONTEXT
        .try_with(Clone::clone)
        .ok()
        .flatten()?;
    let pricing = lookup_model_pricing(&ctx.prices, provider_name, model);
    let cost_usage = CostTokenUsage::new(
        model,
        input_tokens,
        output_tokens,
        pricing.map_or(0.0, |entry| entry.input),
        pricing.map_or(0.0, |entry| entry.output),
    );

    if pricing.is_none() {
        tracing::debug!(
            provider = provider_name,
            model,
            "Cost tracking recorded token usage with zero pricing (no pricing entry found)"
        );
    }

    if let Err(error) = ctx.tracker.record_usage_for_session_with_mode(
        ctx.chat_session_id.as_deref(),
        ctx.coding_mode.as_deref(),
        cost_usage.clone(),
    ) {
        tracing::warn!(
            provider = provider_name,
            model,
            "Failed to record cost tracking usage: {error}"
        );
    }

    Some((cost_usage.total_tokens, cost_usage.cost_usd))
}

pub(crate) fn check_tool_loop_budget(estimated_cost_usd: Option<f64>) -> Option<BudgetCheck> {
    TOOL_LOOP_COST_TRACKING_CONTEXT
        .try_with(Clone::clone)
        .ok()
        .flatten()
        .map(|ctx| {
            let cost = estimated_cost_usd.unwrap_or(0.01);
            ctx.tracker
                .check_budget(cost)
                .unwrap_or(BudgetCheck::Allowed)
        })
}
