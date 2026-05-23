// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PipelineConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_pipeline_max_steps")]
    pub max_steps: usize,

    #[serde(default)]
    pub allowed_tools: Vec<String>,
}

pub(crate) fn default_pipeline_max_steps() -> usize {
    20
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_steps: default_pipeline_max_steps(),
            allowed_tools: Vec::new(),
        }
    }
}

impl PipelineConfig {

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if !self.enabled {
            return errors;
        }
        if self.max_steps == 0 {
            errors.push("pipeline.max_steps must be >= 1 when enabled".into());
        }
        if self.max_steps > 1_000 {
            errors.push("pipeline.max_steps > 1000 is likely misconfigured (DoS risk)".into());
        }
        for (i, t) in self.allowed_tools.iter().enumerate() {
            if t.trim().is_empty() {
                errors.push(format!("pipeline.allowed_tools[{i}] is empty"));
            }
        }
        errors
    }
}
