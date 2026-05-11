// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionLessonKind {
    Insight,
    Avoid,
    Followup,
}

impl ReflectionLessonKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Insight => "insight",
            Self::Avoid => "avoid",
            Self::Followup => "followup",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "avoid" => Self::Avoid,
            "followup" => Self::Followup,
            _ => Self::Insight,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectionLesson {
    pub kind: ReflectionLessonKind,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl ReflectionLesson {
    pub fn new(kind: ReflectionLessonKind, title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            kind,
            title: title.into(),
            body: body.into(),
            tags: Vec::new(),
        }
    }
}
