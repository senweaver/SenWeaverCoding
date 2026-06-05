// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuddyMood {
    Happy,
    Thinking,
    Working,
    Celebrating,
    Confused,
    Sleeping,
    Error,
    Neutral,
}

impl std::fmt::Display for BuddyMood {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Happy => write!(f, "😊"),
            Self::Thinking => write!(f, "🤔"),
            Self::Working => write!(f, "⚙️"),
            Self::Celebrating => write!(f, "🎉"),
            Self::Confused => write!(f, "😕"),
            Self::Sleeping => write!(f, "💤"),
            Self::Error => write!(f, "😵"),
            Self::Neutral => write!(f, "🤖"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BuddyConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_buddy_name")]
    pub name: String,
    #[serde(default = "default_buddy_personality")]
    pub personality: String,
    #[serde(default = "default_true_bool")]
    pub show_notifications: bool,
}

impl BuddyConfig {
    pub fn is_disabled(&self) -> bool {
        !self.enabled
    }
}

fn default_buddy_name() -> String {
    "Sen".to_string()
}

fn default_buddy_personality() -> String {
    "friendly and helpful".to_string()
}

fn default_true_bool() -> bool {
    true
}

impl Default for BuddyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            name: "Sen".to_string(),
            personality: "friendly and helpful".to_string(),
            show_notifications: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BuddyEvent {
    MoodChanged { mood: BuddyMood },
    Notification { message: String },
    Tip { tip: String },
}
