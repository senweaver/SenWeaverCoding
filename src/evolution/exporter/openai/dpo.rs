// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::evolution::types::{EvolutionExportConfig, TurnRecord};

use super::super::{ExportContext, ExportOptions, redact_text};

pub fn project(
    turn: &TurnRecord,
    options: &ExportOptions,
    cfg: &EvolutionExportConfig,
    ctx: &ExportContext,
) -> Option<serde_json::Value> {
    let chosen = turn.response.content.as_deref()?.trim().to_string();
    if chosen.is_empty() || turn.reward.final_score < 0.5 {
        return None;
    }
    let rejected = ctx.pick_rejected(turn)?;
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
    Some(serde_json::json!({
        "prompt": history,
        "chosen": [{
            "role": "assistant",
            "content": redact_text(&chosen, options, cfg),
        }],
        "rejected": [{
            "role": "assistant",
            "content": redact_text(&rejected.content, options, cfg),
        }],
        "metadata": {
            "turn_id": turn.id,
            "reward": turn.reward.final_score,
            "rejected_kind": "low_reward_turn",
            "rejected_turn_id": rejected.turn_id,
            "rejected_reward": rejected.reward,
        },
    }))
}
