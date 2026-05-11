// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecycledExperienceOutcome {
    Success,
    Failure,
    Neutral,
}

impl RecycledExperienceOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Neutral => "neutral",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "success" => Self::Success,
            "failure" => Self::Failure,
            _ => Self::Neutral,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecycledExperience {
    pub id: String,
    pub session_id: String,
    pub turn_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coding_mode: Option<String>,
    pub outcome: RecycledExperienceOutcome,
    pub reward: f32,
    pub headline: String,
    pub context_excerpt: String,
    pub response_excerpt: String,
    pub tools_summary: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub shape_signature: String,
    #[serde(default)]
    pub hits: u64,
    pub created_at: DateTime<Utc>,
}
