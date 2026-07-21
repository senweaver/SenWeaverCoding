// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::providers::traits::ChatMessage;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

fn default_enabled() -> bool {
    true
}

fn default_max_tokens() -> usize {
    64_000
}

fn default_keep_recent() -> usize {
    8
}

fn default_collapse() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HistoryPrunerConfig {

    #[serde(default = "default_enabled")]
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
            enabled: default_enabled(),
            max_tokens: default_max_tokens(),
            keep_recent: default_keep_recent(),
            collapse_tool_results: default_collapse(),
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
    // Use the shared estimator (ASCII/4 + one token per wide char) so pruning
    // thresholds agree with the compaction estimator instead of a byte/4 guess
    // that badly overcounts CJK.
    messages
        .iter()
        .map(crate::providers::traits::estimate_message_tokens)
        .sum()
}

fn is_native_tool_pair(assistant: &ChatMessage, tool: &ChatMessage) -> bool {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&tool.content) {
        if v.get("tool_call_id").is_some() || v.get("tool_use_id").is_some() {
            return true;
        }
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&assistant.content) {
        if v.get("tool_calls").is_some() {
            return true;
        }
    }
    false
}

fn truncate_native_tool_result(message: &mut ChatMessage) -> bool {
    let Ok(mut envelope) = serde_json::from_str::<serde_json::Value>(&message.content) else {
        return false;
    };
    let Some(obj) = envelope.as_object_mut() else {
        return false;
    };
    let Some(content_val) = obj.get("content") else {
        return false;
    };
    let original = match content_val {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let truncated: String = original.chars().take(100).collect();
    if truncated.len() >= original.len() {
        return false;
    }
    obj.insert(
        "content".to_string(),
        serde_json::Value::String(format!("{truncated}...")),
    );
    message.content = envelope.to_string();
    true
}

fn group_is_protected(messages: &[ChatMessage], start: usize, end: usize, keep_recent: usize) -> bool {
    let recent_start = messages.len().saturating_sub(keep_recent);
    if end > recent_start {
        return true;
    }
    (start..end).any(|idx| messages[idx].role == "system")
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

    let mut total_tokens = estimate_tokens(messages);
    if total_tokens <= config.max_tokens {
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
        while i < messages.len() && total_tokens > config.max_tokens {
            if messages[i].role != "assistant"
                || i + 1 >= messages.len()
                || messages[i + 1].role != "tool"
            {
                i += 1;
                continue;
            }

            let mut run_end = i + 1;
            while run_end < messages.len() && messages[run_end].role == "tool" {
                run_end += 1;
            }

            if group_is_protected(messages, i, run_end, config.keep_recent) {
                i = run_end;
                continue;
            }

            if is_native_tool_pair(&messages[i], &messages[i + 1]) {
                let mut collapsed_any = false;
                for message in messages.iter_mut().take(run_end).skip(i + 1) {
                    let before = crate::providers::traits::estimate_message_tokens(message);
                    if truncate_native_tool_result(message) {
                        collapsed_any = true;
                        let after = crate::providers::traits::estimate_message_tokens(message);
                        total_tokens = total_tokens.saturating_sub(before.saturating_sub(after));
                    }
                }
                if collapsed_any {
                    collapsed_pairs += 1;
                }
                i = run_end;
                continue;
            }

            let group_tokens: usize = (i..run_end)
                .map(|idx| crate::providers::traits::estimate_message_tokens(&messages[idx]))
                .sum();
            let first_tool = &messages[i + 1].content;
            let truncated: String = first_tool.chars().take(100).collect();
            let original_text = messages[i].content.trim().to_string();
            let summary = if original_text.is_empty() {
                format!("[Tool result: {truncated}...]")
            } else {
                format!("{original_text}\n[Tool result: {truncated}...]")
            };
            messages[i] = ChatMessage {
                role: "assistant".to_string(),
                content: summary,
                metadata: Default::default(),
            };
            messages.drain(i + 1..run_end);
            let kept = crate::providers::traits::estimate_message_tokens(&messages[i]);
            total_tokens = total_tokens.saturating_sub(group_tokens.saturating_sub(kept));
            collapsed_pairs += 1;
            i += 1;
        }
    }

    let mut dropped_messages: usize = 0;
    if total_tokens > config.max_tokens {
        let mut drop_flags = vec![false; messages.len()];
        let mut i = 0;
        while i < messages.len() && total_tokens > config.max_tokens {
            let mut end = i + 1;
            if messages[i].role == "assistant" || messages[i].role == "tool" {
                while end < messages.len() && messages[end].role == "tool" {
                    end += 1;
                }
            }
            if group_is_protected(messages, i, end, config.keep_recent) {
                i = end;
                continue;
            }
            let group_tokens: usize = (i..end)
                .map(|idx| crate::providers::traits::estimate_message_tokens(&messages[idx]))
                .sum();
            for flag in drop_flags.iter_mut().take(end).skip(i) {
                *flag = true;
            }
            dropped_messages += end - i;
            total_tokens = total_tokens.saturating_sub(group_tokens);
            i = end;
        }
        if dropped_messages > 0 {
            let mut keep_iter = drop_flags.into_iter();
            messages.retain(|_| !keep_iter.next().unwrap_or(false));
        }
    }

    PruneStats {
        messages_before,
        messages_after: messages.len(),
        collapsed_pairs,
        dropped_messages,
    }
}
