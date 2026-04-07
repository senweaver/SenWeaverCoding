// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Query configuration — mirrors claude-code-typescript-src`query/config.ts`.

use serde::{Deserialize, Serialize};

/// Configuration for a single query to the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryConfig {
    /// Model identifier to use for this query.
    pub model: String,
    /// Maximum tokens the model may produce.
    pub max_output_tokens: u32,
    /// Context window size (input + output combined).
    pub context_window: u32,
    /// Whether extended thinking is enabled.
    pub thinking_enabled: bool,
    /// Budget tokens for thinking (when thinking is enabled).
    pub thinking_budget_tokens: Option<u32>,
    /// Temperature (0.0–1.0).
    pub temperature: Option<f32>,
    /// Whether to use streaming.
    pub stream: bool,
    /// System prompt cache TTL hint (seconds); `None` = default.
    pub cache_ttl_secs: Option<u32>,
    /// Whether this is a fast-mode query (reduced quality, lower latency).
    pub fast_mode: bool,
    /// Query source label (for analytics).
    pub source: QuerySource,
}

impl Default for QueryConfig {
    fn default() -> Self {
        Self {
            model: String::new(),
            max_output_tokens: 16384,
            context_window: 200_000,
            thinking_enabled: false,
            thinking_budget_tokens: None,
            temperature: None,
            stream: true,
            cache_ttl_secs: None,
            fast_mode: false,
            source: QuerySource::MainLoop,
        }
    }
}

/// Identifies the originator of a query for telemetry and cost attribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuerySource {
    MainLoop,
    SubAgent,
    Classifier,
    Compact,
    AutoTitle,
    PlanMode,
    Dream,
    Advisor,
    SkillExecution,
    Coordinator,
}

impl std::fmt::Display for QuerySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::MainLoop => "main_loop",
            Self::SubAgent => "sub_agent",
            Self::Classifier => "classifier",
            Self::Compact => "compact",
            Self::AutoTitle => "auto_title",
            Self::PlanMode => "plan_mode",
            Self::Dream => "dream",
            Self::Advisor => "advisor",
            Self::SkillExecution => "skill_execution",
            Self::Coordinator => "coordinator",
        };
        f.write_str(s)
    }
}
