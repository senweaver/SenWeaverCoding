// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Workflow Types - Multi-step agent orchestration.
//!
//! Workflows define sequences of steps that can run agents, with support for:
//! - Sequential execution
//! - Fan-out parallel execution
//! - Conditional branching
//! - Loop iteration
//! - Error handling with retry

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Unique workflow identifier.
pub type WorkflowId = String;

/// Unique workflow run identifier.
pub type WorkflowRunId = String;

/// A workflow definition  - reusable multi-step process.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Workflow {
    /// Unique workflow ID.
    pub id: WorkflowId,
    /// Human-readable name.
    pub name: String,
    /// Description of what the workflow does.
    pub description: String,
    /// Steps in the workflow.
    pub steps: Vec<WorkflowStep>,
    /// Default timeout for workflow execution (seconds).
    #[serde(default = "default_workflow_timeout")]
    pub timeout_secs: u64,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Tags for categorization.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Workflow variables (default values).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub variables: HashMap<String, String>,
}

fn default_workflow_timeout() -> u64 {
    3600 // 1 hour default
}

impl Workflow {
    /// Create a new workflow with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: format!(
                "wf-{}-{}",
                now.timestamp_millis(),
                uuid::Uuid::new_v4().to_string()[..8].to_string()
            ),
            name: name.into(),
            description: String::new(),
            steps: Vec::new(),
            timeout_secs: default_workflow_timeout(),
            created_at: now,
            tags: Vec::new(),
            variables: HashMap::new(),
        }
    }

    /// Add a step to the workflow.
    pub fn add_step(mut self, step: WorkflowStep) -> Self {
        self.steps.push(step);
        self
    }

    /// Set the description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Set the timeout.
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Set default variables.
    pub fn with_variable(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.variables.insert(key.into(), value.into());
        self
    }

    /// Count of steps in the workflow.
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }
}

/// A single step in a workflow.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowStep {
    /// Step name (for identification).
    pub name: String,
    /// Which agent to use for this step.
    pub agent: StepAgent,
    /// Prompt template with variable substitution.
    pub prompt_template: String,
    /// Execution mode for this step.
    #[serde(default)]
    pub mode: StepMode,
    /// Timeout for this step (seconds).
    #[serde(default = "default_step_timeout")]
    pub timeout_secs: u64,
    /// Error handling mode.
    #[serde(default)]
    pub error_mode: ErrorMode,
    /// Output variable name (for storing result).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_var: Option<String>,
}

fn default_step_timeout() -> u64 {
    120 // 2 minutes default
}

impl WorkflowStep {
    /// Create a new step with the given name and prompt.
    pub fn new(name: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            agent: StepAgent::Default,
            prompt_template: prompt.into(),
            mode: StepMode::Sequential,
            timeout_secs: default_step_timeout(),
            error_mode: ErrorMode::Fail,
            output_var: None,
        }
    }

    /// Set the agent for this step.
    pub fn with_agent(mut self, agent: StepAgent) -> Self {
        self.agent = agent;
        self
    }

    /// Set the execution mode.
    pub fn with_mode(mut self, mode: StepMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set the error mode.
    pub fn with_error_mode(mut self, mode: ErrorMode) -> Self {
        self.error_mode = mode;
        self
    }

    /// Set the output variable name.
    pub fn with_output_var(mut self, var: impl Into<String>) -> Self {
        self.output_var = Some(var.into());
        self
    }

    /// Expand the prompt template with variables.
    pub fn expand_prompt(&self, variables: &HashMap<String, String>, input: &str) -> String {
        let mut result = self.prompt_template.clone();

        // Replace {{input}} with the step input
        result = result.replace("{{input}}", input);

        // Replace {{var_name}} with variable values
        for (key, value) in variables {
            result = result.replace(&format!("{{{{{}}}}}", key), value);
        }

        result
    }
}

/// Specification of which agent to use for a step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepAgent {
    /// Use the default agent.
    Default,
    /// Use agent by ID.
    ById { id: String },
    /// Use agent by name.
    ByName { name: String },
}

