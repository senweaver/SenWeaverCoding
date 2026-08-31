// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PlanModeConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_auto_threshold")]
    pub auto_activate_threshold: usize,

    #[serde(default = "default_max_todos")]
    pub max_todos: usize,
}

fn default_auto_threshold() -> usize {
    3
}
fn default_max_todos() -> usize {
    20
}

impl Default for PlanModeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_activate_threshold: default_auto_threshold(),
            max_todos: default_max_todos(),
        }
    }
}
