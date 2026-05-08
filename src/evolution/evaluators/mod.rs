// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod cost_efficiency;
pub mod tool_outcome;
pub mod user_thumbs;
pub mod verification;

use super::types::{TurnRecord, SignalScore};

pub trait FastEvaluator: Send + Sync {
    fn evaluate(&self, turn: &TurnRecord) -> Option<SignalScore>;
}

pub fn run_fast_evaluators(turn: &TurnRecord) -> Vec<SignalScore> {
    let evaluators: Vec<Box<dyn FastEvaluator>> = vec![
        Box::new(tool_outcome::ToolOutcomeEvaluator),
        Box::new(verification::VerificationEvaluator),
        Box::new(cost_efficiency::CostEfficiencyEvaluator),
    ];
    evaluators
        .iter()
        .filter_map(|ev| ev.evaluate(turn))
        .collect()
}