/// Execution mode for a workflow step.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepMode {
    /// Execute sequentially (default).
    #[default]
    Sequential,
    /// Execute in parallel with other FanOut steps.
    FanOut,
    /// Collect results from previous FanOut steps.
    Collect,
    /// Conditional execution based on condition.
    Conditional { condition: String },
    /// Loop execution.
    Loop {
        /// Maximum iterations.
        max_iterations: u32,
        /// Stop when output contains this string.
        until: String,
    },
}

impl StepMode {
    /// Check if this step can run in parallel with others.
    pub fn is_parallel(&self) -> bool {
        matches!(self, StepMode::FanOut)
    }

    /// Check if this step needs special handling (not sequential).
    pub fn is_special(&self) -> bool {
        !matches!(self, StepMode::Sequential)
    }
}

/// Error handling mode for a step.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorMode {
    /// Fail the entire workflow (default).
    #[default]
    Fail,
    /// Skip this step and continue.
    Skip,
    /// Retry with backoff.
    Retry {
        /// Maximum retry attempts.
        max_retries: u32,
    },
}

impl ErrorMode {
    /// Check if this mode allows continuation after error.
    pub fn allows_continue(&self) -> bool {
        matches!(self, ErrorMode::Skip | ErrorMode::Retry { .. })
    }

    /// Get max retries (if applicable).
    pub fn max_retries(&self) -> u32 {
        match self {
            ErrorMode::Retry { max_retries } => *max_retries,
            _ => 0,
        }
    }
}

/// Status of a workflow run.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkflowRunStatus {
    /// Run created but not started.
    Pending,
    /// Run is currently executing.
    Running,
    /// Run completed successfully.
    Completed,
    /// Run failed.
    Failed,
    /// Run was cancelled.
    Cancelled,
}

impl std::fmt::Display for WorkflowRunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// A workflow run  - instance of a workflow execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowRun {
    /// Unique run ID.
    pub id: WorkflowRunId,
    /// Workflow ID being run.
    pub workflow_id: WorkflowId,
    /// Current status.
    pub status: WorkflowRunStatus,
    /// Input to the workflow.
    pub input: String,
    /// Final output (if completed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Current step index.
    pub current_step: usize,
    /// Step results.
    pub step_results: Vec<StepResult>,
    /// Variable values during execution.
    pub variables: HashMap<String, String>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Start timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    /// Completion timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    /// Error message (if failed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl WorkflowRun {
    /// Create a new workflow run.
    pub fn new(workflow_id: WorkflowId, input: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: format!(
                "run-{}-{}",
                now.timestamp_millis(),
                uuid::Uuid::new_v4().to_string()[..8].to_string()
            ),
            workflow_id,
            status: WorkflowRunStatus::Pending,
            input: input.into(),
            output: None,
            current_step: 0,
            step_results: Vec::new(),
            variables: HashMap::new(),
            created_at: now,
            started_at: None,
            completed_at: None,
            error: None,
        }
    }

    /// Mark the run as started.
    pub fn mark_started(&mut self) {
        self.status = WorkflowRunStatus::Running;
        self.started_at = Some(Utc::now());
    }

    /// Mark the run as completed.
    pub fn mark_completed(&mut self, output: impl Into<String>) {
        self.status = WorkflowRunStatus::Completed;
        self.output = Some(output.into());
        self.completed_at = Some(Utc::now());
    }

    /// Mark the run as failed.
    pub fn mark_failed(&mut self, error: impl Into<String>) {
        self.status = WorkflowRunStatus::Failed;
        self.error = Some(error.into());
        self.completed_at = Some(Utc::now());
    }

    /// Mark the run as cancelled.
    pub fn mark_cancelled(&mut self) {
        self.status = WorkflowRunStatus::Cancelled;
        self.completed_at = Some(Utc::now());
    }

    /// Check if the run is in a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            WorkflowRunStatus::Completed | WorkflowRunStatus::Failed | WorkflowRunStatus::Cancelled
        )
    }

    /// Add a step result.
    pub fn add_step_result(&mut self, result: StepResult) {
        self.step_results.push(result);
        self.current_step += 1;
    }

    /// Set a variable.
    pub fn set_variable(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.variables.insert(key.into(), value.into());
    }

    /// Get duration of the run so far (in seconds).
    pub fn duration_secs(&self) -> u64 {
        let end = self.completed_at.unwrap_or_else(Utc::now);
        let start = self.started_at.unwrap_or(self.created_at);
        (end - start).num_seconds().max(0) as u64
    }
}

