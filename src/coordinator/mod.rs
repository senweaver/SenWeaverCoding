// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod delegation;

pub use delegation::{
    DelegationPlan, FailureSummary, MergeStrategy, MergedOutput, SubTask, SubTaskResult,
    build_judge_prompt, merge_results, merge_results_structured, merge_results_with_judge,
    merge_results_with_judge_structured,
};

pub use crate::agent::coordination::{
    AcquireOpts, BarrierError, BarrierManager, BarrierResult, BufferLock, Coordinator,
    CoordinatorHandle, LockError, LockManager, LockResult, RegionLockToken, RegionLockTokens,
    RegionRequest, Vote, VotingManager, VotingResult,
};

use std::collections::HashSet;

const COORDINATOR_ALLOWED_TOOLS: &[&str] = &[
    "delegate",
    // The coordinator's whole job is orchestration, so it must be able to reach
    // the two strongest parallel primitives, not just single delegate.
    "delegate_parallel",
    "spawn_workers",
    "send_message",
    "team_create",
    "team_delete",
    "todo_write",
    "enter_plan_mode",
    "exit_plan_mode",
    "task_create",
    "task_get",
    "task_update",
    "task_list",
    "task_output",
    "task_stop",
    "file_read",
    "glob_search",
    "content_search",
    "memory_store",
    "memory_recall",
    "send_user_message",
    "sleep",
    "lsp",
];

pub fn is_coordinator_tool(tool_name: &str) -> bool {
    COORDINATOR_ALLOWED_TOOLS.contains(&tool_name)
}

pub fn coordinator_tool_set() -> HashSet<String> {
    COORDINATOR_ALLOWED_TOOLS
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

pub fn filter_for_coordinator(tool_names: &[&str]) -> Vec<String> {
    tool_names
        .iter()
        .filter(|name| is_coordinator_tool(name))
        .map(|s| (*s).to_string())
        .collect()
}
