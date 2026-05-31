// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::evolution::types::{ChatMessageView, EvolutionExportConfig, TurnRecord};

use super::super::{ExportOptions, redact_text};

pub fn project(
    turn: &TurnRecord,
    options: &ExportOptions,
    cfg: &EvolutionExportConfig,
) -> Option<serde_json::Value> {
    let assistant_text = turn.response.content.as_deref()?;
    if assistant_text.trim().is_empty() {
        return None;
    }
    if turn.openai_messages.is_empty() {
        return None;
    }
    let mut messages: Vec<serde_json::Value> = Vec::with_capacity(turn.openai_messages.len() + 1);
    for msg in &turn.openai_messages {
        if let Some(value) = project_message(msg, options, cfg) {
            messages.push(value);
        }
    }
    messages.push(serde_json::json!({
        "role": "assistant",
        "content": redact_text(assistant_text, options, cfg),
    }));
    Some(serde_json::json!({
        "messages": messages,
        "metadata": {
            "turn_id": turn.id,
            "session_id": turn.session_id,
            "coding_mode": turn.coding_mode,
            "reward": turn.reward.final_score,
            "model": turn.model,
            "provider": turn.provider,
        },
    }))
}

fn project_message(
    msg: &ChatMessageView,
    options: &ExportOptions,
    cfg: &EvolutionExportConfig,
) -> Option<serde_json::Value> {
    let mut map = serde_json::Map::new();
    map.insert("role".into(), serde_json::Value::String(msg.role.clone()));
    if let Some(content) = msg.content.clone() {
        map.insert(
            "content".into(),
            serde_json::Value::String(redact_text(&content, options, cfg)),
        );
    }
    if let Some(name) = msg.name.clone() {
        map.insert("name".into(), serde_json::Value::String(name));
    }
    if let Some(tool_call_id) = msg.tool_call_id.clone() {
        map.insert(
            "tool_call_id".into(),
            serde_json::Value::String(tool_call_id),
        );
    }
    if !msg.tool_calls.is_empty() {
        map.insert(
            "tool_calls".into(),
            serde_json::to_value(&msg.tool_calls).ok()?,
        );
    }
    Some(serde_json::Value::Object(map))
}
