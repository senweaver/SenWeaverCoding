// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashSet;

use serde_json::Value;

use crate::providers::traits::{
    enforce_context_budget_native_with_window, ChatMessage,
};

const SYNTHETIC_CHAT_TOOL_REPLY: &str =
    "[Synthetic tool reply] No stored result exists for this tool_call_id in the transcript \
     (possible session interruption, context trim, or compaction). Ignore and continue.";

const OPENAI_TOOL_CALL_PREFIX: &str = "call_";
const ANTHROPIC_TOOL_USE_PREFIX: &str = "toolu_";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    OpenAi,
    Anthropic,
    Other,
}

#[inline]
pub fn skip_serializing_tool_calls<T>(v: &Option<Vec<T>>) -> bool {
    v.as_ref().map_or(true, Vec::is_empty)
}

fn close_open_json_structures(prefix: &str) -> Option<String> {
    let trimmed = prefix.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut stack: Vec<char> = Vec::new();
    let mut in_string = false;
    let mut escape = false;
    for c in trimmed.chars() {
        if in_string {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => stack.push('}'),
            '[' => stack.push(']'),
            '}' | ']' => {
                stack.pop();
            }
            _ => {}
        }
    }

    let mut repaired = trimmed.to_string();
    if escape {
        repaired.pop();
    }
    if in_string {
        repaired.push('"');
    }
    loop {
        let end = repaired.trim_end();
        match end.chars().last() {
            Some(',') | Some(':') => {
                let new_len = end.len() - 1;
                repaired.truncate(new_len);
            }
            _ => break,
        }
    }
    while let Some(close) = stack.pop() {
        repaired.push(close);
    }

    match serde_json::from_str::<Value>(&repaired) {
        Ok(_) => Some(repaired),
        Err(_) => None,
    }
}

pub fn repair_partial_tool_input_json(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if !trimmed.starts_with('{') && !trimmed.starts_with('[') {
        return None;
    }

    if let Some(repaired) = close_open_json_structures(trimmed) {
        return Some(repaired);
    }

    let mut comma_positions: Vec<usize> = Vec::new();
    let mut in_string = false;
    let mut escape = false;
    for (idx, c) in trimmed.char_indices() {
        if in_string {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            ',' => comma_positions.push(idx),
            _ => {}
        }
    }

    for &pos in comma_positions.iter().rev().take(32) {
        if let Some(repaired) = close_open_json_structures(&trimmed[..pos]) {
            return Some(repaired);
        }
    }

    None
}

pub fn normalize_tool_call_arguments(function_name: &str, arguments: String) -> String {
    if arguments.trim().is_empty() {
        return "{}".to_string();
    }
    if serde_json::from_str::<Value>(&arguments).is_ok() {
        return arguments;
    }
    if let Some(repaired) = repair_partial_tool_input_json(&arguments) {
        tracing::warn!(
            function = %function_name,
            arguments_len = arguments.len(),
            repaired_len = repaired.len(),
            "tool-call arguments were truncated; recovered partial arguments via structural repair"
        );
        return repaired;
    }
    tracing::error!(
        function = %function_name,
        arguments = %arguments,
        "Invalid JSON in tool-call arguments, using empty object"
    );
    crate::observability::runtime_trace::record_event(
        "tool_args_degraded",
        None,
        None,
        None,
        None,
        Some(false),
        Some("tool-call arguments unparseable; degraded to empty object"),
        serde_json::json!({
            "function": function_name,
            "arguments_len": arguments.len(),
        }),
    );
    "{}".to_string()
}

#[inline]
pub fn normalize_tool_call_id(raw: Option<String>) -> String {
    normalize_tool_call_id_for_provider(raw, ProviderKind::Other)
}

