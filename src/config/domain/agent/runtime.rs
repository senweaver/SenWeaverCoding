// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentRuntimeExtras {

    #[serde(default = "default_max_iterations")]
    pub max_tool_iterations: u32,

    #[serde(default = "default_parallel_tools")]
    pub parallel_tools_enabled: bool,

    #[serde(default = "default_per_turn_token_soft_cap")]
    pub per_turn_token_soft_cap: usize,

    #[serde(default = "default_per_turn_token_hard_cap")]
    pub per_turn_token_hard_cap: usize,

    #[serde(default = "default_max_subagents")]
    pub max_subagents: u32,

    #[serde(default = "default_parallel_tool_max_concurrency")]
    pub parallel_tool_max_concurrency: u32,

    #[serde(default)]
    pub subagent_limit: crate::agent::subagent::limiter::SubagentLimitConfig,

    #[serde(default = "default_subagent_call_timeout_secs")]
    pub subagent_call_timeout_secs: u64,

    #[serde(default)]
    pub fast_apply_model: Option<String>,

    #[serde(default = "default_fast_apply_temperature")]
    pub fast_apply_temperature: f64,

    #[serde(default = "default_fast_apply_timeout_secs")]
    pub fast_apply_timeout_secs: u64,

    #[serde(default)]
    pub self_consistency: SelfConsistencyConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct SelfConsistencyConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_self_consistency_samples")]
    pub samples: u32,

    #[serde(default = "default_self_consistency_temperature")]
    pub temperature: f64,

    #[serde(default = "default_self_consistency_max_concurrent")]
    pub max_concurrent: u32,

    #[serde(default = "default_self_consistency_final_only")]
    pub final_only: bool,
}

fn default_self_consistency_samples() -> u32 {
    3
}
fn default_self_consistency_temperature() -> f64 {
    0.7
}
fn default_self_consistency_max_concurrent() -> u32 {
    3
}
fn default_self_consistency_final_only() -> bool {
    true
}

impl Default for SelfConsistencyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            samples: default_self_consistency_samples(),
            temperature: default_self_consistency_temperature(),
            max_concurrent: default_self_consistency_max_concurrent(),
            final_only: default_self_consistency_final_only(),
        }
    }
}

fn default_max_iterations() -> u32 {

    2000
}
fn default_parallel_tools() -> bool {
    true
}
fn default_per_turn_token_soft_cap() -> usize {
    120_000
}
fn default_per_turn_token_hard_cap() -> usize {
    180_000
}
fn default_max_subagents() -> u32 {
    8
}
fn default_parallel_tool_max_concurrency() -> u32 {
    4
}
fn default_subagent_call_timeout_secs() -> u64 {
    120
}
fn default_fast_apply_temperature() -> f64 {
    0.0
}
fn default_fast_apply_timeout_secs() -> u64 {
    15
}

impl Default for AgentRuntimeExtras {
    fn default() -> Self {
        Self {
            max_tool_iterations: default_max_iterations(),
            parallel_tools_enabled: default_parallel_tools(),
            per_turn_token_soft_cap: default_per_turn_token_soft_cap(),
            per_turn_token_hard_cap: default_per_turn_token_hard_cap(),
            max_subagents: default_max_subagents(),
            parallel_tool_max_concurrency: default_parallel_tool_max_concurrency(),
            subagent_limit: crate::agent::subagent::limiter::SubagentLimitConfig::default(),
            subagent_call_timeout_secs: default_subagent_call_timeout_secs(),
            fast_apply_model: None,
            fast_apply_temperature: default_fast_apply_temperature(),
            fast_apply_timeout_secs: default_fast_apply_timeout_secs(),
            self_consistency: SelfConsistencyConfig::default(),
        }
    }
}

impl AgentRuntimeExtras {

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.max_tool_iterations == 0 {
            errors.push("agent_runtime.max_tool_iterations must be >= 1".into());
        }

        if self.max_tool_iterations > 10_000 {
            errors.push(
                "agent_runtime.max_tool_iterations > 10000 is almost certainly a misconfiguration"
                    .into(),
            );
        }
        if self.per_turn_token_hard_cap < self.per_turn_token_soft_cap {
            errors.push(
                "agent_runtime.per_turn_token_hard_cap must be >= per_turn_token_soft_cap".into(),
            );
        }
        if self.per_turn_token_soft_cap == 0 {
            errors.push("agent_runtime.per_turn_token_soft_cap must be > 0".into());
        }
        if self.max_subagents == 0 {
            errors.push("agent_runtime.max_subagents must be >= 1".into());
        }
        if self.parallel_tool_max_concurrency == 0 {
            errors.push("agent_runtime.parallel_tool_max_concurrency must be >= 1".into());
        }
        if self.parallel_tool_max_concurrency > 64 {
            errors.push("agent_runtime.parallel_tool_max_concurrency > 64 is excessive".into());
        }
        errors.extend(self.self_consistency.validate());
        errors
    }
}

impl SelfConsistencyConfig {

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.enabled {
            if self.samples == 0 {
                errors.push(
                    "agent_runtime.self_consistency.samples must be >= 1 when enabled".into(),
                );
            }
            if self.samples > 16 {
                errors.push(
                    "agent_runtime.self_consistency.samples > 16 is wasteful and slow".into(),
                );
            }
            if self.max_concurrent == 0 {
                errors.push(
                    "agent_runtime.self_consistency.max_concurrent must be >= 1 when enabled"
                        .into(),
                );
            }
            if !(0.0..=2.0).contains(&self.temperature) {
                errors.push(
                    "agent_runtime.self_consistency.temperature must lie in [0.0, 2.0]".into(),
                );
            }
        }
        errors
    }

    pub fn should_engage(&self) -> bool {
        self.enabled && self.samples > 1
    }

    pub fn effective_concurrency(&self) -> u32 {
        if self.max_concurrent == 0 {
            self.samples
        } else {
            self.max_concurrent.min(self.samples)
        }
    }
}
