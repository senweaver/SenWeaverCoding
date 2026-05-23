// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Step {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub inputs: HashMap<String, String>,
}

impl Step {
    pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            inputs: HashMap::new(),
        }
    }

    pub fn with_input(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.inputs.insert(key.into(), value.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Artifact {
    pub step_id: String,
    pub content: String,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

impl Artifact {
    pub fn new(step_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            step_id: step_id.into(),
            content: content.into(),
            language: None,
            metadata: HashMap::new(),
        }
    }

    pub fn with_language(mut self, lang: impl Into<String>) -> Self {
        self.language = Some(lang.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VerificationVerdict {

    Pass,

    Fail { reason: String, retryable: bool },
}

#[derive(Debug, Default, Clone)]
pub struct FlowContext {
    pub goal: String,
    pub transcript: Vec<TranscriptEntry>,
    pub scratchpad: HashMap<String, String>,
}

impl FlowContext {
    pub fn new(goal: impl Into<String>) -> Self {
        Self {
            goal: goal.into(),
            ..Default::default()
        }
    }

    pub fn push(&mut self, entry: TranscriptEntry) {
        self.transcript.push(entry);
    }
}

#[derive(Debug, Clone)]
pub enum TranscriptEntry {
    Plan {
        steps: Vec<Step>,
    },
    Exec {
        step_id: String,
        artifact: Artifact,
    },
    Verify {
        step_id: String,
        verdict: VerificationVerdict,
    },
    Fix {
        step_id: String,
        attempt: u32,
        message: String,
    },
}

#[async_trait]
pub trait AgentHandle: Send + Sync {

    async fn complete(&self, prompt: &str) -> Result<String, FlowError>;
}

#[async_trait]
pub trait Planner: Send + Sync {
    async fn plan(
        &self,
        ctx: &mut FlowContext,
        agent: &dyn AgentHandle,
    ) -> Result<Vec<Step>, FlowError>;
}

#[async_trait]
pub trait Executor: Send + Sync {
    async fn execute(
        &self,
        ctx: &mut FlowContext,
        agent: &dyn AgentHandle,
        step: &Step,
    ) -> Result<ExecOutcome, FlowError>;
}

#[derive(Debug, Clone)]
pub struct ExecOutcome {
    pub artifact: Artifact,
}

impl ExecOutcome {
    pub fn new(artifact: Artifact) -> Self {
        Self { artifact }
    }
}

#[async_trait]
pub trait Verifier: Send + Sync {
    async fn verify(
        &self,
        ctx: &mut FlowContext,
        artifact: &Artifact,
    ) -> Result<VerificationVerdict, FlowError>;
}

#[derive(Debug, Clone)]
pub struct FlowOutcome {
    pub artifacts: Vec<Artifact>,
    pub iterations: u32,
    pub transcript: Vec<TranscriptEntry>,
}

impl FlowOutcome {
    pub fn success(artifacts: Vec<Artifact>, iterations: u32, ctx: FlowContext) -> Self {
        Self {
            artifacts,
            iterations,
            transcript: ctx.transcript,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FlowError {
    #[error("planner error: {0}")]
    Planner(String),
    #[error("executor error: {0}")]
    Executor(String),
    #[error("verifier error: {0}")]
    Verifier(String),
    #[error("fix loop exceeded {0} attempts")]
    FixLoopExhausted(u32),
    #[error("agent handle error: {0}")]
    AgentHandle(String),
    #[error("cancelled")]
    Cancelled,
    #[error("other: {0}")]
    Other(String),
}

#[async_trait]
pub trait Flow: Send + Sync {
    fn name(&self) -> &'static str;
    async fn run(
        &self,
        ctx: &mut FlowContext,
        agent: &dyn AgentHandle,
    ) -> Result<FlowOutcome, FlowError>;
}
