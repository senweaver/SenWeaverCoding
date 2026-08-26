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
    0
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

const PRUNE_NOTICE_PREFIX: &str = "[context notice]";

fn prune_notice_message(removed: usize, blob_id: Option<&str>) -> ChatMessage {
    let body = match blob_id {
        Some(id) => format!(
            "{PRUNE_NOTICE_PREFIX} {removed} earlier message(s) were archived because the \
             conversation exceeded the context budget; retrieve with tool_result_expand id {id}."
        ),
        None => format!(
            "{PRUNE_NOTICE_PREFIX} {removed} earlier message(s) were removed because the \
             conversation exceeded the context budget; their content is no longer available."
        ),
    };
    ChatMessage::user(&body)
}

fn calibrated_message_tokens(message: &ChatMessage, factor: f64) -> usize {
    let raw = crate::providers::traits::estimate_message_tokens(message);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        ((raw as f64 * 1.05) * factor).round() as usize
    }
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

const TRUNCATION_MARKER_PREFIX: &str = "[tool result truncated:";

fn truncation_marker(kept: usize, total: usize, blob_id: Option<&str>) -> String {
    match blob_id {
        Some(id) => format!(
            "{TRUNCATION_MARKER_PREFIX} kept {kept} of {total} chars; full output archived \
             as blob {id}; call tool_result_expand with this id to retrieve the rest]"
        ),
        None => format!(
            "{TRUNCATION_MARKER_PREFIX} kept {kept} of {total} chars; the remainder is no \
             longer available. Do NOT re-run the original call with the same arguments; \
             page into uncovered ranges or change arguments if you need more]"
        ),
    }
}

pub(crate) fn shrink_text_layered(original: &str, keep_chars: usize) -> Option<String> {
    if let Some(marker_pos) = original.rfind(TRUNCATION_MARKER_PREFIX) {
        let body = original[..marker_pos].trim_end();
        let marker = &original[marker_pos..];
        let body_chars = body.chars().count();
        if body_chars <= keep_chars {
            return None;
        }
        let excerpt: String = body.chars().take(keep_chars).collect();
        return Some(format!("{excerpt}\n{marker}"));
    }
    let total_chars = original.chars().count();
    if total_chars <= keep_chars {
        return None;
    }
    let blob_id = super::blob_store::put(original);
    let excerpt: String = original.chars().take(keep_chars).collect();
    let marker = truncation_marker(keep_chars, total_chars, blob_id.as_deref());
    Some(format!("{excerpt}\n{marker}"))
}

fn truncate_native_tool_result(message: &mut ChatMessage, keep_chars: usize) -> bool {
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
    let Some(shrunk) = shrink_text_layered(&original, keep_chars) else {
        return false;
    };
    obj.insert(
        "content".to_string(),
        serde_json::Value::String(shrunk),
    );
    message.content = envelope.to_string();
    true
}

pub(crate) fn compact_tool_message(message: &mut ChatMessage, keep_chars: usize) -> bool {
    if message.role != "tool" {
        return false;
    }
    if truncate_native_tool_result(message, keep_chars) {
        return true;
    }
    match shrink_text_layered(&message.content, keep_chars) {
        Some(shrunk) => {
            message.content = shrunk;
            true
        }
        None => false,
    }
}

fn group_is_protected(messages: &[ChatMessage], start: usize, end: usize, keep_recent: usize) -> bool {
    let recent_start = messages.len().saturating_sub(keep_recent);
    if end > recent_start {
        return true;
    }
    (start..end).any(|idx| messages[idx].role == "system")
}

