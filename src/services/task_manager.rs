// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Re-export the tool-level TaskManager from its canonical location.
//
// The agent has two complementary task management systems:
//
// 1. **TaskManager** (`crate::tools::task_types`) — lightweight, synchronous,
//    in-memory CRUD store exposed to the LLM through the `task_*` tool family
//    (task_create, task_get, task_update, task_list, task_output, task_stop).
//    Uses simple string IDs ("task-1", "task-2") and a basic state enum
//    (Pending/Running/Completed/Failed/Stopped). Suitable for the agent to
//    track its own logical sub-tasks during a conversation.
//
// 2. **TaskRunner** (`crate::tasks::runner`) — async, `tokio::sync::RwLock`-based
//    lifecycle manager for actual background processes (shell commands, sub-agents,
//    workflows). Uses typed `TaskId` with random suffixes, rich `TaskState`
//    (timing, output files, pause tracking), and is wired into `ServiceContainer`.
//
// This module re-exports #1 so that `crate::services::task_manager::*` remains
// a valid import path (used by tool tests).

#[allow(unused_imports)]
pub use crate::tools::task_types::*;
