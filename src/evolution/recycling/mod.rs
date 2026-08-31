// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod harvester;
pub mod pipeline;
pub mod replay;
pub mod store;
pub mod types;

pub use harvester::{harvest_turn, RecyclingHarvestReport};
pub use pipeline::{rank_experiences, ExperienceRank};
pub use replay::build_recycled_block;
pub use store::RecyclingStore;
pub use types::{RecycledExperience, RecycledExperienceOutcome};

pub fn outcome_from_reward(final_score: f32) -> RecycledExperienceOutcome {
    if final_score >= 0.5 {
        RecycledExperienceOutcome::Success
    } else if final_score <= -0.5 {
        RecycledExperienceOutcome::Failure
    } else {
        RecycledExperienceOutcome::Neutral
    }
}
