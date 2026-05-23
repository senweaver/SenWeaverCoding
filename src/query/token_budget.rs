// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};

#[must_use]
pub fn estimate_tokens(text: &str) -> u32 {
    let chars = text.chars().count() as u64;
    if chars == 0 {
        return 0;
    }
    chars.div_ceil(4) as u32
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBudget {

    pub context_window: u32,

    pub max_output_tokens: u32,

    pub system_prompt_tokens: u32,

    pub history_tokens: u32,

    pub tool_definition_tokens: u32,

    #[serde(default)]
    pub last_turn_input_tokens: u32,

    #[serde(default)]
    pub last_turn_output_tokens: u32,
}

impl TokenBudget {
    pub fn new(context_window: u32, max_output_tokens: u32) -> Self {
        Self {
            context_window,
            max_output_tokens,
            system_prompt_tokens: 0,
            history_tokens: 0,
            tool_definition_tokens: 0,
            last_turn_input_tokens: 0,
            last_turn_output_tokens: 0,
        }
    }

    pub fn record_turn(&mut self, input_tokens: u32, output_tokens: u32) {
        self.last_turn_input_tokens = input_tokens;
        self.last_turn_output_tokens = output_tokens;
    }

    pub fn consumed(&self) -> u32 {
        self.system_prompt_tokens + self.history_tokens + self.tool_definition_tokens
    }

    pub fn remaining_input(&self) -> u32 {
        self.context_window
            .saturating_sub(self.consumed())
            .saturating_sub(self.max_output_tokens)
    }

    pub fn is_exhausted(&self) -> bool {
        self.remaining_input() < 1000
    }

    pub fn utilization(&self) -> f64 {
        if self.context_window == 0 {
            return 0.0;
        }
        self.consumed() as f64 / self.context_window as f64
    }

    pub fn should_compact(&self, threshold: f64) -> bool {
        self.utilization() > threshold
    }

    pub fn set_system_prompt_tokens(&mut self, tokens: u32) {
        self.system_prompt_tokens = tokens;
    }

    pub fn set_history_tokens(&mut self, tokens: u32) {
        self.history_tokens = tokens;
    }

    pub fn set_tool_definition_tokens(&mut self, tokens: u32) {
        self.tool_definition_tokens = tokens;
    }
}
