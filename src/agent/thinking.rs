// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingLevel {

    Off,

    Minimal,

    Low,

    #[default]
    Medium,

    High,

    Max,
}

impl ThinkingLevel {

    pub fn from_str_insensitive(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "off" | "none" => Some(Self::Off),
            "minimal" | "min" => Some(Self::Minimal),
            "low" => Some(Self::Low),
            "medium" | "med" | "default" => Some(Self::Medium),
            "high" => Some(Self::High),
            "max" | "maximum" => Some(Self::Max),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ThinkingConfig {

    #[serde(default)]
    pub default_level: ThinkingLevel,
}

impl Default for ThinkingConfig {
    fn default() -> Self {
        Self {
            default_level: ThinkingLevel::Medium,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThinkingParams {

    pub temperature_adjustment: f64,

    pub max_tokens_adjustment: i64,

    pub system_prompt_prefix: Option<String>,
}

pub fn parse_thinking_directive(message: &str) -> Option<(ThinkingLevel, String)> {
    let trimmed = message.trim_start();
    if !trimmed.starts_with("/think:") {
        return None;
    }

    let after_prefix = &trimmed["/think:".len()..];
    let level_end = after_prefix
        .find(|c: char| c.is_whitespace())
        .unwrap_or(after_prefix.len());
    let level_str = &after_prefix[..level_end];

    let level = ThinkingLevel::from_str_insensitive(level_str)?;

    let remaining = after_prefix[level_end..].trim_start().to_string();
    Some((level, remaining))
}

pub fn apply_thinking_level(level: ThinkingLevel) -> ThinkingParams {
    match level {
        ThinkingLevel::Off => ThinkingParams {
            temperature_adjustment: -0.2,
            max_tokens_adjustment: -1000,
            system_prompt_prefix: Some(
                "Be extremely concise. Give direct answers without explanation \
                 unless explicitly asked. No preamble."
                    .into(),
            ),
        },
        ThinkingLevel::Minimal => ThinkingParams {
            temperature_adjustment: -0.1,
            max_tokens_adjustment: -500,
            system_prompt_prefix: Some(
                "Be concise and fast. Keep explanations brief. \
                 Prioritize speed over thoroughness."
                    .into(),
            ),
        },
        ThinkingLevel::Low => ThinkingParams {
            temperature_adjustment: -0.05,
            max_tokens_adjustment: 0,
            system_prompt_prefix: Some("Keep reasoning light. Explain only when helpful.".into()),
        },
        ThinkingLevel::Medium => ThinkingParams {
            temperature_adjustment: 0.0,
            max_tokens_adjustment: 0,
            system_prompt_prefix: None,
        },
        ThinkingLevel::High => ThinkingParams {
            temperature_adjustment: 0.05,
            max_tokens_adjustment: 1000,
            system_prompt_prefix: Some(
                "Think step by step. Provide thorough analysis and \
                 consider edge cases before answering."
                    .into(),
            ),
        },
        ThinkingLevel::Max => ThinkingParams {
            temperature_adjustment: 0.1,
            max_tokens_adjustment: 2000,
            system_prompt_prefix: Some(
                "Think very carefully and exhaustively. Break down the problem \
                 into sub-problems, consider all angles, verify your reasoning, \
                 and provide the most thorough analysis possible."
                    .into(),
            ),
        },
    }
}

pub fn resolve_thinking_level(
    inline_directive: Option<ThinkingLevel>,
    session_override: Option<ThinkingLevel>,
    config: &ThinkingConfig,
) -> ThinkingLevel {
    inline_directive
        .or(session_override)
        .unwrap_or(config.default_level)
}

pub fn clamp_temperature(temp: f64) -> f64 {
    temp.clamp(0.0, 2.0)
}
