// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionRunStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Skipped,
}

impl ReflectionRunStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "skipped" => Self::Skipped,
            _ => Self::Failed,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectionWritebackReport {
    pub lessons_written: u32,
    pub skills_written: u32,
    pub rules_written: u32,
    pub memory_written: u32,
    #[serde(default)]
    pub errors: Vec<String>,
}

impl Default for ReflectionWritebackReport {
    fn default() -> Self {
        Self {
            lessons_written: 0,
            skills_written: 0,
            rules_written: 0,
            memory_written: 0,
            errors: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectionRun {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub trigger: String,
    pub depth: String,
    pub status: ReflectionRunStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub lessons_produced: u32,
    pub turns_analyzed: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReflectionSummary {
    pub total_runs: u64,
    pub completed_runs: u64,
    pub failed_runs: u64,
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_status: Option<String>,
    pub total_lessons_produced: u64,
}
