// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod companion;
pub mod prompt;
pub mod types;

use std::sync::OnceLock;

use parking_lot::Mutex;

use companion::Companion;
use types::{BuddyConfig, BuddyEvent};

fn companions() -> &'static Mutex<std::collections::HashMap<String, Companion>> {
    static COMPANIONS: OnceLock<Mutex<std::collections::HashMap<String, Companion>>> =
        OnceLock::new();
    COMPANIONS.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

fn buddy_session_key() -> String {
    crate::session::current_session_context()
        .map(|c| c.session_id)
        .unwrap_or_else(|| "__no_session__".to_string())
}

// Per-session companion so one session's mood/idle state never bleeds into
// another's (the buddy state was previously a single process-wide singleton).
fn with_companion<R>(config: &BuddyConfig, f: impl FnOnce(&mut Companion) -> R) -> R {
    let mut map = companions().lock();
    let companion = map
        .entry(buddy_session_key())
        .or_insert_with(|| Companion::new(config.clone()));
    f(companion)
}

pub fn lifecycle_event(config: &BuddyConfig, phase: &str) -> Option<(BuddyEvent, String)> {
    if !config.enabled {
        return None;
    }
    with_companion(config, |companion| {
        companion.set_config(config.clone());
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
    })
}

pub fn current_mood(config: &BuddyConfig) -> types::BuddyMood {
    with_companion(config, |companion| companion.mood())
}

pub fn idle_transition_event(config: &BuddyConfig) -> Option<(BuddyEvent, String)> {
    if !config.enabled {
        return None;
    }
    with_companion(config, |companion| {
        companion.set_config(config.clone());
        let before = companion.mood();
        companion.on_idle();
        let after = companion.mood();
        if before == after {
            return None;
        }
        Some((BuddyEvent::MoodChanged { mood: after }, companion.greeting()))
    })
}
