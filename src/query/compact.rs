// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Conversation compaction — reduces message history when token budget is exceeded.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionStrategy {

    None,

    Summarize,

    SlidingWindow,

    Auto,
}

impl Default for CompactionStrategy {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionConfig {
    pub strategy: CompactionStrategy,

    pub trigger_threshold: f64,

    pub target_ratio: f64,

    pub min_keep_messages: usize,

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

pub fn should_compact(current_tokens: usize, max_tokens: usize, config: &CompactionConfig) -> bool {
    if config.strategy == CompactionStrategy::None || max_tokens == 0 {
        return false;
    }
    let ratio = current_tokens as f64 / max_tokens as f64;
    ratio >= config.trigger_threshold
}

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

        for msg in messages {
            if msg.get("role").and_then(|r| r.as_str()) == Some("system") {
                result.push(msg.clone());
            } else {
                break;
            }
        }
    }

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

pub fn create_collapse_marker(collapsed_count: usize, summary: &str) -> serde_json::Value {
    serde_json::json!({
        "role": "system",
        "content": format!(
            "[Context collapsed: {} messages summarized]\n{}",
            collapsed_count, summary
        )
    })
}