/// Result of executing a workflow step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StepResult {
    /// Step name.
    pub step_name: String,
    /// Step index in the workflow.
    pub step_index: usize,
    /// Whether the step succeeded.
    pub success: bool,
    /// Step output.
    pub output: String,
    /// Token usage (input tokens, output tokens).
    pub token_usage: Option<(u64, u64)>,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Error message (if failed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl StepResult {
    /// Create a successful step result.
    pub fn success(step_name: impl Into<String>, index: usize, output: impl Into<String>) -> Self {
        Self {
            step_name: step_name.into(),
            step_index: index,
            success: true,
            output: output.into(),
            token_usage: None,
            duration_ms: 0,
            error: None,
        }
    }

    /// Create a failed step result.
    pub fn failure(step_name: impl Into<String>, index: usize, error: impl Into<String>) -> Self {
        Self {
            step_name: step_name.into(),
            step_index: index,
            success: false,
            output: String::new(),
            token_usage: None,
            duration_ms: 0,
            error: Some(error.into()),
        }
    }

    /// Set token usage.
    pub fn with_tokens(mut self, input: u64, output: u64) -> Self {
        self.token_usage = Some((input, output));
        self
    }

    /// Set duration.
    pub fn with_duration(mut self, ms: u64) -> Self {
        self.duration_ms = ms;
        self
    }
}

/// Request to create and start a workflow run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StartWorkflowRequest {
    /// Workflow ID to run.
    pub workflow_id: WorkflowId,
    /// Input to the workflow.
    pub input: String,
    /// Override variables.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub variables: HashMap<String, String>,
}

/// Response from starting a workflow.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StartWorkflowResponse {
    /// The created run.
    pub run: WorkflowRun,
    /// Whether the run was started synchronously (completed immediately).
    pub completed_synchronously: bool,
}

/// Workflow validation error.
#[derive(Debug, thiserror::Error, Clone, PartialEq)]
pub enum WorkflowValidationError {
    #[error("Workflow has no steps")]
    EmptyWorkflow,
    #[error("Step at index {index} has no name")]
    StepMissingName { index: usize },
    #[error("Step '{name}' has empty prompt template")]
    StepEmptyPrompt { name: String },
    #[error("Conditional step '{name}' has empty condition")]
    ConditionalEmptyCondition { name: String },
    #[error("Loop step '{name}' has invalid max_iterations: {value}")]
    InvalidLoopIterations { name: String, value: u32 },
}

