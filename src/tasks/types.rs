// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Core task types — mirrors claude-code-typescript-src`Task.ts`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::watch;

// ---------------------------------------------------------------------------
// TaskType — the kind of background work
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    LocalBash,
    LocalAgent,
    RemoteAgent,
    InProcessTeammate,
    LocalWorkflow,
    MonitorMcp,
    Dream,
}

impl TaskType {
    /// Short prefix character for ID generation.
    pub fn id_prefix(self) -> char {
        match self {
            Self::LocalBash => 'b',
            Self::LocalAgent => 'a',
            Self::RemoteAgent => 'r',
            Self::InProcessTeammate => 't',
            Self::LocalWorkflow => 'w',
            Self::MonitorMcp => 'm',
            Self::Dream => 'd',
        }
    }
}

impl std::fmt::Display for TaskType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::LocalBash => "local_bash",
            Self::LocalAgent => "local_agent",
            Self::RemoteAgent => "remote_agent",
            Self::InProcessTeammate => "in_process_teammate",
            Self::LocalWorkflow => "local_workflow",
            Self::MonitorMcp => "monitor_mcp",
            Self::Dream => "dream",
        };
        f.write_str(s)
    }
}

// ---------------------------------------------------------------------------
// TaskStatus
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Killed,
}

/// Returns `true` when the status is terminal (no further transitions).
pub fn is_terminal_status(status: TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Killed
    )
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Killed => "killed",
        };
        f.write_str(s)
    }
}

// ---------------------------------------------------------------------------
// TaskId
// ---------------------------------------------------------------------------

/// Typed task identifier: `<prefix><8 alphanumeric chars>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(pub String);

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

const TASK_ID_ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// Generate a random task ID with a type-specific prefix.
pub fn generate_task_id(task_type: TaskType) -> TaskId {
    let prefix = task_type.id_prefix();
    let suffix: String = (0..8)
        .map(|_| {
            let idx = rand::random_range(0..TASK_ID_ALPHABET.len());
            TASK_ID_ALPHABET[idx] as char
        })
        .collect();
    TaskId(format!("{prefix}{suffix}"))
}

// ---------------------------------------------------------------------------
// TaskState — per-task runtime state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskState {
    pub id: TaskId,
    pub task_type: TaskType,
    pub status: TaskStatus,
    pub description: String,
    pub tool_use_id: Option<String>,
    pub start_time_epoch_ms: u64,
    pub end_time_epoch_ms: Option<u64>,
    pub total_paused_ms: Option<u64>,
    pub output_file: PathBuf,
    pub output_offset: u64,
    pub notified: bool,
}

impl TaskState {
    pub fn new(
        id: TaskId,
        task_type: TaskType,
        description: String,
        tool_use_id: Option<String>,
    ) -> Self {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let output_file = get_task_output_path(&id);
        Self {
            id,
            task_type,
            status: TaskStatus::Pending,
            description,
            tool_use_id,
            start_time_epoch_ms: now_ms,
            end_time_epoch_ms: None,
            total_paused_ms: None,
            output_file,
            output_offset: 0,
            notified: false,
        }
    }

    pub fn mark_running(&mut self) {
        self.status = TaskStatus::Running;
    }

    pub fn mark_completed(&mut self) {
        self.status = TaskStatus::Completed;
        self.end_time_epoch_ms = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );
    }

    pub fn mark_failed(&mut self) {
        self.status = TaskStatus::Failed;
        self.end_time_epoch_ms = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );
    }

    pub fn mark_killed(&mut self) {
        self.status = TaskStatus::Killed;
        self.end_time_epoch_ms = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );
    }
}

fn get_task_output_path(id: &TaskId) -> PathBuf {
    let base = data_local_dir().join("sen").join("tasks");
    base.join(format!("{}.output", id.0))
}

/// Cross-platform local data directory without the `dirs` crate.
fn data_local_dir() -> PathBuf {
    #[cfg(windows)]
    {
        std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var("HOME")
            .map(|h| PathBuf::from(h).join("Library").join("Application Support"))
            .unwrap_or_else(|_| PathBuf::from("."))
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|_| {
                std::env::var("HOME").map(|h| PathBuf::from(h).join(".local").join("share"))
            })
            .unwrap_or_else(|_| PathBuf::from("."))
    }
}

// ---------------------------------------------------------------------------
// TaskHandle — returned from spawn, allows cancellation
// ---------------------------------------------------------------------------

pub struct TaskHandle {
    pub task_id: TaskId,
    pub cancel_tx: Option<watch::Sender<bool>>,
    pub cleanup: Option<Box<dyn FnOnce() + Send>>,
}

impl TaskHandle {
    pub fn cancel(&self) {
        if let Some(tx) = &self.cancel_tx {
            let _ = tx.send(true);
        }
    }
}

// ---------------------------------------------------------------------------
// TaskContext — injected into task implementations
// ---------------------------------------------------------------------------

pub struct TaskContext {
    pub abort_signal: watch::Receiver<bool>,
    pub task_id: TaskId,
    pub cwd: PathBuf,
}

// ---------------------------------------------------------------------------
// Task trait — the polymorphic interface for task implementations
// ---------------------------------------------------------------------------

/// Each task type implements this trait.
#[async_trait::async_trait]
pub trait Task: Send + Sync {
    fn name(&self) -> &str;
    fn task_type(&self) -> TaskType;
    async fn kill(&self, task_id: &TaskId) -> anyhow::Result<()>;
}
