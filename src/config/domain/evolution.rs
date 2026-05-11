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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionTriggerMode {
    Manual,
    Auto,
    Scheduled,
}

impl Default for ReflectionTriggerMode {
    fn default() -> Self {
        Self::Manual
    }
}

impl ReflectionTriggerMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Auto => "auto",
            Self::Scheduled => "scheduled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "manual" => Some(Self::Manual),
            "auto" => Some(Self::Auto),
            "scheduled" => Some(Self::Scheduled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionDepth {
    Quick,
    Deep,
}

impl Default for ReflectionDepth {
    fn default() -> Self {
        Self::Quick
    }
}

impl ReflectionDepth {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Quick => "quick",
            Self::Deep => "deep",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "quick" => Some(Self::Quick),
            "deep" => Some(Self::Deep),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionWritebackTarget {
    Lessons,
    Skills,
    Rules,
    Memory,
}

impl ReflectionWritebackTarget {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lessons => "lessons",
            Self::Skills => "skills",
            Self::Rules => "rules",
            Self::Memory => "memory",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "lessons" => Some(Self::Lessons),
            "skills" => Some(Self::Skills),
            "rules" => Some(Self::Rules),
            "memory" => Some(Self::Memory),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExperienceRecyclingConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_recycling_sample_rate")]
    pub sample_rate: f32,

    #[serde(default = "default_recycling_min_reward")]
    pub min_reward: f32,

    #[serde(default = "default_recycling_max_retained")]
    pub max_retained: usize,

    #[serde(default = "default_recycling_max_replay")]
    pub max_replay_in_prompt: usize,

    #[serde(default = "default_recycling_replay_budget")]
    pub replay_token_budget: usize,

    #[serde(default = "default_recycling_redact_paths")]
    pub redact_workspace_paths: bool,

    #[serde(default = "default_recycling_redact_secrets")]
    pub redact_secrets: bool,

    #[serde(default)]
    pub redact_user_text: bool,

    #[serde(default = "default_recycling_include_successes")]
    pub include_successes: bool,

    #[serde(default = "default_recycling_include_failures")]
    pub include_failures: bool,

    #[serde(default = "default_recycling_weight_quality")]
    pub weight_quality: f32,

    #[serde(default = "default_recycling_weight_recency")]
    pub weight_recency: f32,

    #[serde(default = "default_recycling_weight_diversity")]
    pub weight_diversity: f32,
}

fn default_recycling_sample_rate() -> f32 {
    1.0
}
fn default_recycling_min_reward() -> f32 {
    -0.2
}
fn default_recycling_max_retained() -> usize {
    500
}
fn default_recycling_max_replay() -> usize {
    3
}
fn default_recycling_replay_budget() -> usize {
    800
}
fn default_recycling_redact_paths() -> bool {
    true
}
fn default_recycling_redact_secrets() -> bool {
    true
}
fn default_recycling_include_successes() -> bool {
    true
}
fn default_recycling_include_failures() -> bool {
    true
}
fn default_recycling_weight_quality() -> f32 {
    0.5
}
fn default_recycling_weight_recency() -> f32 {
    0.3
}
fn default_recycling_weight_diversity() -> f32 {
    0.2
}

impl Default for ExperienceRecyclingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            sample_rate: default_recycling_sample_rate(),
            min_reward: default_recycling_min_reward(),
            max_retained: default_recycling_max_retained(),
            max_replay_in_prompt: default_recycling_max_replay(),
            replay_token_budget: default_recycling_replay_budget(),
            redact_workspace_paths: default_recycling_redact_paths(),
            redact_secrets: default_recycling_redact_secrets(),
            redact_user_text: false,
            include_successes: default_recycling_include_successes(),
            include_failures: default_recycling_include_failures(),
            weight_quality: default_recycling_weight_quality(),
            weight_recency: default_recycling_weight_recency(),
            weight_diversity: default_recycling_weight_diversity(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SelfReflectionConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub trigger_mode: ReflectionTriggerMode,

    #[serde(default)]
    pub depth: ReflectionDepth,

    #[serde(default)]
    pub reflection_model: Option<String>,

    #[serde(default)]
    pub reflection_provider: Option<String>,

    #[serde(default = "default_reflection_schedule_minutes")]
    pub schedule_interval_minutes: u32,

    #[serde(default = "default_reflection_min_turns_for_auto")]
    pub min_turns_for_auto: usize,

    #[serde(default = "default_reflection_failure_threshold")]
    pub failure_threshold: u32,

    #[serde(default = "default_reflection_writeback_targets")]
    pub writeback_targets: Vec<ReflectionWritebackTarget>,

    #[serde(default = "default_reflection_max_lessons_per_run")]
    pub max_lessons_per_run: usize,

    #[serde(default = "default_reflection_max_total_lessons")]
    pub max_total_lessons: usize,

    #[serde(default = "default_reflection_include_thumbs_down")]
    pub include_user_thumbs_down: bool,

    #[serde(default = "default_reflection_lookback_turns")]
    pub lookback_turns: usize,
}

fn default_reflection_schedule_minutes() -> u32 {
    60
}
fn default_reflection_min_turns_for_auto() -> usize {
    4
}
fn default_reflection_failure_threshold() -> u32 {
    2
}
fn default_reflection_writeback_targets() -> Vec<ReflectionWritebackTarget> {
    vec![ReflectionWritebackTarget::Lessons]
}
fn default_reflection_max_lessons_per_run() -> usize {
    3
}
fn default_reflection_max_total_lessons() -> usize {
    100
}
fn default_reflection_include_thumbs_down() -> bool {
    true
}
fn default_reflection_lookback_turns() -> usize {
    12
}

impl Default for SelfReflectionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            trigger_mode: ReflectionTriggerMode::default(),
            depth: ReflectionDepth::default(),
            reflection_model: None,
            reflection_provider: None,
            schedule_interval_minutes: default_reflection_schedule_minutes(),
            min_turns_for_auto: default_reflection_min_turns_for_auto(),
            failure_threshold: default_reflection_failure_threshold(),
            writeback_targets: default_reflection_writeback_targets(),
            max_lessons_per_run: default_reflection_max_lessons_per_run(),
            max_total_lessons: default_reflection_max_total_lessons(),
            include_user_thumbs_down: default_reflection_include_thumbs_down(),
            lookback_turns: default_reflection_lookback_turns(),
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

    #[serde(default)]
    pub recycling: ExperienceRecyclingConfig,

    #[serde(default)]
    pub reflection: SelfReflectionConfig,
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
            recycling: ExperienceRecyclingConfig::default(),
            reflection: SelfReflectionConfig::default(),
        }
    }
}