pub fn normalize_tool_call_id_for_provider(
    raw: Option<String>,
    kind: ProviderKind,
) -> String {
    if let Some(value) = raw {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    match kind {
        ProviderKind::OpenAi => format!("{OPENAI_TOOL_CALL_PREFIX}{}", uuid_short()),
        ProviderKind::Anthropic => format!("{ANTHROPIC_TOOL_USE_PREFIX}{}", uuid_short()),
        ProviderKind::Other => uuid::Uuid::new_v4().to_string(),
    }
}

fn uuid_short() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

pub fn ensure_tool_id_pair(value: &mut Value) {
    let Some(obj) = value.as_object_mut() else {
        return;
    };
    let call_id = obj
        .get("tool_call_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty());
    let use_id = obj
        .get("tool_use_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty());
    let resolved = match (call_id, use_id) {
        (Some(call), Some(_use_id)) => call,
        (Some(call), None) => call,
        (None, Some(use_id)) => use_id,
        (None, None) => return,
    };
    obj.insert("tool_call_id".to_string(), Value::String(resolved.clone()));
    obj.insert("tool_use_id".to_string(), Value::String(resolved));
}

pub fn ensure_tool_call_id_pair_in_assistant_envelope(value: &mut Value) {
    let Some(obj) = value.as_object_mut() else {
        return;
    };
    let Some(tool_calls) = obj.get_mut("tool_calls").and_then(Value::as_array_mut) else {
        return;
    };
    for call in tool_calls.iter_mut() {
        let Some(call_obj) = call.as_object_mut() else {
            continue;
        };
        let primary = call_obj
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                call_obj
                    .get("tool_use_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .filter(|s| !s.trim().is_empty())
            })
            .or_else(|| {
                call_obj
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .filter(|s| !s.trim().is_empty())
            });
        let Some(primary) = primary else { continue };
        call_obj.insert("id".to_string(), Value::String(primary.clone()));
        call_obj.insert("tool_call_id".to_string(), Value::String(primary.clone()));
        call_obj.insert("tool_use_id".to_string(), Value::String(primary));
    }
}

pub fn sanitize_messages_before_send_for_trait(
    provider: &dyn crate::providers::traits::Provider,
    messages: Vec<ChatMessage>,
    model: &str,
    reserve_output_tokens: usize,
    context_window: Option<usize>,
) -> Vec<ChatMessage> {
    sanitize_messages_before_send_for_provider_ext(
        messages,
        model,
        reserve_output_tokens,
        context_window,
        provider.message_format_kind(),
        provider.consumes_reasoning_envelope(),
    )
}

pub fn sanitize_messages_before_send_for_provider(
    messages: Vec<ChatMessage>,
    model: &str,
    reserve_output_tokens: usize,
    context_window: Option<usize>,
    kind: ProviderKind,
) -> Vec<ChatMessage> {
    sanitize_messages_before_send_for_provider_ext(
        messages,
        model,
        reserve_output_tokens,
        context_window,
        kind,
        false,
    )
}

pub fn sanitize_messages_before_send_for_provider_ext(
    messages: Vec<ChatMessage>,
    model: &str,
    reserve_output_tokens: usize,
    context_window: Option<usize>,
    kind: ProviderKind,
    consumes_reasoning_envelope: bool,
) -> Vec<ChatMessage> {
    let trimmed = enforce_context_budget_native_with_window(
        messages,
        model,
        reserve_output_tokens,
        context_window,
    );
    normalize_chat_messages_for_provider_ext(trimmed, kind, consumes_reasoning_envelope)
}

pub fn normalize_chat_messages_for_wire(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    normalize_chat_messages_for_provider(messages, ProviderKind::OpenAi)
}

pub fn normalize_chat_messages_for_provider(
    messages: Vec<ChatMessage>,
    kind: ProviderKind,
) -> Vec<ChatMessage> {
    normalize_chat_messages_for_provider_ext(messages, kind, true)
}

pub fn normalize_chat_messages_for_provider_ext(
    messages: Vec<ChatMessage>,
    kind: ProviderKind,
    consumes_reasoning_envelope: bool,
) -> Vec<ChatMessage> {
    let mirrored = mirror_tool_ids_in_chat_messages(messages);
    let cleaned = clean_empty_assistant_tool_calls_in_chat_messages(mirrored);
    let non_empty = drop_payloadless_assistant_messages(cleaned);
    let reasoning_normalized = match kind {
        ProviderKind::Anthropic => non_empty,
        _ => {
            if consumes_reasoning_envelope {
                promote_reasoning_only_assistants_for_openai(non_empty)
            } else {
                flatten_reasoning_envelopes_for_wire(non_empty)
            }
        }
    };
    let no_orphans = match kind {
        ProviderKind::Anthropic => coerce_orphan_tool_messages_for_anthropic(reasoning_normalized),
        _ => coerce_orphan_tool_messages_in_chat_messages(reasoning_normalized),
    };
    repair_dangling_tool_pairs_for_provider(no_orphans, kind)
}

pub fn flatten_reasoning_envelopes_for_wire(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    let mut out: Vec<ChatMessage> = Vec::with_capacity(messages.len());
    let mut flattened: usize = 0;
    let mut dropped: usize = 0;

    for mut msg in messages {
        if msg.role != "assistant" {
            out.push(msg);
            continue;
        }
        let trimmed = msg.content.trim_start();
        if !trimmed.starts_with('{') {
            out.push(msg);
            continue;
        }
        let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(trimmed) else {
            out.push(msg);
            continue;
        };
        let is_envelope = !obj.is_empty()
            && obj.keys().all(|key| {
                matches!(key.as_str(), "content" | "tool_calls" | "reasoning_content")
            });
        if !is_envelope {
            out.push(msg);
            continue;
        }
        // Envelopes carrying tool_calls are already destructured by every wire
        // (tool_calls parsed, plain content extracted, reasoning dropped), so they
        // never leak. Only the reasoning-only envelope needs flattening here.
        let has_tool_calls = obj
            .get("tool_calls")
            .and_then(Value::as_array)
            .map(|arr| !arr.is_empty())
            .unwrap_or(false);
        if has_tool_calls {
            out.push(msg);
            continue;
        }
        let content = obj
            .get("content")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let reasoning = obj
            .get("reasoning_content")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        match content.or(reasoning) {
            Some(text) => {
                msg.content = text.to_string();
                flattened += 1;
                out.push(msg);
            }
            None => {
                dropped += 1;
            }
        }
    }

    if flattened > 0 || dropped > 0 {
        tracing::warn!(
            target: "providers.sanitize",
            flattened,
            dropped,
            "flattened reasoning envelopes to plain content for a wire that does not consume them (prevents raw JSON leak)"
        );
    }
    out
}

pub fn promote_reasoning_only_assistants_for_openai(
    messages: Vec<ChatMessage>,
) -> Vec<ChatMessage> {
    let mut out: Vec<ChatMessage> = Vec::with_capacity(messages.len());
    let mut promoted: usize = 0;
    let mut dropped: usize = 0;

    for mut msg in messages {
        if msg.role != "assistant" {
            out.push(msg);
            continue;
        }

        let trimmed = msg.content.trim_start();
        if !trimmed.starts_with('{') {
            out.push(msg);
            continue;
        }

        let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(trimmed) else {
            out.push(msg);
            continue;
        };

        let is_envelope = !obj.is_empty()
            && obj.keys().all(|key| {
                matches!(key.as_str(), "content" | "tool_calls" | "reasoning_content")
            });
        if !is_envelope {
            out.push(msg);
            continue;
        }

        let has_tool_calls = obj
            .get("tool_calls")
            .and_then(Value::as_array)
            .map(|arr| !arr.is_empty())
            .unwrap_or(false);
        let content_empty = obj
            .get("content")
            .and_then(Value::as_str)
            .map(|s| s.trim().is_empty())
            .unwrap_or(true);

        if has_tool_calls || !content_empty {
            out.push(msg);
            continue;
        }

        let reasoning = obj
            .get("reasoning_content")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());

        match reasoning {
            Some(text) => {
                msg.content = text.to_string();
                promoted += 1;
                out.push(msg);
            }
            None => {
                dropped += 1;
            }
        }
    }

    if promoted > 0 || dropped > 0 {
        tracing::warn!(
            target: "providers.sanitize",
            promoted,
            dropped,
            "normalized reasoning-only assistant messages for OpenAI-style wire (no content/tool_calls)"
        );
    }
    out
}

pub fn drop_payloadless_assistant_messages(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    let before = messages.len();
    let out: Vec<ChatMessage> = messages
        .into_iter()
        .filter(|msg| !(msg.role == "assistant" && assistant_has_no_payload(&msg.content)))
        .collect();
    let dropped = before - out.len();
    if dropped > 0 {
        tracing::warn!(
            target: "providers.sanitize",
            dropped,
            "dropped payload-less assistant messages (no content, no tool_calls) to satisfy provider message validation"
        );
    }
    out
}

pub fn assistant_has_no_payload(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed.starts_with('{') {
        if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(trimmed) {
            let is_envelope = obj.contains_key("tool_calls")
                || obj.contains_key("content")
                || obj.contains_key("reasoning_content");
            if is_envelope {
                let has_tool_calls = obj
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .map(|arr| !arr.is_empty())
                    .unwrap_or(false);
                let body_empty = obj
                    .get("content")
                    .and_then(Value::as_str)
                    .map(|s| s.trim().is_empty())
                    .unwrap_or(true);
                let reasoning_empty = obj
                    .get("reasoning_content")
                    .and_then(Value::as_str)
                    .map(|s| s.trim().is_empty())
                    .unwrap_or(true);
                return !has_tool_calls && body_empty && reasoning_empty;
            }
        }
    } else if trimmed.starts_with('[') {
        if let Ok(arr) = serde_json::from_str::<Vec<Value>>(trimmed) {
            let is_block_envelope = !arr.is_empty()
                && arr.iter().all(|item| {
                    item.is_object() && item.get("type").and_then(Value::as_str).is_some()
                });
            if !is_block_envelope {
                return false;
            }
            let has_payload = arr.iter().any(|item| {
                match item.get("type").and_then(Value::as_str) {
                    Some("tool_use") => true,
                    Some("text") | Some("reasoning") | Some("thinking") => item
                        .get("text")
                        .and_then(Value::as_str)
                        .map(|s| !s.trim().is_empty())
                        .unwrap_or(false),
                    _ => false,
                }
            });
            return !has_payload;
        }
    }
    false
}

pub fn mirror_tool_ids_in_chat_messages(mut messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    for msg in messages.iter_mut() {
        let trimmed = msg.content.trim_start();
        if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
            continue;
        }
        let Ok(mut value) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        let mutated = mirror_tool_ids_in_json(&mut value);
        if mutated {
            msg.content = value.to_string();
        }
    }
    messages
}

