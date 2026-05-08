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
    let chosen = turn.response.content.as_deref()?.trim().to_string();
    if chosen.is_empty() || turn.reward.final_score < 0.5 {
        return None;
    }
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
    let rejected = turn
        .response
        .thinking
        .clone()
        .unwrap_or_else(|| "(no rejected sample available)".to_string());
    Some(serde_json::json!({
        "prompt": prompt,
        "chosen": redact_text(&chosen, options, cfg),
        "rejected": redact_text(&rejected, options, cfg),
        "score_chosen": turn.reward.final_score,
        "score_rejected": -turn.reward.final_score,
        "metadata": {
            "turn_id": turn.id,
            "rejected_kind": "self_thinking",
        },
    }))
}
