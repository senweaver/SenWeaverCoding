// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod companion;
pub mod prompt;
pub mod types;

use companion::Companion;
use types::{BuddyConfig, BuddyEvent};

pub fn lifecycle_event(config: &BuddyConfig, phase: &str) -> Option<(BuddyEvent, String)> {
    if !config.enabled {
        return None;
    }
    let mut companion = Companion::new(config.clone());
    match phase {
        "thinking" => companion.on_agent_thinking(),
        "working" => companion.on_agent_working(),
        "completed" => companion.on_task_completed(),
        "error" => companion.on_error(),
        "idle" => companion.on_idle(),
        _ => return None,
    }
    Some((
        BuddyEvent::MoodChanged {
            mood: companion.mood(),
        },
        companion.greeting(),
    ))
}
