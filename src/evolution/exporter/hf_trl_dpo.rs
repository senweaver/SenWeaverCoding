// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::evolution::types::{EvolutionExportConfig, TurnRecord};

use super::{ExportContext, ExportOptions, redact_text};

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
    let prompt = turn
        .openai_messages
        .iter()
        .filter_map(|m| {
            let content = m.content.clone()?;
            Some(format!(
                "{}: {}",
                m.role,
                redact_text(&content, options, cfg)
            ))
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(serde_json::json!({
        "prompt": prompt,
        "chosen": redact_text(&chosen, options, cfg),
        "rejected": redact_text(&rejected.content, options, cfg),
        "score_chosen": turn.reward.final_score,
        "score_rejected": rejected.reward,
        "metadata": {
            "turn_id": turn.id,
            "rejected_kind": "low_reward_turn",
            "rejected_turn_id": rejected.turn_id,
        },
    }))
}
