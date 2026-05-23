// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryConfig {

    pub model: String,

    pub max_output_tokens: u32,

    pub context_window: u32,

    pub thinking_enabled: bool,

    pub thinking_budget_tokens: Option<u32>,

    pub temperature: Option<f32>,

    pub stream: bool,

    pub cache_ttl_secs: Option<u32>,

    pub fast_mode: bool,

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
