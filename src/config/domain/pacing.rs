// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PacingConfig {

    #[serde(default)]
    pub step_timeout_secs: Option<u64>,

    #[serde(default)]
    pub loop_detection_min_elapsed_secs: Option<u64>,

    #[serde(default)]
    pub loop_ignore_tools: Vec<String>,

    #[serde(default)]
    pub message_timeout_scale_max: Option<u64>,

    #[serde(default = "default_loop_detection_enabled")]
    pub loop_detection_enabled: bool,

    #[serde(default = "default_loop_detection_window_size")]
    pub loop_detection_window_size: usize,

    #[serde(default = "default_loop_detection_max_repeats")]
    pub loop_detection_max_repeats: usize,

    #[serde(default = "default_loop_detection_identical_output_threshold")]
    pub loop_detection_identical_output_threshold: u32,
}

pub(crate) fn default_loop_detection_enabled() -> bool {
    true
}

pub(crate) fn default_loop_detection_window_size() -> usize {
    20
}

pub(crate) fn default_loop_detection_max_repeats() -> usize {
    3
}

pub(crate) fn default_loop_detection_identical_output_threshold() -> u32 {
    5
}

impl Default for PacingConfig {
    fn default() -> Self {
        Self {
            step_timeout_secs: None,
            loop_detection_min_elapsed_secs: None,
            loop_ignore_tools: Vec::new(),
            message_timeout_scale_max: None,
            loop_detection_enabled: default_loop_detection_enabled(),
            loop_detection_window_size: default_loop_detection_window_size(),
            loop_detection_max_repeats: default_loop_detection_max_repeats(),
            loop_detection_identical_output_threshold:
                default_loop_detection_identical_output_threshold(),
        }
    }
}

impl PacingConfig {

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.loop_detection_enabled && self.loop_detection_window_size == 0 {
            errors.push(
                "pacing.loop_detection_window_size must be >= 1 when detection is enabled".into(),
            );
        }
        if self.loop_detection_enabled && self.loop_detection_max_repeats == 0 {
            errors.push(
                "pacing.loop_detection_max_repeats must be >= 1 when detection is enabled".into(),
            );
        }
        if self.loop_detection_enabled && self.loop_detection_identical_output_threshold == 0 {
            errors.push(
                "pacing.loop_detection_identical_output_threshold must be >= 1 when detection is enabled"
                    .into(),
            );
        }
        if self.loop_detection_window_size < self.loop_detection_max_repeats {
            errors.push("pacing.loop_detection_window_size must be >= max_repeats".into());
        }
        if let Some(t) = self.step_timeout_secs {
            if t == 0 {
                errors.push("pacing.step_timeout_secs must be > 0 when set".into());
            }
        }
        errors
    }
}
