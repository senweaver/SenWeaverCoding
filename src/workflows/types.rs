// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type WorkflowId = String;

pub type WorkflowRunId = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Workflow {

    pub id: WorkflowId,

    pub name: String,

    pub description: String,

    pub steps: Vec<WorkflowStep>,

    #[serde(default = "default_workflow_timeout")]
    pub timeout_secs: u64,

    pub created_at: DateTime<Utc>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub variables: HashMap<String, String>,
}

fn default_workflow_timeout() -> u64 {
    3600
}

impl Workflow {

    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: format!(
                "wf-{}-{}",
                now.timestamp_millis(),
                &uuid::Uuid::new_v4().to_string()[..8]
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

    pub fn add_step(mut self, step: WorkflowStep) -> Self {
        self.steps.push(step);
        self
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    pub fn with_variable(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.variables.insert(key.into(), value.into());
        self
    }

    pub fn step_count(&self) -> usize {
        self.steps.len()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowStep {

    pub name: String,

    pub agent: StepAgent,

    pub prompt_template: String,

    #[serde(default)]
    pub mode: StepMode,

    #[serde(default = "default_step_timeout")]
    pub timeout_secs: u64,

    #[serde(default)]
    pub error_mode: ErrorMode,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_var: Option<String>,
}

fn default_step_timeout() -> u64 {
    120
}

impl WorkflowStep {

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

    pub fn with_agent(mut self, agent: StepAgent) -> Self {
        self.agent = agent;
        self
    }

    pub fn with_mode(mut self, mode: StepMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_error_mode(mut self, mode: ErrorMode) -> Self {
        self.error_mode = mode;
        self
    }

    pub fn with_output_var(mut self, var: impl Into<String>) -> Self {
        self.output_var = Some(var.into());
        self
    }

    pub fn expand_prompt(&self, variables: &HashMap<String, String>, input: &str) -> String {
        let mut result = self.prompt_template.clone();

        result = result.replace("{{input}}", input);

        for (key, value) in variables {
            result = result.replace(&format!("{{{{{}}}}}", key), value);
        }

        result
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepAgent {

    Default,

    ById { id: String },

    ByName { name: String },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepMode {

    #[default]
    Sequential,

    FanOut,

    Collect,

    Conditional { condition: String },

    Loop {

        max_iterations: u32,

        until: String,
    },
}

impl StepMode {

    pub fn is_parallel(&self) -> bool {
        matches!(self, StepMode::FanOut)
    }

    pub fn is_special(&self) -> bool {
        !matches!(self, StepMode::Sequential)
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorMode {

    #[default]
    Fail,

    Skip,

    Retry {

        max_retries: u32,
    },
}

impl ErrorMode {

    pub fn allows_continue(&self) -> bool {
        matches!(self, ErrorMode::Skip | ErrorMode::Retry { .. })
    }

    pub fn max_retries(&self) -> u32 {
        match self {
            ErrorMode::Retry { max_retries } => *max_retries,
            _ => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkflowRunStatus {

    Pending,

    Running,

    Completed,

    Failed,

    Cancelled,
}

impl std::fmt::Display for WorkflowRunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowRun {

    pub id: WorkflowRunId,

    pub workflow_id: WorkflowId,

    pub status: WorkflowRunStatus,

    pub input: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,

    pub current_step: usize,

    pub step_results: Vec<StepResult>,

    pub variables: HashMap<String, String>,

    pub created_at: DateTime<Utc>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl WorkflowRun {

    pub fn new(workflow_id: WorkflowId, input: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: format!(
                "run-{}-{}",
                now.timestamp_millis(),
                &uuid::Uuid::new_v4().to_string()[..8]
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

    pub fn mark_started(&mut self) {
        self.status = WorkflowRunStatus::Running;
        self.started_at = Some(Utc::now());
    }

    pub fn mark_completed(&mut self, output: impl Into<String>) {
        self.status = WorkflowRunStatus::Completed;
        self.output = Some(output.into());
        self.completed_at = Some(Utc::now());
    }

    pub fn mark_failed(&mut self, error: impl Into<String>) {
        self.status = WorkflowRunStatus::Failed;
        self.error = Some(error.into());
        self.completed_at = Some(Utc::now());
    }

    pub fn mark_cancelled(&mut self) {
        self.status = WorkflowRunStatus::Cancelled;
        self.completed_at = Some(Utc::now());
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            WorkflowRunStatus::Completed | WorkflowRunStatus::Failed | WorkflowRunStatus::Cancelled
        )
    }

    pub fn add_step_result(&mut self, result: StepResult) {
        self.step_results.push(result);
        self.current_step += 1;
    }

    pub fn set_variable(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.variables.insert(key.into(), value.into());
    }

    pub fn duration_secs(&self) -> u64 {
        let end = self.completed_at.unwrap_or_else(Utc::now);
        let start = self.started_at.unwrap_or(self.created_at);
        (end - start).num_seconds().max(0) as u64
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StepResult {

    pub step_name: String,

    pub step_index: usize,

    pub success: bool,

    pub output: String,

    pub token_usage: Option<(u64, u64)>,

    pub duration_ms: u64,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl StepResult {

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

    pub fn with_tokens(mut self, input: u64, output: u64) -> Self {
        self.token_usage = Some((input, output));
        self
    }

    pub fn with_duration(mut self, ms: u64) -> Self {
        self.duration_ms = ms;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StartWorkflowRequest {

    pub workflow_id: WorkflowId,

    pub input: String,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub variables: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StartWorkflowResponse {

    pub run: WorkflowRun,

    pub completed_synchronously: bool,
}

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
