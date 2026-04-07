// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Workflow Engine - Multi-step agent orchestration.
//!
//! This module provides a workflow engine.
//! It allows defining and executing multi-step processes with support for:
//!
//! - **Sequential execution** - Run steps one after another
//! - **FanOut/Collect** - Run multiple steps in parallel, then combine results
//! - **Conditional branching** - Execute steps based on conditions
//! - **Loop iteration**  - Repeat steps until a condition is met
//! - **Error handling**  - Fail, skip, or retry on errors
//!
//! # Example
//!
//! ```rust,no_run
//! use senweavercoding::workflows::{
//!     Workflow, WorkflowStep, StepMode, WorkflowEngine, WorkflowRun,
//! };
//!
//! // Define a workflow
//! let workflow = Workflow::new("Data Analysis")
//!     .with_description("Analyze and summarize data")
//!     .add_step(
//!         WorkflowStep::new("extract", "Extract key points from {{input}}")
//!             .with_output_var("extracted"),
//!     )
//!     .add_step(
//!         WorkflowStep::new("summarize", "Summarize: {{extracted}}")
//!             .with_output_var("summary"),
//!     );
//!
//! // Execute the workflow
//! let run = WorkflowRun::new(workflow.id.clone(), "input data");
//! let engine = WorkflowEngine::new();
//! // ... provide executor and resolver
//! ```

pub mod executor;
pub mod types;

// Re-export main types
#[allow(unused_imports)]
pub use executor::{WorkflowEngine, mock_step_executor};
#[allow(unused_imports)]
pub use types::{
    ErrorMode, StartWorkflowRequest, StartWorkflowResponse, StepAgent, StepMode, StepResult,
    Workflow, WorkflowId, WorkflowRun, WorkflowRunId, WorkflowRunStatus, WorkflowStep,
    WorkflowValidationError, validate_workflow,
};
