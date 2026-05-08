// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::types::{EvolutionSignalWeights, Reward, SignalScore, SignalSource};

pub fn fuse_signals(scores: &[SignalScore], weights: &EvolutionSignalWeights) -> Reward {
    let mut reward = Reward::default();
    for score in scores {
        apply_signal(&mut reward, score);
    }
    recompute_final(&mut reward, weights);
    reward
}

pub fn merge_signal(reward: &mut Reward, score: &SignalScore, weights: &EvolutionSignalWeights) {
    apply_signal(reward, score);
    recompute_final(reward, weights);
}

fn apply_signal(reward: &mut Reward, score: &SignalScore) {
    match score.source {
        SignalSource::UserThumbs => reward.thumbs = Some(score.score),
        SignalSource::NextState => reward.next_state = Some(score.score),
        SignalSource::Tool => reward.tool = Some(score.score),
        SignalSource::Verification => reward.verification = Some(score.score),
        SignalSource::Cost => reward.cost = Some(score.score),
    }
}

fn recompute_final(reward: &mut Reward, weights: &EvolutionSignalWeights) {
    let components: [(Option<f32>, f32); 5] = [
        (reward.thumbs, weights.thumbs),
        (reward.next_state, weights.next_state),
        (reward.tool, weights.tool),
        (reward.verification, weights.verification),
        (reward.cost, weights.cost),
    ];
    let mut total_w = 0.0_f32;
    let mut acc = 0.0_f32;
    let mut any = false;
    for (value, weight) in components {
        if let Some(v) = value {
            total_w += weight;
            acc += v * weight;
            any = true;
        }
    }
    reward.final_score = if total_w > f32::EPSILON {
        (acc / total_w).clamp(-1.0, 1.0)
    } else {
        0.0
    };
    reward.loss_mask = u8::from(any);
}
