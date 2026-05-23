// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SopConfig {

    #[serde(default)]
    pub sops_dir: Option<String>,

    #[serde(default = "default_sop_execution_mode")]
    pub default_execution_mode: String,

    #[serde(default = "default_sop_max_concurrent_total")]
    pub max_concurrent_total: usize,

    #[serde(default = "default_sop_approval_timeout_secs")]
    pub approval_timeout_secs: u64,

    #[serde(default = "default_sop_max_finished_runs")]
    pub max_finished_runs: usize,
}

fn default_sop_execution_mode() -> String {
    "supervised".to_string()
}

fn default_sop_max_concurrent_total() -> usize {
    4
}

fn default_sop_approval_timeout_secs() -> u64 {
    300
}

fn default_sop_max_finished_runs() -> usize {
    100
}

impl Default for SopConfig {
    fn default() -> Self {
        Self {
            sops_dir: None,
            default_execution_mode: default_sop_execution_mode(),
            max_concurrent_total: default_sop_max_concurrent_total(),
            approval_timeout_secs: default_sop_approval_timeout_secs(),
            max_finished_runs: default_sop_max_finished_runs(),
        }
    }
}