fn collapse_pass(
    messages: &mut Vec<ChatMessage>,
    config: &HistoryPrunerConfig,
    factor: f64,
    keep_chars: usize,
    total_tokens: &mut usize,
) -> usize {
    let mut collapsed_pairs = 0usize;
    let mut i = 0;
    while i < messages.len() && *total_tokens > config.max_tokens {
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
                let before = calibrated_message_tokens(message, factor);
                if truncate_native_tool_result(message, keep_chars) {
                    collapsed_any = true;
                    let after = calibrated_message_tokens(message, factor);
                    *total_tokens =
                        total_tokens.saturating_sub(before.saturating_sub(after));
                }
            }
            if collapsed_any {
                collapsed_pairs += 1;
            }
            i = run_end;
            continue;
        }

        let group_tokens: usize = (i..run_end)
            .map(|idx| calibrated_message_tokens(&messages[idx], factor))
            .sum();
        let first_tool = messages[i + 1].content.clone();
        let excerpt = shrink_text_layered(&first_tool, keep_chars)
            .unwrap_or_else(|| first_tool.clone());
        let original_text = messages[i].content.trim().to_string();
        let summary = if original_text.is_empty() {
            format!("[Tool result]\n{excerpt}")
        } else {
            format!("{original_text}\n[Tool result]\n{excerpt}")
        };
        messages[i].content = summary;
        messages.drain(i + 1..run_end);
        let kept = calibrated_message_tokens(&messages[i], factor);
        *total_tokens = total_tokens.saturating_sub(group_tokens.saturating_sub(kept));
        collapsed_pairs += 1;
        i += 1;
    }
    collapsed_pairs
}

pub fn prune_history(
    messages: &mut Vec<ChatMessage>,
    config: &HistoryPrunerConfig,
    model: &str,
) -> PruneStats {
    let messages_before = messages.len();
    if !config.enabled || config.max_tokens == 0 || messages.is_empty() {
        return PruneStats {
            messages_before,
            messages_after: messages_before,
            collapsed_pairs: 0,
            dropped_messages: 0,
        };
    }

    let factor = crate::agent::token::budget::calibration_factor_for(model);
    let mut total_tokens =
        crate::agent::token::budget::estimate_history_tokens_calibrated(messages, model);
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
        let generous_keep_chars =
            crate::agent::token::budget::default_max_tool_result_tokens().saturating_mul(4);
        const AGGRESSIVE_KEEP_CHARS: usize = 400;
        const PLACEHOLDER_KEEP_CHARS: usize = 80;
        for keep_chars in [generous_keep_chars, AGGRESSIVE_KEEP_CHARS, PLACEHOLDER_KEEP_CHARS] {
            if total_tokens <= config.max_tokens {
                break;
            }
            collapsed_pairs += collapse_pass(
                messages,
                config,
                factor,
                keep_chars,
                &mut total_tokens,
            );
        }
    }

    let mut dropped_messages: usize = 0;
    if total_tokens > config.max_tokens {
        let mut drop_flags = vec![false; messages.len()];
        let mut i = 0;
        while i < messages.len() && total_tokens > config.max_tokens {
            if messages[i].role == "system" || messages[i].role == "user" {
                i += 1;
                continue;
            }
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
                .map(|idx| calibrated_message_tokens(&messages[idx], factor))
                .sum();
            for flag in drop_flags.iter_mut().take(end).skip(i) {
                *flag = true;
            }
            dropped_messages += end - i;
            total_tokens = total_tokens.saturating_sub(group_tokens);
            i = end;
        }
        if dropped_messages > 0 {
            let mut rebuilt: Vec<ChatMessage> = Vec::with_capacity(messages.len());
            let mut run = 0usize;
            let mut archived = String::new();
            for (message, dropped) in messages.drain(..).zip(drop_flags.into_iter()) {
                if dropped {
                    run += 1;
                    if archived.len() < 6 * 1024 * 1024 {
                        archived.push('[');
                        archived.push_str(&message.role);
                        archived.push_str("]\n");
                        archived.push_str(&message.content);
                        archived.push_str("\n\n");
                    }
                    continue;
                }
                if run > 0 {
                    let blob_id = super::blob_store::put(&archived);
                    rebuilt.push(prune_notice_message(run, blob_id.as_deref()));
                    run = 0;
                    archived.clear();
                }
                rebuilt.push(message);
            }
            if run > 0 {
                let blob_id = super::blob_store::put(&archived);
                rebuilt.push(prune_notice_message(run, blob_id.as_deref()));
            }
            *messages = rebuilt;
            tracing::warn!(
                target: "agent.history.pruner",
                dropped = dropped_messages,
                "history pruner archived earlier assistant/tool messages over the context budget; placeholder notices inserted"
            );
        }
    }

    if collapsed_pairs > 0 || dropped_messages > 0 {
        crate::agent::context::compressor::repair_tool_pairs(messages);
    }

    PruneStats {
        messages_before,
        messages_after: messages.len(),
        collapsed_pairs,
        dropped_messages,
    }
}
