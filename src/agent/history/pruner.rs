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
    messages.iter().map(|m| m.content.len() / 4).sum()
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
        while i < messages.len() {
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

            let protected = protected_indices(messages, config.keep_recent);
            let group_protected = (i..run_end).any(|idx| protected[idx]);
            if group_protected {
                i = run_end;
                continue;
            }

            if is_native_tool_pair(&messages[i], &messages[i + 1]) {
                let mut collapsed_any = false;
                for idx in (i + 1)..run_end {
                    if truncate_native_tool_result(&mut messages[idx]) {
                        collapsed_any = true;
                    }
                }
                if collapsed_any {
                    collapsed_pairs += 1;
                }
                i = run_end;
                continue;
            }

            let first_tool = &messages[i + 1].content;
            let truncated: String = first_tool.chars().take(100).collect();
            let summary = format!("[Tool result: {truncated}...]");
            messages[i] = ChatMessage {
                role: "assistant".to_string(),
                content: summary,
                metadata: Default::default(),
            };
            messages.drain(i + 1..run_end);
            collapsed_pairs += 1;
            i += 1;
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
