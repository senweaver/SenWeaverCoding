// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
//! Restores persisted desktop gateway SQLite transcripts into shaped
//! [`crate::providers::ConversationMessage`] entries so downstream
//! [`crate::agent::dispatcher::ToolDispatcher::to_provider_messages`] matches
//! what the streamed loop produced originally.

use crate::providers::{
    ChatMessage, ConversationMessage, ToolCall, ToolResultMessage,
};
use serde_json::Value;

fn tool_payload_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn merge_desktop_assistant_array(blocks: &[Value]) -> Option<ConversationMessage> {
    if blocks.is_empty() {
        return None;
    }

    let mut reasoning = String::new();
    let mut text_parts = String::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();

    for b in blocks {
        let obj = match b.as_object() {
            Some(o) => o,
            None => continue,
        };
        let ty = match obj.get("type").and_then(|x| x.as_str()) {
            Some(t) => t,
            None => continue,
        };
        match ty {
            "thinking" => {
                if let Some(ts) = b.get("thinking").and_then(Value::as_str) {
                    reasoning.push_str(ts);
                    reasoning.push('\n');
                }
            }
            "text" => {
                if let Some(ts) = b.get("text").and_then(Value::as_str) {
                    text_parts.push_str(ts);
                    text_parts.push('\n');
                }
            }
            "tool_use" => {
                let Some(id_s) = b.get("id").and_then(Value::as_str) else {
                    continue;
                };
                let Some(name_s) = b.get("name").and_then(Value::as_str) else {
                    continue;
                };
                let id = id_s.to_string();
                let name = name_s.to_string();
                let input_val = b.get("input").cloned().unwrap_or(Value::Null);
                let arguments =
                    serde_json::to_string(&input_val).unwrap_or_else(|_| "{}".to_string());
                tool_calls.push(ToolCall {
                    id,
                    name,
                    arguments,
                });
            }
            _ => continue,
        }
    }

    if tool_calls.is_empty()
        && text_parts.trim().is_empty()
        && reasoning.trim().is_empty()
    {
        return None;
    }

    let reasoning_content = reasoning.trim_end().trim();
    let mut reasoning_opt =
        (!reasoning_content.is_empty()).then(|| reasoning_content.to_string());

    let text_trimmed = text_parts.trim_end();
    let text_opt = (!text_trimmed.is_empty()).then(|| text_trimmed.to_string());

    let text_payload = match (&text_opt, tool_calls.is_empty()) {
        (Some(t), _) => Some(t.clone()),
        (None, false) => Some(String::new()),
        (None, true) => None,
    };

    if !tool_calls.is_empty() && reasoning_opt.is_none() {
        reasoning_opt = Some(
            "(chain-of-thought unavailable — rehydrated tool-call turn had no stored thinking block)"
                .to_string(),
        );
    }

    Some(ConversationMessage::AssistantToolCalls {
        text: text_payload,
        tool_calls,
        reasoning_content: reasoning_opt,
    })
}

pub(crate) fn hydrate_gateway_sqlite_messages(messages: &[ChatMessage]) -> Vec<ConversationMessage> {
    let mut out = Vec::new();

    for msg in messages {
        if msg.role == "system" {
            continue;
        }

        match msg.role.as_str() {
            "assistant" => {
                let s = msg.content.trim();
                if s.starts_with('[') {
                    if let Ok(vals) = serde_json::from_str::<Vec<Value>>(s) {
                        if let Some(m) = merge_desktop_assistant_array(&vals) {
                            out.push(m);
                            continue;
                        }
                    }
                }
                out.push(ConversationMessage::Chat(msg.clone()));
            }
            "tool" => {
                let s = msg.content.trim();
                if let Ok(vals) = serde_json::from_str::<Vec<Value>>(s) {
                    let mut extracted: Vec<ToolResultMessage> = Vec::new();
                    let mut invalid = false;
                    for v in vals {
                        let ty = match v.get("type").and_then(Value::as_str) {
                            Some(t) => t,
                            None => {
                                invalid = true;
                                break;
                            }
                        };
                        if ty != "tool_result" {
                            invalid = true;
                            break;
                        }
                        let Some(tool_use_id) = v.get("tool_use_id").and_then(Value::as_str).map(ToString::to_string) else {
                            invalid = true;
                            break;
                        };
                        let content_val = match v.get("content") {
                            Some(c) => tool_payload_string(c),
                            None => String::new(),
                        };
                        let is_error = v
                            .get("is_error")
                            .and_then(Value::as_bool)
                            .unwrap_or(false)
                            || v
                                .get("isError")
                                .and_then(Value::as_bool)
                                .unwrap_or(false);
                        let mut body = content_val;
                        if is_error && !body.starts_with("Error") {
                            body = format!("Error: {body}");
                        }
                        extracted.push(ToolResultMessage {
                            tool_call_id: tool_use_id,
                            content: body,
                        });
                    }
                    if !invalid && !extracted.is_empty() {
                        out.push(ConversationMessage::ToolResults(extracted));
                        continue;
                    }
                }
                out.push(ConversationMessage::Chat(msg.clone()));
            }
            _ => out.push(ConversationMessage::Chat(msg.clone())),
        }
    }

    out
}
