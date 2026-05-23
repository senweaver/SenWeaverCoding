// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use super::pipeline::rank_experiences;
use super::types::RecycledExperienceOutcome;
use crate::evolution::EvolutionEngine;

const APPROX_CHARS_PER_TOKEN: usize = 4;

pub fn build_recycled_block(
    engine: &Arc<EvolutionEngine>,
    coding_mode: Option<&str>,
) -> Option<String> {
    let snapshot = engine.config_snapshot();
    if !snapshot.recycling.enabled {
        return None;
    }
    let store = engine.recycling_store()?;
    let recent_limit = snapshot.recycling.max_retained.clamp(8, 200);
    let candidates = store.list_recent(recent_limit).ok()?;
    if candidates.is_empty() {
        return None;
    }
    let ranked = rank_experiences(candidates, coding_mode, &snapshot.recycling);
    if ranked.is_empty() {
        return None;
    }
    let max_replay = snapshot.recycling.max_replay_in_prompt.max(1);
    let char_budget = snapshot
        .recycling
        .replay_token_budget
        .max(64)
        .saturating_mul(APPROX_CHARS_PER_TOKEN);
    let mut chosen_ids: Vec<String> = Vec::new();
    let mut buf = String::from(
        "## Recycled past experiences\n\n\
         The following compact summaries were harvested from prior turns. \
         Treat them as soft hints: prefer the patterns that worked and avoid \
         repeating the failures. Ignore any that conflict with the current \
         task or with user instructions.\n\n",
    );
    let mut used_chars: usize = 0;
    let mut emitted = 0_usize;
    for entry in ranked {
        if emitted >= max_replay {
            break;
        }
        let formatted = format_experience(&entry.experience);
        let approx = formatted.chars().count() + 16;
        if used_chars + approx > char_budget && emitted > 0 {
            break;
        }
        buf.push_str(&formatted);
        buf.push('\n');
        used_chars += approx;
        chosen_ids.push(entry.experience.id.clone());
        emitted += 1;
    }
    if emitted == 0 {
        return None;
    }
    if let Err(error) = store.bump_hits(&chosen_ids) {
        tracing::debug!(error = %error, "evolution: failed to bump recycled experience hits");
    }
    Some(buf)
}

fn format_experience(exp: &super::types::RecycledExperience) -> String {
    let outcome_tag = match exp.outcome {
        RecycledExperienceOutcome::Success => "succeeded",
        RecycledExperienceOutcome::Failure => "failed",
        RecycledExperienceOutcome::Neutral => "neutral",
    };
    let mut text = format!("- **{}** ({outcome_tag}, reward {:.2})", exp.headline.trim(), exp.reward);
    if !exp.tools_summary.is_empty() {
        text.push_str(" — tools: ");
        text.push_str(exp.tools_summary.trim());
    }
    if !exp.context_excerpt.is_empty() {
        text.push_str("\n  context: ");
        text.push_str(exp.context_excerpt.trim());
    }
    if !exp.response_excerpt.is_empty() {
        text.push_str("\n  response: ");
        text.push_str(exp.response_excerpt.trim());
    }
    if !exp.tags.is_empty() {
        text.push_str("\n  tags: ");
        text.push_str(&exp.tags.join(", "));
    }
    text
}
