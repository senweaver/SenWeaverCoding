// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::providers::traits::ChatMessage;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

fn default_max_tokens() -> usize {
    8192
}

fn default_keep_recent() -> usize {
    4
}

fn default_collapse() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HistoryPrunerConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,

    #[serde(default = "default_keep_recent")]
    pub keep_recent: usize,

    #[serde(default = "default_collapse")]
    pub collapse_tool_results: bool,
}

impl Default for HistoryPrunerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_tokens: 8192,
            keep_recent: 4,
            collapse_tool_results: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PruneStats {
    pub messages_before: usize,
    pub messages_after: usize,
    pub collapsed_pairs: usize,
    pub dropped_messages: usize,
}

fn estimate_tokens(messages: &[ChatMessage]) -> usize {
    messages.iter().map(|m| m.content.len() / 4).sum()
}

fn protected_indices(messages: &[ChatMessage], keep_recent: usize) -> Vec<bool> {
    let len = messages.len();
    let mut protected = vec![false; len];
    for (i, msg) in messages.iter().enumerate() {
        if msg.role == "system" {
            protected[i] = true;
        }
    }
    let recent_start = len.saturating_sub(keep_recent);
    for p in protected.iter_mut().skip(recent_start) {
        *p = true;
    }
    protected
}

pub fn prune_history(messages: &mut Vec<ChatMessage>, config: &HistoryPrunerConfig) -> PruneStats {
    let messages_before = messages.len();
    if !config.enabled || messages.is_empty() {
        return PruneStats {
            messages_before,
            messages_after: messages_before,
            collapsed_pairs: 0,
            dropped_messages: 0,
        };
    }

    let mut collapsed_pairs: usize = 0;

    if config.collapse_tool_results {
        let mut i = 0;
        while i + 1 < messages.len() {
            let protected = protected_indices(messages, config.keep_recent);
            if messages[i].role == "assistant"
                && messages[i + 1].role == "tool"
                && !protected[i]
                && !protected[i + 1]
            {
                let tool_content = &messages[i + 1].content;
                let truncated: String = tool_content.chars().take(100).collect();
                let summary = format!("[Tool result: {truncated}...]");
                messages[i] = ChatMessage {
                    role: "assistant".to_string(),
                    content: summary,
                    metadata: Default::default(),
                };
                messages.remove(i + 1);
                collapsed_pairs += 1;
            } else {
                i += 1;
            }
        }
    }

    let mut dropped_messages: usize = 0;
    while estimate_tokens(messages) > config.max_tokens {
        let protected = protected_indices(messages, config.keep_recent);
        if let Some(idx) = protected
            .iter()
            .enumerate()
            .find(|(_, p)| !**p)
            .map(|(i, _)| i)
        {
            messages.remove(idx);
            dropped_messages += 1;
        } else {
            break;
        }
    }

    PruneStats {
        messages_before,
        messages_after: messages.len(),
        collapsed_pairs,
        dropped_messages,
    }
}
