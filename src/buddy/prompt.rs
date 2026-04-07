// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Buddy prompt — mirrors claude-code-typescript-src`buddy/prompt.ts`.
// Generates contextual prompts for the buddy companion.

use super::types::BuddyMood;

/// Generate a system prompt addition for the buddy personality.
pub fn buddy_system_prompt(name: &str, personality: &str, mood: BuddyMood) -> String {
    let mood_instruction = match mood {
        BuddyMood::Happy => "You are in a great mood. Be enthusiastic and encouraging.",
        BuddyMood::Thinking => "You are deep in thought. Be analytical and methodical.",
        BuddyMood::Working => "You are focused on the task. Be efficient and precise.",
        BuddyMood::Celebrating => "A task was just completed successfully! Be celebratory.",
        BuddyMood::Confused => "Something is unclear. Ask clarifying questions.",
        BuddyMood::Sleeping => "You were idle for a while. Be ready to re-engage.",
        BuddyMood::Error => "An error occurred. Be calm and solution-oriented.",
        BuddyMood::Neutral => "You are ready and attentive.",
    };

    format!(
        "Your companion name is {name}. \
         Your personality is {personality}. \
         Current mood context: {mood_instruction}"
    )
}
