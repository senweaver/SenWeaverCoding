// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvolutionExportFormat {
    #[default]
    OpenaiSft,
    OpenaiDpo,
    AnthropicMessages,
    HfTrlDpo,
    RlSar,
    AgentTrajectory,
}

impl EvolutionExportFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenaiSft => "openai_sft",
            Self::OpenaiDpo => "openai_dpo",
            Self::AnthropicMessages => "anthropic_messages",
            Self::HfTrlDpo => "hf_trl_dpo",
            Self::RlSar => "rl_sar",
            Self::AgentTrajectory => "agent_trajectory",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "openai_sft" => Some(Self::OpenaiSft),
            "openai_dpo" => Some(Self::OpenaiDpo),
            "anthropic_messages" => Some(Self::AnthropicMessages),
            "hf_trl_dpo" => Some(Self::HfTrlDpo),
            "rl_sar" => Some(Self::RlSar),
            "agent_trajectory" => Some(Self::AgentTrajectory),
            _ => None,
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::OpenaiSft,
            Self::OpenaiDpo,
            Self::AnthropicMessages,
            Self::HfTrlDpo,
            Self::RlSar,
            Self::AgentTrajectory,
        ]
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
pub struct EvolutionSignalWeights {
    #[serde(default = "default_weight_thumbs")]
    pub thumbs: f32,
    #[serde(default = "default_weight_next_state")]
    pub next_state: f32,
    #[serde(default = "default_weight_tool")]
    pub tool: f32,
    #[serde(default = "default_weight_verification")]
    pub verification: f32,
    #[serde(default = "default_weight_cost")]
    pub cost: f32,
}

fn default_weight_thumbs() -> f32 {
    0.40
}
fn default_weight_next_state() -> f32 {
    0.25
}
fn default_weight_tool() -> f32 {
    0.15
}
fn default_weight_verification() -> f32 {
    0.10
}
fn default_weight_cost() -> f32 {
    0.10
}

impl Default for EvolutionSignalWeights {
    fn default() -> Self {
        Self {
            thumbs: default_weight_thumbs(),
            next_state: default_weight_next_state(),
            tool: default_weight_tool(),
            verification: default_weight_verification(),
            cost: default_weight_cost(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EvolutionExportConfig {
    #[serde(default)]
    pub export_dir: Option<PathBuf>,

    #[serde(default)]
    pub default_format: EvolutionExportFormat,

    #[serde(default)]
    pub auto_push: bool,

    #[serde(default)]
    pub auto_push_target_id: Option<String>,

    #[serde(default = "default_auto_push_min_samples")]
    pub auto_push_min_samples: usize,

    #[serde(default = "default_auto_push_min_interval_hours")]
    pub auto_push_min_interval_hours: u32,

    #[serde(default = "default_redact_workspace_paths")]
    pub redact_workspace_paths: bool,

    #[serde(default = "default_redact_secrets")]
    pub redact_secrets: bool,
}

fn default_auto_push_min_samples() -> usize {
    100
}
fn default_auto_push_min_interval_hours() -> u32 {
    24
}
fn default_redact_workspace_paths() -> bool {
    true
}
fn default_redact_secrets() -> bool {
    true
}

impl Default for EvolutionExportConfig {
    fn default() -> Self {
        Self {
            export_dir: None,
            default_format: EvolutionExportFormat::default(),
            auto_push: false,
            auto_push_target_id: None,
            auto_push_min_samples: default_auto_push_min_samples(),
            auto_push_min_interval_hours: default_auto_push_min_interval_hours(),
            redact_workspace_paths: default_redact_workspace_paths(),
            redact_secrets: default_redact_secrets(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EvolutionConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    #[serde(default)]
    pub persist_training_data: bool,

    #[serde(default = "default_enabled")]
    pub next_state_judge_enabled: bool,

    #[serde(default)]
    pub judge_model: Option<String>,

    #[serde(default)]
    pub signal_weights: EvolutionSignalWeights,

    #[serde(default = "default_max_lessons_in_prompt")]
    pub max_lessons_in_prompt: usize,

    #[serde(default = "default_lesson_token_budget")]
    pub lesson_token_budget: usize,

    #[serde(default = "default_enabled")]
    pub auto_distill_on_session_end: bool,

    #[serde(default)]
    pub export: EvolutionExportConfig,
}

fn default_enabled() -> bool {
    true
}
fn default_max_lessons_in_prompt() -> usize {
    6
}
fn default_lesson_token_budget() -> usize {
    1500
}

impl Default for EvolutionConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            persist_training_data: false,
            next_state_judge_enabled: default_enabled(),
            judge_model: None,
            signal_weights: EvolutionSignalWeights::default(),
            max_lessons_in_prompt: default_max_lessons_in_prompt(),
            lesson_token_budget: default_lesson_token_budget(),
            auto_distill_on_session_end: default_enabled(),
            export: EvolutionExportConfig::default(),
        }
    }
}
