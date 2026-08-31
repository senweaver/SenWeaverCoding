// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEvent {
    pub timestamp: DateTime<Utc>,
    pub kind: SessionEventKind,
}

impl SessionEvent {
    pub fn new(kind: SessionEventKind) -> Self {
        Self {
            timestamp: Utc::now(),
            kind,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionEventKind {

    TurnStarted { input: String },

    FirstToken { agent_id: String, elapsed_ms: u64 },

    Delta { text: String },

    Thinking { text: String },

    StreamReset,

    FileEdit {
        path: String,
        additions: i32,
        deletions: i32,
    },

    ToolCall {
        tool_name: String,
        tool_call_id: String,
        arguments: serde_json::Value,
    },

    ToolResult {
        tool_call_id: String,
        output: String,
        is_error: bool,
    },

    TurnFinished { output: String, tokens_used: u64 },

    Error { message: String },

    ContextCompressed {
        tokens_before: usize,
        tokens_after: usize,
    },

    ModeChanged { mode: String },

    WritePlanCreated {
        goal: String,
        summary: String,
        steps: u32,
    },

    WriteStepStarted { index: u32, label: String },

    WriteStepFinished {
        index: u32,
        label: String,
        ok: bool,
        summary: String,
    },

    WriteVerify { status: String },

    DiffSessionApplied {
        files: u32,
        hunks_exact: u32,
        hunks_fuzzy: u32,
    },

    DiffSessionRolledBack { files: u32 },

    ApprovalRequested {
        id: String,
        tool_name: String,
        arguments: serde_json::Value,
        issued_at: DateTime<Utc>,
    },

    ApprovalResponded {
        id: String,
        decision: String,
        responder: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        updated_input: Option<serde_json::Value>,
    },

    CheckpointCreated {
        cp_id: String,
        edit_batch_id: Option<String>,
    },

    OpenFileMarked {
        path: String,
        cursor: Option<(u32, u32)>,
        source: String,
    },

    ProviderRetry {
        attempt: u32,
        max_attempts: u32,
        wait_ms: u64,
        class: String,
        provider: String,
        model: String,
        message: String,
    },

    WorkerSpawned {
        parent_tool_use_id: String,
        worker_id: String,
        title: String,
        model: String,
    },

    WorkerStatus {
        worker_id: String,
        status: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },

    WorkerProgress {
        worker_id: String,
        action: String,
        detail: String,
    },

    WorkerCompleted {
        worker_id: String,
        success: bool,
        summary: String,
    },

    WorkerStopped {
        worker_id: String,
        reason: String,
    },

    ParentResumed { reason: String },
}
