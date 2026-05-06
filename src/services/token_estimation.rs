// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Token estimation service — mirrors claude-code-typescript-src`services/tokenEstimation.ts`.
// Fast approximate token counting for budget management without a full tokenizer.

pub fn estimate_tokens(text: &str) -> u64 {

    let chars = text.len() as f64;
    (chars / 3.5).ceil() as u64
}

pub fn estimate_json_tokens(value: &serde_json::Value) -> u64 {
    let serialized = serde_json::to_string(value).unwrap_or_default();
    estimate_tokens(&serialized)
}

pub fn estimate_tool_definition_tokens(
    name: &str,
    description: &str,
    schema: &serde_json::Value,
) -> u64 {
    let name_tokens = estimate_tokens(name);
    let desc_tokens = estimate_tokens(description);
    let schema_tokens = estimate_json_tokens(schema);

    let overhead = 20;
    name_tokens + desc_tokens + schema_tokens + overhead
}

pub fn estimate_message_tokens(_role: &str, content: &str) -> u64 {

    let role_overhead = 4;
    role_overhead + estimate_tokens(content)
}

pub struct TokenEstimator {
    chars_per_token: f64,
}

impl TokenEstimator {
    pub fn new(chars_per_token: f64) -> Self {
        Self { chars_per_token }
    }

    pub fn estimate(&self, text: &str) -> u64 {
        (text.len() as f64 / self.chars_per_token).ceil() as u64
    }

    pub fn estimate_conversation(&self, messages: &[(String, String)]) -> u64 {
        messages
            .iter()
            .map(|(role, content)| 4 + self.estimate(content) + self.estimate(role))
            .sum()
    }

    pub fn chars_for_budget(&self, token_budget: u64) -> usize {
        (token_budget as f64 * self.chars_per_token) as usize
    }
}

impl Default for TokenEstimator {
    fn default() -> Self {
        Self {
            chars_per_token: 3.5,
        }
    }
}
