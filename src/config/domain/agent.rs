// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//!
//! Agent-related configuration types extracted from `schema.rs`.
//!
//! This module contains AgentConfig, ToolFilterGroup, and related types.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolFilterGroupMode {

    Always,

    #[default]
    Dynamic,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolFilterGroup {

    #[serde(default)]
    pub mode: ToolFilterGroupMode,

    #[serde(default)]
    pub tools: Vec<String>,

    #[serde(default)]
    pub keywords: Vec<String>,

    #[serde(default)]
    pub filter_builtins: bool,
}

impl Default for ToolFilterGroup {
    fn default() -> Self {
        Self {
            mode: ToolFilterGroupMode::default(),
            tools: Vec::new(),
            keywords: Vec::new(),
            filter_builtins: false,
        }
    }
}

impl ToolFilterGroup {

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        for (i, tool) in self.tools.iter().enumerate() {
            if tool.is_empty() {
                errors.push(format!(
                    "agent.tool_filter_groups[{}].tools contains empty pattern",
                    i
                ));
            }
        }
        if matches!(self.mode, ToolFilterGroupMode::Dynamic) && self.keywords.is_empty() {
            errors.push(
                "agent.tool_filter_groups entry with mode='dynamic' should have at least one keyword".into(),
            );
        }
        errors
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GlobalDirective {

    pub content: String,

    #[serde(default)]
    pub mode: Option<String>,
}

impl Default for GlobalDirective {
    fn default() -> Self {
        Self {
            content: String::new(),
            mode: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AutoIndexConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_auto_index_include_patterns")]
    pub include_patterns: Vec<String>,

    #[serde(default = "default_auto_index_exclude_patterns")]
    pub exclude_patterns: Vec<String>,

    #[serde(default = "default_auto_index_max_files")]
    pub max_files: usize,

    #[serde(default = "default_auto_index_refresh")]
    pub refresh_interval_secs: u64,
}

impl Default for AutoIndexConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            include_patterns: default_auto_index_include_patterns(),
            exclude_patterns: default_auto_index_exclude_patterns(),
            max_files: default_auto_index_max_files(),
            refresh_interval_secs: default_auto_index_refresh(),
        }
    }
}

fn default_auto_index_include_patterns() -> Vec<String> {
    vec![
        "**/*.rs".to_string(),
        "**/*.ts".to_string(),
        "**/*.tsx".to_string(),
        "**/*.js".to_string(),
        "**/*.jsx".to_string(),
        "**/*.py".to_string(),
        "**/*.go".to_string(),
    ]
}

fn default_auto_index_exclude_patterns() -> Vec<String> {
    vec![
        "**/node_modules/**".to_string(),
        "**/target/**".to_string(),
        "**/.git/**".to_string(),
        "**/dist/**".to_string(),
        "**/build/**".to_string(),
    ]
}

fn default_auto_index_max_files() -> usize {
    10_000
}

fn default_auto_index_refresh() -> u64 {
    3600
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OpenAiSttConfig {

    #[serde(default)]
    pub api_key: Option<String>,

    #[serde(default = "default_openai_stt_model")]
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeepgramSttConfig {

    #[serde(default)]
    pub api_key: Option<String>,

    #[serde(default = "default_deepgram_stt_model")]
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AssemblyAiSttConfig {

    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoogleSttConfig {

    #[serde(default)]
    pub api_key: Option<String>,

    #[serde(default = "default_google_stt_language_code")]
    pub language_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LocalWhisperConfig {

    pub url: String,

    #[serde(default)]
    pub bearer_token: Option<String>,

    #[serde(default = "default_local_whisper_max_audio_bytes")]
    pub max_audio_bytes: usize,

    #[serde(default = "default_local_whisper_timeout_secs")]
    pub timeout_secs: u64,
}

fn default_openai_stt_model() -> String {
    "whisper-1".into()
}

fn default_deepgram_stt_model() -> String {
    "nova-2".into()
}

fn default_google_stt_language_code() -> String {
    "en-US".into()
}

fn default_local_whisper_max_audio_bytes() -> usize {
    25 * 1024 * 1024
}

fn default_local_whisper_timeout_secs() -> u64 {
    300
}

impl Default for OpenAiSttConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            model: default_openai_stt_model(),
        }
    }
}

impl Default for DeepgramSttConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            model: default_deepgram_stt_model(),
        }
    }
}

impl Default for AssemblyAiSttConfig {
    fn default() -> Self {
        Self { api_key: None }
    }
}

impl Default for GoogleSttConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            language_code: default_google_stt_language_code(),
        }
    }
}

impl Default for LocalWhisperConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            bearer_token: None,
            max_audio_bytes: default_local_whisper_max_audio_bytes(),
            timeout_secs: default_local_whisper_timeout_secs(),
        }
    }
}
