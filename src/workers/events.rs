// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Stopped,
}

impl WorkerStatus {
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Stopped)
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
        }
    }
}

impl std::fmt::Display for WorkerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerSpec {
    pub parent_session_id: String,
    pub parent_tool_use_id: String,
    pub title: String,
    pub prompt: String,

    pub context: Option<String>,

    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerResult {
    pub worker_id: String,
    pub title: String,
    pub status: WorkerStatus,
    pub output: String,

    pub error: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerSummary {
    pub worker_id: String,
    pub parent_session_id: String,
    pub parent_tool_use_id: String,
    pub title: String,
    pub model: String,
    pub status: WorkerStatus,

    pub last_action: Option<String>,

    pub last_detail: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerMeta {
    pub worker_id: String,
    pub parent_session_id: String,
    pub parent_tool_use_id: String,
    pub title: String,
    pub prompt: String,

    pub context: Option<String>,
    pub model: String,
    pub status: WorkerStatus,

    pub last_action: Option<String>,

    pub last_detail: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,

    pub output: Option<String>,

    pub error: Option<String>,
}

impl WorkerMeta {
    #[must_use]
    pub fn from_spec(worker_id: String, spec: &WorkerSpec, model: String) -> Self {
        Self {
            worker_id,
            parent_session_id: spec.parent_session_id.clone(),
            parent_tool_use_id: spec.parent_tool_use_id.clone(),
            title: spec.title.clone(),
            prompt: spec.prompt.clone(),
            context: spec.context.clone(),
            model,
            status: WorkerStatus::Pending,
            last_action: None,
            last_detail: None,
            started_at: Utc::now(),
            finished_at: None,
            output: None,
            error: None,
        }
    }

    #[must_use]
    pub fn to_summary(&self) -> WorkerSummary {
        WorkerSummary {
            worker_id: self.worker_id.clone(),
            parent_session_id: self.parent_session_id.clone(),
            parent_tool_use_id: self.parent_tool_use_id.clone(),
            title: self.title.clone(),
            model: self.model.clone(),
            status: self.status,
            last_action: self.last_action.clone(),
            last_detail: self.last_detail.clone(),
            started_at: self.started_at,
            finished_at: self.finished_at,
        }
    }
}
