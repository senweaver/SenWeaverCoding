// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use chrono::Utc;

use crate::evolution::types::{SignalScore, SignalSource, ThumbVote};

pub fn score_from_vote(vote: &ThumbVote) -> SignalScore {
    let raw = f32::from(vote.score).clamp(-1.0, 1.0);
    SignalScore {
        source: SignalSource::UserThumbs,
        score: raw,
        confidence: 1.0,
        reason: vote.comment.clone(),
        ts: Utc::now(),
    }
}
