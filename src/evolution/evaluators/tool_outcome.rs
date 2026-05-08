// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use chrono::Utc;

use super::FastEvaluator;
use crate::evolution::types::{SignalScore, SignalSource, TurnRecord};

pub struct ToolOutcomeEvaluator;

impl FastEvaluator for ToolOutcomeEvaluator {
    fn evaluate(&self, turn: &TurnRecord) -> Option<SignalScore> {
        if turn.tool_outcomes.is_empty() {
            return None;
        }
        let total = turn.tool_outcomes.len() as f32;
        let ok_count = turn.tool_outcomes.iter().filter(|o| o.ok).count() as f32;
        let success_rate = ok_count / total;
        let score = (success_rate * 2.0 - 1.0).clamp(-1.0, 1.0);
        Some(SignalScore {
            source: SignalSource::Tool,
            score,
            confidence: total.min(8.0) / 8.0,
            reason: Some(format!(
                "{}/{} tools succeeded",
                ok_count as u64, total as u64
            )),
            ts: Utc::now(),
        })
    }
}