/// Validate a workflow definition.
pub fn validate_workflow(workflow: &Workflow) -> Result<(), WorkflowValidationError> {
    if workflow.steps.is_empty() {
        return Err(WorkflowValidationError::EmptyWorkflow);
    }

    for (index, step) in workflow.steps.iter().enumerate() {
        if step.name.is_empty() {
            return Err(WorkflowValidationError::StepMissingName { index });
        }

        if step.prompt_template.is_empty() {
            return Err(WorkflowValidationError::StepEmptyPrompt {
                name: step.name.clone(),
            });
        }

        if let StepMode::Conditional { condition } = &step.mode {
            if condition.is_empty() {
                return Err(WorkflowValidationError::ConditionalEmptyCondition {
                    name: step.name.clone(),
                });
            }
        }

        if let StepMode::Loop { max_iterations, .. } = &step.mode {
            if *max_iterations == 0 {
                return Err(WorkflowValidationError::InvalidLoopIterations {
                    name: step.name.clone(),
                    value: *max_iterations,
                });
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_creation() {
        let workflow = Workflow::new("Test Workflow")
            .with_description("A test workflow")
            .with_timeout(600)
            .with_variable("api_key", "secret123");

        assert_eq!(workflow.name, "Test Workflow");
        assert_eq!(workflow.timeout_secs, 600);
        assert!(workflow.variables.contains_key("api_key"));
    }

    #[test]
    fn test_workflow_step() {
        let step = WorkflowStep::new("step1", "Process {{input}}")
            .with_agent(StepAgent::ByName {
                name: "analyzer".to_string(),
            })
            .with_mode(StepMode::FanOut)
            .with_output_var("result");

        assert_eq!(step.name, "step1");
        assert!(matches!(step.agent, StepAgent::ByName { .. }));
        assert!(matches!(step.mode, StepMode::FanOut));
        assert_eq!(step.output_var, Some("result".to_string()));
    }

    #[test]
    fn test_prompt_expansion() {
        let step = WorkflowStep::new("test", "Process {{input}} with key {{api_key}}");
        let mut vars = HashMap::new();
        vars.insert("api_key".to_string(), "secret123".to_string());

        let expanded = step.expand_prompt(&vars, "data.txt");
        assert_eq!(expanded, "Process data.txt with key secret123");
    }

    #[test]
    fn test_workflow_run_lifecycle() {
        let mut run = WorkflowRun::new("wf-123".to_string(), "input data");
        assert_eq!(run.status, WorkflowRunStatus::Pending);

        run.mark_started();
        assert_eq!(run.status, WorkflowRunStatus::Running);

        run.add_step_result(StepResult::success("step1", 0, "output1"));
        assert_eq!(run.current_step, 1);

        run.mark_completed("final output");
        assert_eq!(run.status, WorkflowRunStatus::Completed);
        assert!(run.is_terminal());
    }

    #[test]
    fn test_step_result_creation() {
        let result = StepResult::success("step1", 0, "output")
            .with_tokens(100, 50)
            .with_duration(1500);

        assert!(result.success);
        assert_eq!(result.token_usage, Some((100, 50)));
        assert_eq!(result.duration_ms, 1500);
    }

    #[test]
    fn test_error_mode() {
        assert!(!ErrorMode::Fail.allows_continue());
        assert!(ErrorMode::Skip.allows_continue());
        assert!(ErrorMode::Retry { max_retries: 3 }.allows_continue());
        assert_eq!(ErrorMode::Retry { max_retries: 3 }.max_retries(), 3);
    }

    #[test]
    fn test_step_mode_parallel() {
        assert!(StepMode::FanOut.is_parallel());
        assert!(!StepMode::Sequential.is_parallel());
        assert!(StepMode::Collect.is_special());
    }

    #[test]
    fn test_validate_workflow_empty() {
        let workflow = Workflow::new("Empty");
        assert!(matches!(
            validate_workflow(&workflow),
            Err(WorkflowValidationError::EmptyWorkflow)
        ));
    }

    #[test]
    fn test_validate_workflow_valid() {
        let workflow =
            Workflow::new("Valid").add_step(WorkflowStep::new("step1", "Process {{input}}"));

        assert!(validate_workflow(&workflow).is_ok());
    }

    #[test]
    fn test_validate_empty_prompt() {
        let workflow = Workflow::new("Invalid").add_step(WorkflowStep::new("step1", ""));

        assert!(matches!(
            validate_workflow(&workflow),
            Err(WorkflowValidationError::StepEmptyPrompt { .. })
        ));
    }

    #[test]
    fn test_workflow_building() {
        let workflow = Workflow::new("Complex Workflow")
            .with_description("Does multiple things")
            .add_step(
                WorkflowStep::new("analyze", "Analyze {{input}}")
                    .with_mode(StepMode::FanOut)
                    .with_output_var("analysis"),
            )
            .add_step(
                WorkflowStep::new("synthesize", "Synthesize {{analysis}}")
                    .with_mode(StepMode::Sequential)
                    .with_error_mode(ErrorMode::Retry { max_retries: 2 }),
            );

        assert_eq!(workflow.step_count(), 2);
    }
}
