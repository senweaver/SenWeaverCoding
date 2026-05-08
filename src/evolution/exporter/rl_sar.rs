// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::evolution::types::{EvolutionExportConfig, TurnRecord};

use super::{ExportOptions, redact_text};

pub fn project(
    turn: &TurnRecord,
    options: &ExportOptions,
    cfg: &EvolutionExportConfig,
) -> Option<serde_json::Value> {
    let action_text = turn.response.content.as_deref()?.trim().to_string();
    if action_text.is_empty() {
        return None;
    }
    let state_messages: Vec<serde_json::Value> = turn
        .openai_messages
        .iter()
        .map(|m| {
            let content = m
                .content
                .clone()
                .map(|c| redact_text(&c, options, cfg))
                .unwrap_or_default();
            serde_json::json!({"role": m.role, "content": content})
        })
        .collect();
    let tool_outcomes: Vec<serde_json::Value> = turn
        .tool_outcomes
        .iter()
        .map(|o| {
            serde_json::json!({
                "name": o.name,
                "ok": o.ok,
                "exit_code": o.exit_code,
                "latency_ms": o.latency_ms,
            })
        })
        .collect();
    Some(serde_json::json!({
        "state": state_messages,
        "action": redact_text(&action_text, options, cfg),
        "tool_outcomes": tool_outcomes,
        "reward": {
            "final": turn.reward.final_score,
            "thumbs": turn.reward.thumbs,
            "next_state": turn.reward.next_state,
            "tool": turn.reward.tool,
            "verification": turn.reward.verification,
            "cost": turn.reward.cost,
        },
        "metadata": {
            "turn_id": turn.id,
            "session_id": turn.session_id,
            "coding_mode": turn.coding_mode,
            "model": turn.model,
            "provider": turn.provider,
        },
    }))
}
