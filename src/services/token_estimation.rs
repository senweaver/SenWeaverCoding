// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Token estimation service — mirrors claude-code-typescript-src`services/tokenEstimation.ts`.
// Fast approximate token counting for budget management without a full tokenizer.

/// Estimate token count from text using the ~4 chars/token heuristic.
pub fn estimate_tokens(text: &str) -> u64 {
    // Claude tokenizer averages ~3.5–4 chars per token for English.
    // For mixed content (code + prose) we use 3.5.
    let chars = text.len() as f64;
    (chars / 3.5).ceil() as u64
}

/// Estimate tokens for a JSON-serializable value.
pub fn estimate_json_tokens(value: &serde_json::Value) -> u64 {
    let serialized = serde_json::to_string(value).unwrap_or_default();
    estimate_tokens(&serialized)
}

/// Estimate tokens for a tool definition (name + description + schema).
pub fn estimate_tool_definition_tokens(
    name: &str,
    description: &str,
    schema: &serde_json::Value,
) -> u64 {
    let name_tokens = estimate_tokens(name);
    let desc_tokens = estimate_tokens(description);
    let schema_tokens = estimate_json_tokens(schema);
    // Overhead for XML/JSON wrapping
    let overhead = 20;
    name_tokens + desc_tokens + schema_tokens + overhead
}

/// Estimate tokens for a conversation message.
pub fn estimate_message_tokens(_role: &str, content: &str) -> u64 {
    // Role prefix overhead (~4 tokens) + content
    let role_overhead = 4;
    role_overhead + estimate_tokens(content)
}

/// Token estimator with configurable chars-per-token ratio.
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

    /// Estimate tokens for an entire conversation.
    pub fn estimate_conversation(&self, messages: &[(String, String)]) -> u64 {
        messages
            .iter()
            .map(|(role, content)| 4 + self.estimate(content) + self.estimate(role))
            .sum()
    }

    /// Estimate how many characters fit in a token budget.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        let text = "Hello, world!"; // 13 chars
        let tokens = estimate_tokens(text);
        assert!(tokens >= 3 && tokens <= 5);
    }

    #[test]
    fn test_estimator_chars_for_budget() {
        let est = TokenEstimator::default();
        let chars = est.chars_for_budget(1000);
        assert_eq!(chars, 3500);
    }
}
