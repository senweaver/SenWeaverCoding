// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::evolution::types::{
    AnthropicBlockView, AnthropicMessageView, EvolutionExportConfig, TurnRecord,
};

use super::{ExportOptions, redact_text};

pub fn project(
    turn: &TurnRecord,
    options: &ExportOptions,
    cfg: &EvolutionExportConfig,
) -> Option<serde_json::Value> {
    let assistant_text = turn.response.content.as_deref()?;
    if assistant_text.trim().is_empty() {
        return None;
    }
    if turn.anthropic_messages.is_empty() {
        return None;
    }
    let mut messages: Vec<serde_json::Value> = turn
        .anthropic_messages
        .iter()
        .map(|m| project_message(m, options, cfg))
        .collect();
    messages.push(serde_json::json!({
        "role": "assistant",
        "content": [{
            "type": "text",
            "text": redact_text(assistant_text, options, cfg),
        }],
    }));
    let mut payload = serde_json::Map::new();
    if let Some(ref system) = turn.anthropic_system {
        payload.insert(
            "system".into(),
            serde_json::Value::String(redact_text(system, options, cfg)),
        );
    }
    payload.insert("messages".into(), serde_json::Value::Array(messages));
    payload.insert(
        "metadata".into(),
        serde_json::json!({
            "turn_id": turn.id,
            "session_id": turn.session_id,
            "reward": turn.reward.final_score,
            "model": turn.model,
            "provider": turn.provider,
        }),
    );
    Some(serde_json::Value::Object(payload))
}

fn project_message(
    msg: &AnthropicMessageView,
    options: &ExportOptions,
    cfg: &EvolutionExportConfig,
) -> serde_json::Value {
    let blocks: Vec<serde_json::Value> = msg
        .content
        .iter()
        .map(|b| project_block(b, options, cfg))
        .collect();
    serde_json::json!({
        "role": msg.role,
        "content": blocks,
    })
}

fn project_block(
    block: &AnthropicBlockView,
    options: &ExportOptions,
    cfg: &EvolutionExportConfig,
) -> serde_json::Value {
    match block.kind.as_str() {
        "text" => {
            let text = block.text.clone().unwrap_or_default();
            serde_json::json!({
                "type": "text",
                "text": redact_text(&text, options, cfg),
            })
        }
        "tool_use" => serde_json::json!({
            "type": "tool_use",
            "id": block.id.clone().unwrap_or_default(),
            "name": block.name.clone().unwrap_or_default(),
            "input": block.input.clone().unwrap_or(serde_json::Value::Null),
        }),
        "tool_result" => {
            let content_text = match &block.content {
                Some(serde_json::Value::String(s)) => redact_text(s, options, cfg),
                Some(other) => redact_text(&other.to_string(), options, cfg),
                None => String::new(),
            };
            serde_json::json!({
                "type": "tool_result",
                "tool_use_id": block.tool_use_id.clone().unwrap_or_default(),
                "content": content_text,
            })
        }
        other => serde_json::json!({
            "type": other,
        }),
    }
}
