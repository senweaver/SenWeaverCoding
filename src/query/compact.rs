// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Conversation compaction — reduces message history when token budget is exceeded.

use serde::{Deserialize, Serialize};

/// Compaction strategy for managing conversation length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionStrategy {
    /// No compaction - let the conversation grow
    None,
    /// Summarize old messages into a condensed summary
    Summarize,
    /// Drop old messages beyond a sliding window
    SlidingWindow,
    /// Smart compaction using token counting
    Auto,
}

impl Default for CompactionStrategy {
    fn default() -> Self {
        Self::Auto
    }
}

/// Configuration for auto-compaction behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionConfig {
    pub strategy: CompactionStrategy,
    /// Token threshold to trigger compaction (default: 80% of max tokens)
    pub trigger_threshold: f64,
    /// Target token count after compaction (default: 50% of max tokens)
    pub target_ratio: f64,
    /// Minimum messages to keep even after compaction
    pub min_keep_messages: usize,
    /// Keep system message intact during compaction
    pub preserve_system: bool,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            strategy: CompactionStrategy::Auto,
            trigger_threshold: 0.8,
            target_ratio: 0.5,
            min_keep_messages: 4,
            preserve_system: true,
        }
    }
}

/// Check if compaction should be triggered based on current token usage.
pub fn should_compact(current_tokens: usize, max_tokens: usize, config: &CompactionConfig) -> bool {
    if config.strategy == CompactionStrategy::None || max_tokens == 0 {
        return false;
    }
    let ratio = current_tokens as f64 / max_tokens as f64;
    ratio >= config.trigger_threshold
}

/// Apply sliding-window compaction: keep only the last N messages.
pub fn sliding_window_compact(
    messages: &[serde_json::Value],
    keep_count: usize,
    preserve_system: bool,
) -> Vec<serde_json::Value> {
    if messages.len() <= keep_count {
        return messages.to_vec();
    }

    let mut result = Vec::new();

    if preserve_system {
        // Keep system message(s) at the start
        for msg in messages {
            if msg.get("role").and_then(|r| r.as_str()) == Some("system") {
                result.push(msg.clone());
            } else {
                break;
            }
        }
    }

    // Keep the last `keep_count` non-system messages
    let non_system: Vec<_> = messages
        .iter()
        .filter(|m| m.get("role").and_then(|r| r.as_str()) != Some("system"))
        .collect();

    let start = non_system.len().saturating_sub(keep_count);
    for msg in &non_system[start..] {
        result.push((*msg).clone());
    }

    result
}

/// Context collapse: generate a summary marker for collapsed messages.
pub fn create_collapse_marker(collapsed_count: usize, summary: &str) -> serde_json::Value {
    serde_json::json!({
        "role": "system",
        "content": format!(
            "[Context collapsed: {} messages summarized]\n{}",
            collapsed_count, summary
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_strategy_is_auto() {
        assert_eq!(
            CompactionConfig::default().strategy,
            CompactionStrategy::Auto
        );
    }

    #[test]
    fn should_not_compact_when_below_threshold() {
        let config = CompactionConfig::default();
        assert!(!should_compact(100, 1000, &config));
    }

    #[test]
    fn should_compact_when_above_threshold() {
        let config = CompactionConfig::default();
        assert!(should_compact(900, 1000, &config));
    }

    #[test]
    fn should_not_compact_when_strategy_is_none() {
        let config = CompactionConfig {
            strategy: CompactionStrategy::None,
            ..Default::default()
        };
        assert!(!should_compact(900, 1000, &config));
    }

    #[test]
    fn sliding_window_keeps_recent() {
        let messages = vec![
            json!({"role": "system", "content": "sys"}),
            json!({"role": "user", "content": "msg1"}),
            json!({"role": "assistant", "content": "resp1"}),
            json!({"role": "user", "content": "msg2"}),
            json!({"role": "assistant", "content": "resp2"}),
        ];
        let result = sliding_window_compact(&messages, 2, true);
        // System + last 2 non-system
        assert_eq!(result.len(), 3);
        assert_eq!(result[0]["role"], "system");
        assert_eq!(result[1]["content"], "msg2");
        assert_eq!(result[2]["content"], "resp2");
    }

    #[test]
    fn collapse_marker_format() {
        let marker = create_collapse_marker(5, "User discussed project setup");
        assert!(marker["content"].as_str().unwrap().contains("5 messages"));
    }
}
