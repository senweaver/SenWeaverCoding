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
    let assistant_text = turn.response.content.as_deref()?.trim().to_string();
    if assistant_text.is_empty() {
        return None;
    }
    let mut steps: Vec<serde_json::Value> = Vec::new();
    for call in &turn.response.tool_calls {
        steps.push(serde_json::json!({
            "kind": "tool_call",
            "name": call.name,
            "arguments": call.arguments,
        }));
    }
    for outcome in &turn.tool_outcomes {
        steps.push(serde_json::json!({
            "kind": "tool_outcome",
            "name": outcome.name,
            "ok": outcome.ok,
            "latency_ms": outcome.latency_ms,
        }));
    }
    steps.push(serde_json::json!({
        "kind": "final_response",
        "text": redact_text(&assistant_text, options, cfg),
    }));
    Some(serde_json::json!({
        "trajectory": steps,
        "task_summary": turn
            .openai_messages
            .iter()
            .rfind(|m| m.role == "user")
            .and_then(|m| m.content.clone())
            .map(|c| redact_text(&c, options, cfg))
            .unwrap_or_default(),
        "reward": turn.reward.final_score,
        "metadata": {
            "turn_id": turn.id,
            "session_id": turn.session_id,
            "coding_mode": turn.coding_mode,
            "model": turn.model,
            "provider": turn.provider,
        },
    }))
}
