// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::Serialize;

pub const PHASE_STATUS_NOT_STARTED: &str = "not_started";
pub const PHASE_STATUS_IN_PROGRESS: &str = "in_progress";
pub const PHASE_STATUS_DONE: &str = "done";
pub const PHASE_STATUS_BLOCKED: &str = "blocked";

pub const TASK_STATUS_TODO: &str = "todo";
pub const TASK_STATUS_IN_PROGRESS: &str = "in_progress";
pub const TASK_STATUS_DONE: &str = "done";
pub const TASK_STATUS_BLOCKED: &str = "blocked";

pub const TASK_KIND_TASK: &str = "task";
pub const TASK_KIND_MILESTONE: &str = "milestone";

pub const TASK_PRIORITY_LOW: &str = "low";
pub const TASK_PRIORITY_MEDIUM: &str = "medium";
pub const TASK_PRIORITY_HIGH: &str = "high";
pub const TASK_PRIORITY_URGENT: &str = "urgent";

pub const CHAT_KIND_TEXT: &str = "text";
pub const CHAT_KIND_FILE: &str = "file";
pub const CHAT_KIND_SYSTEM: &str = "system";

pub struct DefaultPhase {
    pub phase_id: &'static str,
    pub name: &'static str,
    pub color: &'static str,
}

pub fn default_phases() -> Vec<DefaultPhase> {
    vec![
        DefaultPhase {
            phase_id: "requirements",
            name: "Requirements",
            color: "#6366f1",
        },
        DefaultPhase {
            phase_id: "design",
            name: "Design",
            color: "#8b5cf6",
        },
        DefaultPhase {
            phase_id: "development",
            name: "Development",
            color: "#0ea5e9",
        },
        DefaultPhase {
            phase_id: "testing",
            name: "Testing",
            color: "#f59e0b",
        },
        DefaultPhase {
            phase_id: "deployment",
            name: "Deployment",
            color: "#10b981",
        },
        DefaultPhase {
            phase_id: "maintenance",
            name: "Maintenance",
            color: "#64748b",
        },
    ]
}

pub fn normalize_phase_status(value: &str) -> &'static str {
    match value {
        PHASE_STATUS_IN_PROGRESS => PHASE_STATUS_IN_PROGRESS,
        PHASE_STATUS_DONE => PHASE_STATUS_DONE,
        PHASE_STATUS_BLOCKED => PHASE_STATUS_BLOCKED,
        _ => PHASE_STATUS_NOT_STARTED,
    }
}

pub fn normalize_task_status(value: &str) -> &'static str {
    match value {
        TASK_STATUS_IN_PROGRESS => TASK_STATUS_IN_PROGRESS,
        TASK_STATUS_DONE => TASK_STATUS_DONE,
        TASK_STATUS_BLOCKED => TASK_STATUS_BLOCKED,
        _ => TASK_STATUS_TODO,
    }
}

pub fn normalize_task_kind(value: &str) -> &'static str {
    match value {
        TASK_KIND_MILESTONE => TASK_KIND_MILESTONE,
        _ => TASK_KIND_TASK,
    }
}

pub fn normalize_task_priority(value: &str) -> &'static str {
    match value {
        TASK_PRIORITY_LOW => TASK_PRIORITY_LOW,
        TASK_PRIORITY_HIGH => TASK_PRIORITY_HIGH,
        TASK_PRIORITY_URGENT => TASK_PRIORITY_URGENT,
        _ => TASK_PRIORITY_MEDIUM,
    }
}

pub fn normalize_chat_kind(value: &str) -> &'static str {
    match value {
        CHAT_KIND_FILE => CHAT_KIND_FILE,
        CHAT_KIND_SYSTEM => CHAT_KIND_SYSTEM,
        _ => CHAT_KIND_TEXT,
    }
}

pub fn phase_status_base_percent(status: &str) -> f64 {
    match normalize_phase_status(status) {
        PHASE_STATUS_DONE => 100.0,
        PHASE_STATUS_IN_PROGRESS => 50.0,
        PHASE_STATUS_BLOCKED => 25.0,
        _ => 0.0,
    }
}

pub fn task_progress_value(status: &str, progress: i64) -> f64 {
    match normalize_task_status(status) {
        TASK_STATUS_DONE => 100.0,
        TASK_STATUS_BLOCKED => progress.clamp(0, 100) as f64,
        _ => progress.clamp(0, 100) as f64,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub role: String,
    #[serde(rename = "memberCount")]
    pub member_count: i64,
    #[serde(rename = "docCount")]
    pub doc_count: i64,
    #[serde(rename = "taskCount")]
    pub task_count: i64,
    #[serde(rename = "openTaskCount")]
    pub open_task_count: i64,
    #[serde(rename = "phaseCount")]
    pub phase_count: i64,
    pub progress: f64,
    #[serde(rename = "unread")]
    pub unread: i64,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemberView {
    #[serde(rename = "userId")]
    pub user_id: String,
    pub nickname: String,
    pub role: String,
    pub online: bool,
    #[serde(rename = "joinedAt")]
    pub joined_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PhaseView {
    pub id: String,
    pub name: String,
    pub order: i64,
    pub status: String,
    pub color: String,
    pub percent: f64,
    #[serde(rename = "docCount")]
    pub doc_count: i64,
    #[serde(rename = "taskCount")]
    pub task_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocumentView {
    pub id: String,
    pub name: String,
    #[serde(rename = "isDir")]
    pub is_dir: bool,
    pub size: i64,
    #[serde(rename = "phaseId")]
    pub phase_id: String,
    pub uploader: String,
    #[serde(rename = "uploaderNickname")]
    pub uploader_nickname: String,
    #[serde(rename = "contentHash")]
    pub content_hash: String,
    pub version: i64,
    pub note: String,
    pub available: bool,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskView {
    pub id: String,
    pub title: String,
    pub description: String,
    #[serde(rename = "phaseId")]
    pub phase_id: String,
    pub assignee: String,
    #[serde(rename = "assigneeNickname")]
    pub assignee_nickname: String,
    pub status: String,
    pub priority: String,
    #[serde(rename = "dueMs")]
    pub due_ms: i64,
    pub deps: Vec<String>,
    pub parent: String,
    pub kind: String,
    pub progress: i64,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupMessageView {
    pub id: String,
    pub author: String,
    #[serde(rename = "authorNickname")]
    pub author_nickname: String,
    pub body: String,
    pub kind: String,
    #[serde(rename = "docId")]
    pub doc_id: String,
    #[serde(rename = "tsMs")]
    pub ts_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupSnapshot {
    pub group: GroupSummary,
    pub members: Vec<MemberView>,
    pub phases: Vec<PhaseView>,
    pub documents: Vec<DocumentView>,
    pub tasks: Vec<TaskView>,
}
