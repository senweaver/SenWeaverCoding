// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DelegateAgentConfig {

    pub provider: String,

    pub model: String,

    #[serde(default)]
    pub system_prompt: Option<String>,

    #[serde(default)]
    pub api_key: Option<String>,

    #[serde(default)]
    pub temperature: Option<f64>,

    #[serde(default = "default_max_depth")]
    pub max_depth: u32,

    #[serde(default)]
    pub agentic: bool,

    #[serde(default)]
    pub allowed_tools: Vec<String>,

    #[serde(default = "default_max_tool_iterations")]
    pub max_iterations: usize,

    #[serde(default)]
    pub timeout_secs: Option<u64>,

    #[serde(default)]
    pub agentic_timeout_secs: Option<u64>,

    #[serde(default)]
    pub skills_directory: Option<String>,
}

pub(crate) fn default_max_depth() -> u32 {
    3
}

pub(crate) fn default_max_tool_iterations() -> usize {

    50
}

impl DelegateAgentConfig {

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.provider.trim().is_empty() {
            errors.push("provider must be non-empty".into());
        }
        if self.model.trim().is_empty() {
            errors.push("model must be non-empty".into());
        }
        if self.max_depth == 0 {
            errors.push("max_depth must be >= 1".into());
        }
        if self.max_depth > 10 {
            errors.push("max_depth > 10 risks runaway recursion".into());
        }
        if self.max_iterations == 0 {
            errors.push("max_iterations must be >= 1".into());
        }
        if let Some(t) = self.temperature {
            if !(0.0..=2.0).contains(&t) {
                errors.push(format!("temperature {t} out of range [0.0, 2.0]"));
            }
        }
        if let Some(ts) = self.timeout_secs {
            if ts == 0 {
                errors.push("timeout_secs must be > 0".into());
            }
        }
        if let Some(ts) = self.agentic_timeout_secs {
            if ts == 0 {
                errors.push("agentic_timeout_secs must be > 0".into());
            }
        }
        errors
    }
}
