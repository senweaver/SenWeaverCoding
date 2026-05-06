// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Typed error hierarchy for the SenWeaverCoding public API.
//!
//! Internal code may continue to use `anyhow::Result` for convenience,
//! but all `pub` functions on SDK-facing types should return
//! `Result<T, SenError>` so downstream consumers can match on variants.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SenError {
    #[error("agent error: {0}")]
    Agent(#[from] AgentError),

    #[error("scheduler error: {0}")]
    Scheduler(#[from] SchedulerError),

    #[error("coordinator error: {0}")]
    Coordinator(#[from] CoordinatorError),

    #[error("blackboard error: {0}")]
    Blackboard(#[from] BlackboardError),

    #[error("event bus error: {0}")]
    EventBus(#[from] EventBusError),

    #[error("supervisor error: {0}")]
    Supervisor(#[from] SupervisorError),

    #[error("registry error: {0}")]
    Registry(#[from] RegistryError),

    #[error("task queue error: {0}")]
    TaskQueue(#[from] TaskQueueError),

    #[error("config error: {0}")]
    Config(String),

    #[error("provider error: {0}")]
    Provider(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Error)]
pub enum AgentError {

    #[error("agent exceeded maximum tool iterations ({0})")]
    LoopOverflow(usize),

    #[error("model switch failed: {0}")]
    ModelSwitchFailed(String),

    #[error("turn was cancelled")]
    TurnCancelled,

    #[error("tool dispatch failed: {0}")]
    ToolDispatchFailed(String),

    #[error("stream interrupted: {0}")]
    StreamInterrupted(String),

    #[error("context budget exceeded: {0}")]
    ContextBudgetExceeded(String),

    #[error("cost budget exceeded: {0}")]
    CostBudgetExceeded(String),

    #[error("loop aborted by detector: {0}")]
    LoopAborted(String),

    #[error("provider error: {0}")]
    Provider(String),

    #[error("tool `{tool_name}` failed: {cause}")]
    Tool {
        tool_name: String,
        #[source]
        cause: crate::tools::ToolErrorCause,
    },
}

impl AgentError {

    pub fn tool_failed(tool_name: impl Into<String>, cause: crate::tools::ToolErrorCause) -> Self {
        AgentError::Tool {
            tool_name: tool_name.into(),
            cause,
        }
    }

    pub fn tool_name(&self) -> Option<&str> {
        match self {
            AgentError::Tool { tool_name, .. } => Some(tool_name.as_str()),
            _ => None,
        }
    }

    pub fn cause(&self) -> Option<&crate::tools::ToolErrorCause> {
        match self {
            AgentError::Tool { cause, .. } => Some(cause),
            _ => None,
        }
    }
}

impl From<anyhow::Error> for AgentError {

    fn from(e: anyhow::Error) -> Self {
        if let Some(io_err) = e.downcast_ref::<std::io::Error>() {
            return AgentError::Tool {
                tool_name: "<unknown>".into(),
                cause: crate::tools::ToolErrorCause::Io(std::io::Error::new(
                    io_err.kind(),
                    io_err.to_string(),
                )),
            };
        }
        if e.downcast_ref::<tokio::time::error::Elapsed>().is_some() {
            return AgentError::Tool {
                tool_name: "<unknown>".into(),
                cause: crate::tools::ToolErrorCause::Timeout(std::time::Duration::from_secs(0)),
            };
        }
        AgentError::ToolDispatchFailed(e.to_string())
    }
}

impl From<String> for AgentError {
    fn from(s: String) -> Self {
        AgentError::ToolDispatchFailed(s)
    }
}

impl From<&str> for AgentError {
    fn from(s: &str) -> Self {
        AgentError::ToolDispatchFailed(s.to_string())
    }
}

#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("dependency cycle detected in task graph")]
    CycleDetected,

    #[error("unknown dependency: task '{task}' depends on '{dependency}'")]
    UnknownDependency { task: String, dependency: String },

    #[error("task '{0}' not found")]
    TaskNotFound(String),

    #[error("scheduler cancelled")]
    Cancelled,
}

#[derive(Debug, Error)]
pub enum CoordinatorError {
    #[error("lock contention on resource '{resource}' by agent '{agent}'")]
    LockContention { resource: String, agent: String },

    #[error("barrier '{0}' timed out")]
    BarrierTimeout(String),

    #[error("voting session '{0}' expired")]
    VotingExpired(String),

    #[error("agent '{0}' not registered")]
    AgentNotFound(String),
}

#[derive(Debug, Error)]
pub enum BlackboardError {
    #[error("key '{0}' not found")]
    KeyNotFound(String),

    #[error("write conflict on key '{0}' (version mismatch)")]
    VersionConflict(String),

    #[error("entry expired")]
    Expired,
}

#[derive(Debug, Error)]
pub enum EventBusError {
    #[error("channel closed")]
    ChannelClosed,

    #[error("subscriber lagged behind by {0} events")]
    Lagged(u64),
}

#[derive(Debug, Error)]
pub enum SupervisorError {
    #[error("maximum agent limit ({0}) reached")]
    MaxAgentsLimit(usize),

    #[error("capability '{0}' agent limit ({1}) reached")]
    CapabilityLimit(String, usize),

    #[error("agent '{0}' already registered")]
    AlreadyRegistered(String),

    #[error("agent '{0}' not found")]
    AgentNotFound(String),
}

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("agent '{0}' already registered")]
    AlreadyRegistered(String),

    #[error("agent '{0}' not found")]
    AgentNotFound(String),

    #[error("agent '{0}' is not available for task assignment (state: {1})")]
    AgentNotAvailable(String, String),

    #[error("agent '{agent_id}' not in expected state: expected {expected}, found {found}")]
    StateMismatch {
        agent_id: String,
        expected: String,
        found: String,
    },
}

impl From<String> for RegistryError {
    fn from(s: String) -> Self {

        if s.contains("not found") || s.contains("not_registered") {
            RegistryError::AgentNotFound(s)
        } else if s.contains("already registered") {
            RegistryError::AlreadyRegistered(s)
        } else {
            RegistryError::AgentNotFound(s)
        }
    }
}

#[derive(Debug, Error)]
pub enum TaskQueueError {
    #[error("task '{0}' not found")]
    TaskNotFound(String),

    #[error("task '{task_id}' not in expected status: expected {expected}, found {found}")]
    StatusMismatch {
        task_id: String,
        expected: String,
        found: String,
    },

    #[error("task '{0}' not in running state")]
    NotRunning(String),

    #[error("queue capacity exceeded")]
    CapacityExceeded,
}
