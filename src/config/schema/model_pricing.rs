// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CostConfig {
    #[serde(default = "default_cost_enabled")]
    pub enabled: bool,

    #[serde(default = "default_daily_limit")]
    pub daily_limit_usd: f64,

    #[serde(default = "default_monthly_limit")]
    pub monthly_limit_usd: f64,

    #[serde(default = "default_warn_percent")]
    pub warn_at_percent: u8,

    #[serde(default)]
    pub allow_override: bool,

    #[serde(default = "get_default_pricing")]
    pub prices: std::collections::HashMap<String, ModelPricing>,

    #[serde(default)]
    pub enforcement: CostEnforcementConfig,
}

impl CostConfig {
    pub fn merge_default_prices(&mut self) {
        for (key, value) in get_default_pricing() {
            self.prices.entry(key).or_insert(value);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CostEnforcementConfig {
    #[serde(default = "default_cost_enforcement_mode")]
    pub mode: String,

    #[serde(default)]
    pub route_down_model: Option<String>,

    #[serde(default = "default_reserve_percent")]
    pub reserve_percent: u8,
}

fn default_cost_enforcement_mode() -> String {
    "warn".to_string()
}

fn default_reserve_percent() -> u8 {
    10
}

impl Default for CostEnforcementConfig {
    fn default() -> Self {
        Self {
            mode: default_cost_enforcement_mode(),
            route_down_model: None,
            reserve_percent: default_reserve_percent(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModelPricing {
    #[serde(default)]
    pub input: f64,

    #[serde(default)]
    pub output: f64,
}

fn default_daily_limit() -> f64 {
    10.0
}

fn default_monthly_limit() -> f64 {
    100.0
}

fn default_warn_percent() -> u8 {
    80
}

fn default_cost_enabled() -> bool {
    true
}

impl Default for CostConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            daily_limit_usd: default_daily_limit(),
            monthly_limit_usd: default_monthly_limit(),
            warn_at_percent: default_warn_percent(),
            allow_override: false,
            prices: get_default_pricing(),
            enforcement: CostEnforcementConfig::default(),
        }
    }
}

pub fn get_default_pricing() -> std::collections::HashMap<String, ModelPricing> {
    const DEFAULTS: &[(&str, f64, f64)] = &[
        ("anthropic/claude-sonnet-4-20250514", 3.0, 15.0),
        ("anthropic/claude-opus-4-20250514", 15.0, 75.0),
        ("anthropic/claude-sonnet-4-5", 3.0, 15.0),
        ("anthropic/claude-opus-4-1", 15.0, 75.0),
        ("anthropic/claude-haiku-4-5", 1.0, 5.0),
        ("anthropic/claude-3.5-sonnet", 3.0, 15.0),
        ("anthropic/claude-3-5-haiku", 0.8, 4.0),
        ("anthropic/claude-3-haiku", 0.25, 1.25),
        ("openai/gpt-4o", 2.5, 10.0),
        ("openai/gpt-4o-mini", 0.15, 0.60),
        ("openai/gpt-4.1", 2.0, 8.0),
        ("openai/gpt-4.1-mini", 0.4, 1.6),
        ("openai/gpt-4.1-nano", 0.1, 0.4),
        ("openai/gpt-5", 1.25, 10.0),
        ("openai/gpt-5-mini", 0.25, 2.0),
        ("openai/gpt-5-nano", 0.05, 0.4),
        ("openai/gpt-5-codex", 1.25, 10.0),
        ("openai/o1", 15.0, 60.0),
        ("openai/o3", 2.0, 8.0),
        ("openai/o4-mini", 1.1, 4.4),
        ("deepseek/deepseek-chat", 0.28, 1.10),
        ("deepseek/deepseek-reasoner", 0.55, 2.19),
        ("deepseek/deepseek-v3", 0.27, 1.10),
        ("deepseek/deepseek-v3.1", 0.27, 1.10),
        ("moonshot/kimi-k2", 0.6, 2.5),
        ("moonshot/kimi-k2-thinking", 0.6, 2.5),
        ("qwen/qwen-max", 1.6, 6.4),
        ("qwen/qwen-plus", 0.4, 1.2),
        ("qwen/qwen-turbo", 0.05, 0.2),
        ("qwen/qwen3-coder-plus", 1.0, 5.0),
        ("zhipu/glm-4.6", 0.6, 2.2),
        ("zhipu/glm-4.5", 0.6, 2.2),
        ("zhipu/glm-4.5-air", 0.2, 1.1),
        ("google/gemini-2.0-flash", 0.10, 0.40),
        ("google/gemini-1.5-pro", 1.25, 5.0),
        ("google/gemini-2.5-pro", 1.25, 10.0),
        ("google/gemini-2.5-flash", 0.30, 2.50),
        ("google/gemini-2.5-flash-lite", 0.10, 0.40),
        ("xai/grok-4", 3.0, 15.0),
        ("xai/grok-4-fast", 0.2, 0.5),
        ("xai/grok-code-fast-1", 0.2, 1.5),
        ("mistral/mistral-large", 2.0, 6.0),
    ];

    DEFAULTS
        .iter()
        .map(|(model, input, output)| {
            (
                (*model).to_string(),
                ModelPricing {
                    input: *input,
                    output: *output,
                },
            )
        })
        .collect()
}