fn mirror_tool_ids_in_json(value: &mut Value) -> bool {
    let mut changed = false;
    if value.is_object() {
        let has_id_keys = match value.as_object() {
            Some(map) => {
                map.contains_key("tool_call_id") || map.contains_key("tool_use_id")
            }
            None => false,
        };
        if has_id_keys {
            let before = value.clone();
            ensure_tool_id_pair(value);
            if before != *value {
                changed = true;
            }
        }

        let has_envelope_calls = value
            .get("tool_calls")
            .map(Value::is_array)
            .unwrap_or(false);
        if has_envelope_calls {
            let before = value.clone();
            ensure_tool_call_id_pair_in_assistant_envelope(value);
            if before != *value {
                changed = true;
            }
        }

        if let Some(map) = value.as_object_mut() {
            for (_, child) in map.iter_mut() {
                if mirror_tool_ids_in_json(child) {
                    changed = true;
                }
            }
        }
        return changed;
    }
    if let Some(items) = value.as_array_mut() {
        for item in items.iter_mut() {
            if mirror_tool_ids_in_json(item) {
                changed = true;
            }
        }
    }
    changed
}

pub fn clean_empty_assistant_tool_calls_in_chat_messages(
    mut messages: Vec<ChatMessage>,
) -> Vec<ChatMessage> {
    let mut rewritten: usize = 0;
    for msg in messages.iter_mut() {
        if msg.role != "assistant" {
            continue;
        }

        let metadata_had_empty = msg
            .metadata
            .get("tool_calls")
            .map(|v| match v {
                Value::Array(arr) => arr.is_empty(),
                Value::Null => true,
                _ => false,
            })
            .unwrap_or(false);
        if metadata_had_empty {
            msg.metadata.remove("tool_calls");
            rewritten += 1;
        }

        let trimmed = msg.content.trim_start();
        if !trimmed.starts_with('{') {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        let Some(obj) = value.as_object() else {
            continue;
        };
        let tool_calls_empty = obj
            .get("tool_calls")
            .map(|v| match v {
                Value::Array(arr) => arr.is_empty(),
                Value::Null => true,
                _ => false,
            })
            .unwrap_or(false);
        if !tool_calls_empty {
            continue;
        }
        let body_text = obj
            .get("content")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_default();
        let reasoning = obj
            .get("reasoning_content")
            .and_then(Value::as_str)
            .map(str::to_string);
        if reasoning.as_deref().is_some_and(|s| !s.is_empty()) {
            let mut map = serde_json::Map::new();
            if !body_text.is_empty() {
                map.insert("content".to_string(), Value::String(body_text.clone()));
            }
            if let Some(r) = reasoning {
                map.insert("reasoning_content".to_string(), Value::String(r));
            }
            msg.content = Value::Object(map).to_string();
        } else {
            msg.content = body_text;
        }
        rewritten += 1;
    }
    if rewritten > 0 {
        tracing::debug!(
            target: "providers.sanitize",
            rewritten,
            "stripped empty tool_calls arrays from assistant envelope content"
        );
    }
    messages
}

pub fn coerce_orphan_tool_messages_in_chat_messages(
    messages: Vec<ChatMessage>,
) -> Vec<ChatMessage> {
    let mut out: Vec<ChatMessage> = Vec::with_capacity(messages.len());
    let mut coerced: usize = 0;

    for msg in messages {
        if msg.role != "tool" {
            out.push(msg);
            continue;
        }

        let preceded_ok = match out.last() {
            Some(last) if last.role == "tool" => true,
            Some(last) if last.role == "assistant" => {
                extract_assistant_tool_call_ids(&last.content)
                    .map(|ids| !ids.is_empty())
                    .unwrap_or(false)
            }
            _ => false,
        };

        if preceded_ok {
            out.push(msg);
        } else {
            let id = extract_tool_call_id_from_tool_message(&msg.content);
            coerced += 1;
            out.push(recover_orphan_tool_as_user(&msg, id.as_deref()));
        }
    }

    if coerced > 0 {
        tracing::warn!(
            target: "providers.sanitize",
            coerced,
            "coerced orphan tool messages into recovered user transcript (no preceding assistant.tool_calls)"
        );
    }
    out
}

pub fn coerce_orphan_tool_messages_for_anthropic(
    messages: Vec<ChatMessage>,
) -> Vec<ChatMessage> {
    let mut out: Vec<ChatMessage> = Vec::with_capacity(messages.len());
    let mut absorbed: usize = 0;
    let mut dropped: usize = 0;

    for msg in messages {
        if msg.role != "tool" {
            out.push(msg);
            continue;
        }

        let preceded_ok = match out.last() {
            Some(last) if last.role == "tool" => true,
            Some(last) if last.role == "assistant" => {
                extract_assistant_tool_call_ids(&last.content)
                    .map(|ids| !ids.is_empty())
                    .unwrap_or(false)
            }
            _ => false,
        };

        if preceded_ok {
            out.push(msg);
            continue;
        }

        let id = extract_tool_call_id_from_tool_message(&msg.content);
        let body = extract_tool_body_from_envelope(&msg.content);
        let id_text = id.as_deref().unwrap_or("unknown");
        let absorbed_text = format!(
            "[Recovered tool output; tool_use preamble missing for tool_call_id={id_text}]\n{body}"
        );

        if let Some(prev) = out.last_mut() {
            if prev.role == "user" {
                if !prev.content.trim_end().is_empty() {
                    prev.content.push_str("\n\n");
                }
                prev.content.push_str(&absorbed_text);
                absorbed += 1;
                continue;
            }
        }

        dropped += 1;
        out.push(ChatMessage::user(absorbed_text));
    }

    if absorbed > 0 || dropped > 0 {
        tracing::warn!(
            target: "providers.sanitize",
            absorbed,
            promoted_to_user = dropped,
            "absorbed orphan tool messages for anthropic pairing safety"
        );
    }
    out
}

fn extract_tool_body_from_envelope(content: &str) -> String {
    let trimmed = content.trim_start();
    if trimmed.starts_with('{') {
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            if let Some(body) = value.get("content").and_then(Value::as_str) {
                return body.to_string();
            }
        }
    } else if trimmed.starts_with('[') {
        if let Ok(values) = serde_json::from_str::<Vec<Value>>(trimmed) {
            for v in values {
                if v.get("type").and_then(Value::as_str) == Some("tool_result") {
                    if let Some(body) = v.get("content").and_then(Value::as_str) {
                        return body.to_string();
                    }
                }
            }
        }
    }
    content.to_string()
}

