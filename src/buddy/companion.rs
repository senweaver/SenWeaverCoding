// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::types::{BuddyConfig, BuddyMood};

pub struct Companion {
    config: BuddyConfig,
    mood: BuddyMood,
    idle_since_ms: u64,
}

impl Companion {
    pub fn new(config: BuddyConfig) -> Self {
        Self {
            config,
            mood: BuddyMood::Neutral,
            idle_since_ms: now_ms(),
        }
    }

    pub fn mood(&self) -> BuddyMood {
        self.mood
    }

    pub fn set_config(&mut self, config: BuddyConfig) {
        self.config = config;
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn on_agent_thinking(&mut self) {
        self.mood = BuddyMood::Thinking;
        self.idle_since_ms = now_ms();
    }

    pub fn on_agent_working(&mut self) {
        self.mood = BuddyMood::Working;
        self.idle_since_ms = now_ms();
    }

    pub fn on_task_completed(&mut self) {
        self.mood = BuddyMood::Celebrating;
        self.idle_since_ms = now_ms();
    }

    pub fn on_error(&mut self) {
        self.mood = BuddyMood::Error;
        self.idle_since_ms = now_ms();
    }

    pub fn on_idle(&mut self) {
        let elapsed = now_ms().saturating_sub(self.idle_since_ms);
        if elapsed > 300_000 {

            self.mood = BuddyMood::Sleeping;
        } else if elapsed > 60_000 {
            self.mood = BuddyMood::Neutral;
        }
    }

    pub fn greeting(&self) -> String {
        let name = &self.config.name;
        match self.mood {
            BuddyMood::Happy => format!("{name} is happy to help!"),
            BuddyMood::Thinking => format!("{name} is thinking..."),
            BuddyMood::Working => format!("{name} is working on it..."),
            BuddyMood::Celebrating => format!("{name} celebrates! Task done!"),
            BuddyMood::Confused => format!("{name} is a bit confused..."),
            BuddyMood::Sleeping => format!("{name} is resting. Wake me up anytime!"),
            BuddyMood::Error => format!("{name} encountered an issue."),
            BuddyMood::Neutral => format!("{name} is ready."),
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
