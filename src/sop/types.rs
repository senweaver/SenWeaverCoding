// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SopPriority {
    Low,
    #[default]
    Normal,
    High,
    Critical,
}

impl fmt::Display for SopPriority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::Normal => write!(f, "normal"),
            Self::High => write!(f, "high"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SopExecutionMode {

    Auto,

    #[default]
    Supervised,

    StepByStep,

    PriorityBased,

    Deterministic,
}

impl fmt::Display for SopExecutionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => write!(f, "auto"),
            Self::Supervised => write!(f, "supervised"),
            Self::StepByStep => write!(f, "step_by_step"),
            Self::PriorityBased => write!(f, "priority_based"),
            Self::Deterministic => write!(f, "deterministic"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SopTrigger {
    Mqtt {
        topic: String,
        #[serde(default)]
        condition: Option<String>,
    },
    Webhook {
        path: String,
    },
    Cron {
        expression: String,
    },
    Peripheral {
        board: String,
        signal: String,
        #[serde(default)]
        condition: Option<String>,
    },
    Manual,
}

impl fmt::Display for SopTrigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mqtt { topic, .. } => write!(f, "mqtt:{topic}"),
            Self::Webhook { path } => write!(f, "webhook:{path}"),
            Self::Cron { expression } => write!(f, "cron:{expression}"),
            Self::Peripheral { board, signal, .. } => write!(f, "peripheral:{board}/{signal}"),
            Self::Manual => write!(f, "manual"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SopStepKind {

    #[default]
    Execute,

    Checkpoint,
}

impl fmt::Display for SopStepKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Execute => write!(f, "execute"),
            Self::Checkpoint => write!(f, "checkpoint"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepSchema {

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SopStep {
    pub number: u32,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub suggested_tools: Vec<String>,
    #[serde(default)]
    pub requires_confirmation: bool,

    #[serde(default)]
    pub kind: SopStepKind,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<StepSchema>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sop {
    pub name: String,
    pub description: String,
    pub version: String,
    pub priority: SopPriority,
    pub execution_mode: SopExecutionMode,
    pub triggers: Vec<SopTrigger>,
    pub steps: Vec<SopStep>,
    #[serde(default = "default_cooldown_secs")]
    pub cooldown_secs: u64,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: u32,
    #[serde(skip)]
    pub location: Option<PathBuf>,

    #[serde(default)]
    pub deterministic: bool,
}

fn default_cooldown_secs() -> u64 {
    0
}

fn default_max_concurrent() -> u32 {
    1
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SopManifest {
    pub sop: SopMeta,
    #[serde(default)]
    pub triggers: Vec<SopTrigger>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SopMeta {
    pub name: String,
    pub description: String,
    #[serde(default = "default_sop_version")]
    pub version: String,
    #[serde(default)]
    pub priority: SopPriority,
    #[serde(default)]
    pub execution_mode: Option<SopExecutionMode>,
    #[serde(default = "default_cooldown_secs")]
    pub cooldown_secs: u64,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: u32,

    #[serde(default)]
    pub deterministic: bool,
}

fn default_sop_version() -> String {
    "0.1.0".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SopTriggerSource {
    Mqtt,
    Webhook,
    Cron,
    Peripheral,
    Manual,
}

impl fmt::Display for SopTriggerSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mqtt => write!(f, "mqtt"),
            Self::Webhook => write!(f, "webhook"),
            Self::Cron => write!(f, "cron"),
            Self::Peripheral => write!(f, "peripheral"),
            Self::Manual => write!(f, "manual"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SopEvent {
    pub source: SopTriggerSource,

    #[serde(default)]
    pub topic: Option<String>,

    #[serde(default)]
    pub payload: Option<String>,

    pub timestamp: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SopRunStatus {
    Pending,
    Running,
    WaitingApproval,

    PausedCheckpoint,
    Completed,
    Failed,
    Cancelled,
}

impl fmt::Display for SopRunStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Running => write!(f, "running"),
            Self::WaitingApproval => write!(f, "waiting_approval"),
            Self::PausedCheckpoint => write!(f, "paused_checkpoint"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SopStepStatus {
    Completed,
    Failed,
    Skipped,
}

impl fmt::Display for SopStepStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::Skipped => write!(f, "skipped"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SopStepResult {
    pub step_number: u32,
    pub status: SopStepStatus,
    pub output: String,
    pub started_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SopRun {
    pub run_id: String,
    pub sop_name: String,
    pub trigger_event: SopEvent,
    pub status: SopRunStatus,
    pub current_step: u32,
    pub total_steps: u32,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub step_results: Vec<SopStepResult>,

    #[serde(default)]
    pub waiting_since: Option<String>,

    #[serde(default)]
    pub llm_calls_saved: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeterministicRunState {

    pub run_id: String,

    pub sop_name: String,

    pub last_completed_step: u32,

    pub total_steps: u32,

    pub step_outputs: HashMap<u32, serde_json::Value>,

    pub persisted_at: String,

    pub llm_calls_saved: u64,

    pub paused_at_checkpoint: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeterministicSavings {

    pub total_llm_calls_saved: u64,

    pub total_runs: u64,
}

#[derive(Debug, Clone)]
pub enum SopRunAction {

    ExecuteStep {
        run_id: String,
        step: SopStep,
        context: String,
    },

    WaitApproval {
        run_id: String,
        step: SopStep,
        context: String,
    },

    DeterministicStep {
        run_id: String,
        step: SopStep,
        input: serde_json::Value,
    },

    CheckpointWait {
        run_id: String,
        step: SopStep,
        state_file: PathBuf,
    },

    Completed { run_id: String, sop_name: String },

    Failed {
        run_id: String,
        sop_name: String,
        reason: String,
    },
}
