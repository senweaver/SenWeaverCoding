// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use chrono::Utc;

use super::FastEvaluator;
use crate::evolution::types::{SignalScore, SignalSource, TurnRecord};

const TARGET_TOKENS: f32 = 4_000.0;
const FAIL_TOKENS: f32 = 64_000.0;
const TARGET_USD: f32 = 0.05;
const FAIL_USD: f32 = 1.50;

pub struct CostEfficiencyEvaluator;

impl FastEvaluator for CostEfficiencyEvaluator {
    fn evaluate(&self, turn: &TurnRecord) -> Option<SignalScore> {
        if turn.cost.total_tokens == 0 && turn.cost.usd == 0.0 {
            return None;
        }
        let token_score = if turn.cost.total_tokens == 0 {
            1.0
        } else {
            interpolate(turn.cost.total_tokens as f32, TARGET_TOKENS, FAIL_TOKENS)
        };
        let usd_score = if turn.cost.usd <= 0.0 {
            1.0
        } else {
            #[allow(clippy::cast_possible_truncation)]
            let usd = turn.cost.usd as f32;
            interpolate(usd, TARGET_USD, FAIL_USD)
        };
        let score = ((token_score + usd_score) * 0.5).clamp(-1.0, 1.0);
        Some(SignalScore {
            source: SignalSource::Cost,
            score,
            confidence: 0.6,
            reason: Some(format!(
                "{} tokens / ${:.4}",
                turn.cost.total_tokens, turn.cost.usd
            )),
            ts: Utc::now(),
        })
    }
}

fn interpolate(value: f32, target: f32, fail: f32) -> f32 {
    if value <= target {
        return 1.0;
    }
    if value >= fail {
        return -1.0;
    }
    let span = (fail - target).max(f32::EPSILON);
    let normalised = (value - target) / span;
    (1.0 - normalised * 2.0).clamp(-1.0, 1.0)
}
