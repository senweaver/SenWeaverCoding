// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod executor;
pub mod types;
pub use executor::{WorkflowEngine, mock_step_executor};
pub use types::{
    ErrorMode, StartWorkflowRequest, StartWorkflowResponse, StepAgent, StepMode, StepResult,
    Workflow, WorkflowId, WorkflowRun, WorkflowRunId, WorkflowRunStatus, WorkflowStep,
    WorkflowValidationError, validate_workflow,
};