fn recover_orphan_tool_as_user(msg: &ChatMessage, tool_call_id: Option<&str>) -> ChatMessage {
    let inner_body = match serde_json::from_str::<Value>(msg.content.trim_start()) {
        Ok(value) => value
            .get("content")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| msg.content.clone()),
        Err(_) => msg.content.clone(),
    };
    let id_text = tool_call_id.unwrap_or("unknown");
    let recovered = format!(
        "[Recovered tool output; assistant.tool_calls preamble was missing in transcript]\n\
         tool_call_id={id_text}\n\
         {inner_body}",
    );
    ChatMessage::user(recovered)
}

pub fn repair_dangling_tool_pairs_in_chat_messages(
    messages: Vec<ChatMessage>,
) -> Vec<ChatMessage> {
    repair_dangling_tool_pairs_for_provider(messages, ProviderKind::OpenAi)
}

pub fn repair_dangling_tool_pairs_for_provider(
    mut messages: Vec<ChatMessage>,
    kind: ProviderKind,
) -> Vec<ChatMessage> {
    let mut patched: usize = 0;
    let mut i: usize = 0;
    while i < messages.len() {
        if messages[i].role != "assistant" {
            i += 1;
            continue;
        }

        let id_name_map = extract_assistant_tool_call_id_name_map(&messages[i].content);
        let required: Vec<String> = if id_name_map.is_empty() {
            match extract_assistant_tool_call_ids(&messages[i].content) {
                Some(ids) if !ids.is_empty() => ids,
                _ => {
                    i += 1;
                    continue;
                }
            }
        } else {
            id_name_map.iter().map(|(id, _)| id.clone()).collect()
        };

        let mut j = i + 1;
        let mut seen: HashSet<String> = HashSet::new();
        while j < messages.len() && messages[j].role == "tool" {
            if let Some(id) = extract_tool_call_id_from_tool_message(&messages[j].content) {
                seen.insert(id);
            }
            j += 1;
        }

        let missing: Vec<String> = required
            .into_iter()
            .filter(|id| !seen.contains(id))
            .collect();
        if missing.is_empty() {
            i = j;
            continue;
        }

        tracing::warn!(
            target: "providers.sanitize",
            missing = ?missing,
            ?kind,
            "injecting synthetic chat-message tool replies after incomplete assistant.tool_calls"
        );

        let mut insert_at = j;
        for id in missing {
            let tool_name = id_name_map
                .iter()
                .find(|(cid, _)| cid == &id)
                .map(|(_, name)| name.clone());
            let payload = build_synthetic_tool_reply_envelope(&id, tool_name.as_deref(), kind);
            let mut msg = ChatMessage::tool(payload);
            msg.metadata.insert(
                "synthetic".to_string(),
                Value::Bool(true),
            );
            msg.metadata.insert(
                "synthetic_reason".to_string(),
                Value::String("dangling_tool_pair".to_string()),
            );
            if let Some(name) = tool_name {
                msg.metadata
                    .insert("tool_name".to_string(), Value::String(name));
            }
            messages.insert(insert_at, msg);
            insert_at += 1;
            patched += 1;
        }
        i = insert_at;
    }

    if patched > 0 {
        tracing::info!(
            target: "providers.sanitize",
            stubs = patched,
            ?kind,
            "applied synthetic chat-message tool reply rows"
        );
    }

    messages
}

