// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::evolution::types::{EvolutionExportConfig, TurnRecord};

use super::super::{ExportOptions, redact_text};

pub fn project(
    turn: &TurnRecord,
    options: &ExportOptions,
    cfg: &EvolutionExportConfig,
) -> Option<serde_json::Value> {
    let chosen = turn.response.content.as_deref()?.trim().to_string();
    if chosen.is_empty() || turn.reward.final_score < 0.5 {
        return None;
    }
    let history: Vec<serde_json::Value> = turn
        .openai_messages
        .iter()
        .filter_map(|m| {
            let content = m.content.clone()?;
            Some(serde_json::json!({
                "role": m.role,
                "content": redact_text(&content, options, cfg),
            }))
        })
        .collect();
    let rejected_seed = turn
        .response
        .thinking
        .clone()
        .unwrap_or_else(|| "(no rejected sample available)".to_string());
    Some(serde_json::json!({
        "prompt": history,
        "chosen": [{
            "role": "assistant",
            "content": redact_text(&chosen, options, cfg),
        }],
        "rejected": [{
            "role": "assistant",
            "content": redact_text(&rejected_seed, options, cfg),
        }],
        "metadata": {
            "turn_id": turn.id,
            "reward": turn.reward.final_score,
            "rejected_kind": "self_thinking",
        },
    }))
}
