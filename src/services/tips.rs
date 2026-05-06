// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Tips service — mirrors claude-code-typescript-src`services/tips/`.
// Provides contextual tips and suggestions to users during sessions.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tip {
    pub id: String,
    pub title: String,
    pub body: String,
    pub category: TipCategory,
    pub priority: u8,
    pub shown_count: u32,
    pub dismissed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TipCategory {
    GettingStarted,
    Productivity,
    Advanced,
    NewFeature,
    Keyboard,
}

pub struct TipManager {
    tips: Vec<Tip>,
    max_per_session: u32,
    shown_this_session: u32,
}

impl TipManager {
    pub fn new(max_per_session: u32) -> Self {
        Self {
            tips: default_tips(),
            max_per_session,
            shown_this_session: 0,
        }
    }

    pub fn next_tip(&mut self) -> Option<&Tip> {
        if self.shown_this_session >= self.max_per_session {
            return None;
        }
        self.tips
            .iter()
            .find(|t| !t.dismissed && t.shown_count == 0)
    }

    pub fn mark_shown(&mut self, id: &str) {
        if let Some(tip) = self.tips.iter_mut().find(|t| t.id == id) {
            tip.shown_count += 1;
            self.shown_this_session += 1;
        }
    }

    pub fn dismiss(&mut self, id: &str) {
        if let Some(tip) = self.tips.iter_mut().find(|t| t.id == id) {
            tip.dismissed = true;
        }
    }
}

fn default_tips() -> Vec<Tip> {
    vec![
        Tip {
            id: "compact".to_string(),
            title: "Context getting long?".to_string(),
            body: "Use /compact to summarize the conversation and free up context window.".to_string(),
            category: TipCategory::Productivity,
            priority: 1,
            shown_count: 0,
            dismissed: false,
        },
        Tip {
            id: "plan_mode".to_string(),
            title: "Plan before coding".to_string(),
            body: "Use /plan to enter plan mode — the agent will outline an approach before making changes.".to_string(),
            category: TipCategory::Productivity,
            priority: 2,
            shown_count: 0,
            dismissed: false,
        },
        Tip {
            id: "skills".to_string(),
            title: "Custom skills".to_string(),
            body: "Create reusable skills in .senweavercoding/skills/ to teach the agent project-specific workflows.".to_string(),
            category: TipCategory::Advanced,
            priority: 3,
            shown_count: 0,
            dismissed: false,
        },
        Tip {
            id: "memory".to_string(),
            title: "Persistent memory".to_string(),
            body: "Use /memory to manage what the agent remembers across sessions.".to_string(),
            category: TipCategory::GettingStarted,
            priority: 2,
            shown_count: 0,
            dismissed: false,
        },
    ]
}
