// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Task management module — mirrors claude-code's `tasks/` and `Task.ts`.
//
// Provides typed task abstractions (local shell, local agent, remote agent,
// in-process teammate, workflow, monitor, dream) with lifecycle management,
// output capture, and status tracking.

pub mod dream;
pub mod local_agent;
pub mod local_shell;
pub mod remote_agent;
pub mod runner;
pub mod teammate;
pub mod types;

pub use runner::TaskRunner;
pub use types::{
    Task, TaskContext, TaskHandle, TaskId, TaskState, TaskStatus, TaskType, generate_task_id,
    is_terminal_status,
};
