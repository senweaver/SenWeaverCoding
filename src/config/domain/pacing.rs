// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PacingConfig {

    #[serde(default)]
    pub step_timeout_secs: Option<u64>,

    #[serde(default = "default_tool_timeout_secs")]
    pub tool_timeout_secs: Option<u64>,

    #[serde(default)]
    pub total_turn_timeout_secs: Option<u64>,

    #[serde(default = "default_stream_idle_timeout_secs")]
    pub stream_idle_timeout_secs: Option<u64>,

    #[serde(default = "default_loop_detection_min_elapsed_secs")]
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

    #[serde(default = "default_no_progress_iteration_limit")]
    pub no_progress_iteration_limit: usize,

    // When a turn modified code files, run the workspace verification pipeline
    // (cargo check / tsc --noEmit / go vet / pytest --collect-only, whichever the
    // repo has) before finalizing and feed real build/type errors back to the
    // model instead of trusting it to self-verify. This is the deterministic
    // "don't say done until it builds" gate.
    #[serde(default = "default_auto_verify_after_edit")]
    pub auto_verify_after_edit: bool,

    #[serde(default = "default_auto_verify_timeout_secs")]
    pub auto_verify_timeout_secs: u64,

    #[serde(default = "default_auto_verify_max_retries")]
    pub auto_verify_max_retries: u32,
}

pub(crate) fn default_auto_verify_after_edit() -> bool {
    true
}

pub(crate) fn default_auto_verify_timeout_secs() -> u64 {
    120
}

pub(crate) fn default_auto_verify_max_retries() -> u32 {
    2
}

pub(crate) fn default_stream_idle_timeout_secs() -> Option<u64> {
    Some(300)
}

pub(crate) fn default_loop_detection_min_elapsed_secs() -> Option<u64> {
    // Must match `Default` (Some(0)). With plain `#[serde(default)]` this
    // deserialized to None when a `[pacing]` table omitted the key, and the
    // loop_/mod.rs `None => false` branch then disabled the per-iteration
    // identical-output circuit breaker entirely.
    Some(0)
}

pub(crate) fn default_tool_timeout_secs() -> Option<u64> {
    Some(600)
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

pub(crate) fn default_no_progress_iteration_limit() -> usize {
    200
}

impl Default for PacingConfig {
    fn default() -> Self {
        Self {
            step_timeout_secs: None,
            tool_timeout_secs: default_tool_timeout_secs(),
            total_turn_timeout_secs: None,
            stream_idle_timeout_secs: default_stream_idle_timeout_secs(),
            loop_detection_min_elapsed_secs: Some(0),
            loop_ignore_tools: Vec::new(),
            message_timeout_scale_max: None,
            loop_detection_enabled: default_loop_detection_enabled(),
            loop_detection_window_size: default_loop_detection_window_size(),
            loop_detection_max_repeats: default_loop_detection_max_repeats(),
            loop_detection_identical_output_threshold:
                default_loop_detection_identical_output_threshold(),
            no_progress_iteration_limit: default_no_progress_iteration_limit(),
            auto_verify_after_edit: default_auto_verify_after_edit(),
            auto_verify_timeout_secs: default_auto_verify_timeout_secs(),
            auto_verify_max_retries: default_auto_verify_max_retries(),
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
        if let Some(t) = self.tool_timeout_secs {
            if t == 0 {
                errors.push("pacing.tool_timeout_secs must be > 0 when set".into());
            }
        }
        if let Some(t) = self.total_turn_timeout_secs {
            if t == 0 {
                errors.push("pacing.total_turn_timeout_secs must be > 0 when set".into());
            }
        }
        if let Some(t) = self.stream_idle_timeout_secs {
            if t == 0 {
                errors.push("pacing.stream_idle_timeout_secs must be > 0 when set".into());
            }
        }
        if self.no_progress_iteration_limit == 0 {
            errors.push("pacing.no_progress_iteration_limit must be >= 1".into());
        }
        errors
    }
}