fn build_synthetic_tool_reply_envelope(
    id: &str,
    tool_name: Option<&str>,
    _kind: ProviderKind,
) -> String {
    let content = match tool_name {
        Some(name) => format!(
            "[Synthetic tool reply for `{name}`] No stored result exists for this tool_call_id in the transcript \
             (possible session interruption, context trim, or compaction). Ignore and continue."
        ),
        None => SYNTHETIC_CHAT_TOOL_REPLY.to_string(),
    };
    serde_json::json!({
        "tool_call_id": id,
        "tool_use_id": id,
        "content": content,
        "synthetic": true,
    })
    .to_string()
}

fn extract_assistant_tool_call_id_name_map(content: &str) -> Vec<(String, String)> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with('{') {
        return Vec::new();
    }
    let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
        return Vec::new();
    };
    let Some(calls) = value.get("tool_calls").and_then(Value::as_array) else {
        return Vec::new();
    };
    calls
        .iter()
        .filter_map(|c| {
            let id = c
                .get("id")
                .or_else(|| c.get("tool_use_id"))
                .or_else(|| c.get("tool_call_id"))
                .and_then(Value::as_str)
                .map(str::to_string)?;
            let name = c
                .get("function")
                .and_then(|f| f.get("name"))
                .or_else(|| c.get("name"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_default();
            Some((id, name))
        })
        .filter(|(id, _)| !id.is_empty())
        .collect()
}

pub fn count_dangling_tool_pairs_in_chat_messages(messages: &[ChatMessage]) -> (usize, usize) {
    let mut frames: usize = 0;
    let mut stubs: usize = 0;
    let mut i: usize = 0;
    while i < messages.len() {
        if messages[i].role != "assistant" {
            i += 1;
            continue;
        }
        let required: Vec<String> = match extract_assistant_tool_call_ids(&messages[i].content) {
            Some(ids) if !ids.is_empty() => ids,
            _ => {
                i += 1;
                continue;
            }
        };
        let mut j = i + 1;
        let mut seen: HashSet<String> = HashSet::new();
        while j < messages.len() && messages[j].role == "tool" {
            if let Some(id) = extract_tool_call_id_from_tool_message(&messages[j].content) {
                seen.insert(id);
            }
            j += 1;
        }
        let missing = required.iter().filter(|id| !seen.contains(*id)).count();
        if missing > 0 {
            frames += 1;
            stubs += missing;
        }
        i = j;
    }
    (frames, stubs)
}

fn extract_assistant_tool_call_ids(content: &str) -> Option<Vec<String>> {
    let trimmed = content.trim_start();
    if trimmed.starts_with('{') {
        let value: Value = serde_json::from_str(trimmed).ok()?;
        let arr = value.get("tool_calls").and_then(Value::as_array)?;
        let ids: Vec<String> = arr
            .iter()
            .filter_map(extract_tool_call_id_from_call_entry)
            .filter(|s| !s.trim().is_empty())
            .collect();
        return Some(ids);
    }
    if trimmed.starts_with('[') {
        let arr: Vec<Value> = serde_json::from_str(trimmed).ok()?;
        let mut ids: Vec<String> = Vec::new();
        for v in arr {
            if v.get("type").and_then(Value::as_str) != Some("tool_use") {
                continue;
            }
            let id = v
                .get("id")
                .and_then(Value::as_str)
                .or_else(|| v.get("tool_use_id").and_then(Value::as_str))
                .or_else(|| v.get("tool_call_id").and_then(Value::as_str));
            if let Some(id) = id {
                if !id.trim().is_empty() {
                    ids.push(id.to_string());
                }
            }
        }
        return Some(ids);
    }
    None
}

fn extract_tool_call_id_from_call_entry(call: &Value) -> Option<String> {
    let direct = call
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| call.get("tool_use_id").and_then(Value::as_str))
        .or_else(|| call.get("tool_call_id").and_then(Value::as_str));
    direct.map(str::to_string)
}

fn extract_tool_call_id_from_tool_message(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    if trimmed.starts_with('{') {
        let value: Value = serde_json::from_str(trimmed).ok()?;
        let id = value
            .get("tool_call_id")
            .and_then(Value::as_str)
            .or_else(|| value.get("tool_use_id").and_then(Value::as_str))?;
        if id.trim().is_empty() {
            return None;
        }
        return Some(id.to_string());
    }
    if trimmed.starts_with('[') {
        let arr: Vec<Value> = serde_json::from_str(trimmed).ok()?;
        for v in arr {
            if v.get("type").and_then(Value::as_str) != Some("tool_result") {
                continue;
            }
            let id = v
                .get("tool_use_id")
                .and_then(Value::as_str)
                .or_else(|| v.get("tool_call_id").and_then(Value::as_str));
            if let Some(id) = id {
                if !id.trim().is_empty() {
                    return Some(id.to_string());
                }
            }
        }
    }
    None
}
