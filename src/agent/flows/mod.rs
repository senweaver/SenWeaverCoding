// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod builtins;
pub mod checkpoint;
pub mod checkpoint_backend;

pub mod code_edit_plan;
pub mod plan_exec_verify;
pub mod registry;
pub mod traits;

pub use checkpoint::{Checkpoint, CheckpointStore};
pub use checkpoint_backend::{
    CheckpointBackend, CheckpointBackendError, CheckpointMeta, PersistentCheckpointBackend,
};
pub use code_edit_plan::{
    CODE_EDIT_PLANNER_PROMPT_V2, CODE_EDIT_PLANNER_RETRY_PROMPT, PLANNER_JSON_SCHEMA,
    PlanDependencyGraph, PlanParseError, PlanStepJson, PlanStepKind, PlannerResponse, RiskLevel,
    auto_expand_with_symbol_graph, degraded_catch_all_step, render_planner_prompt,
    render_planner_retry_prompt, step_from_plan, validate_planner_response,
};
pub use plan_exec_verify::{LayeredPlan, PlanExecVerifyFlow, PlanExecVerifyOptions};
pub use registry::{global_agent_handle, global_checkpoint_store, set_global_agent_handle};
pub use traits::{
    AgentHandle, Artifact, ExecOutcome, Executor, Flow, FlowContext, FlowError, FlowOutcome,
    Planner, Step, TranscriptEntry, VerificationVerdict, Verifier as FlowVerifier,
};
