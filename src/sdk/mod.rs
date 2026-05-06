// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Stable SDK surface.
//!
//! This module is the **only** supported entry point for external
//! code that embeds `senweavercoding` as a library.  Everything
//! re-exported here is guaranteed semver-stable for the current
//! major version.  Items not listed here — including the top-level
//! `commands`, `cli`, `main`, and internal framework modules — are
//! implementation details and may change without notice.
//!
//! # Categories
//!
//! - **Agent orchestration**: [`Agent`], [`AgentBuilder`],
//!   [`AgentConfig`], [`TurnEvent`], [`MultiAgentRuntime`], and the
//!   multi-agent primitives ([`Coordinator`], [`AgentRegistry`],
//!   [`Supervisor`], [`TaskQueue`], [`Blackboard`]).
//! - **Providers**: [`Provider`] trait + [`ChatMessage`], [`ChatResponse`]
//!   and the factory [`create_provider`].
//! - **Tools**: [`Tool`] trait + [`ToolResult`] + [`ToolSpec`] for
//!   third-party tool authors.
//! - **Memory**: [`Memory`] trait + [`MemoryEntry`] + [`MemoryCategory`].
//! - **Config**: [`Config`] + high-level loaders.
//! - **Observability**: [`Observer`] trait + lifecycle [`ObserverEvent`]
//!   and [`ObserverMetric`] enums.
//! - **SDK entrypoint**: [`SdkEntrypoint`] for higher-level
//!   "run a session from JSON config" use cases.
//! - **Runtime**: [`spawn_supervised`] for SDK-level background tasks
//!   that want panic capture and registry tracking.
//!
//! # Non-goals
//!
//! The SDK does **not** re-export CLI command handlers, internal
//! module structures (schema, context, prompt), or the gateway HTTP
//! layer.  Those remain available via the `senweavercoding::...`
//! paths but carry no stability guarantee.

pub use crate::agent::coordination::{Coordinator, CoordinatorHandle};
pub use crate::agent::multi_agent_runtime::{
    MultiAgentRuntime, RuntimeHealthSummary, global_runtime, init_global_runtime,
};
pub use crate::agent::registry::{AgentRegistry, AgentRegistryHandle};
pub use crate::agent::supervisor::{Supervisor, SupervisorHandle};
pub use crate::agent::task_queue::{TaskQueue, TaskQueueHandle};
pub use crate::agent::{Agent, AgentBuilder, TurnEvent};

pub use crate::coordinator::{
    DelegationPlan, MergeStrategy, SubTask, SubTaskResult, merge_results, merge_results_with_judge,
};

pub use crate::providers::traits::{
    ChatMessage, ChatRequest, ChatResponse, Provider, ProviderCapabilities, StreamEvent,
    StreamOptions, ToolCall,
};

pub use crate::tools::traits::{Tool, ToolResult, ToolSpec};

pub use crate::memory::blackboard::{Blackboard, BlackboardHandle};
pub use crate::memory::traits::{Memory, MemoryCategory, MemoryEntry};

pub use crate::config::Config;

pub use crate::observability::traits::{Observer, ObserverEvent, ObserverMetric};

pub use crate::entrypoints::{
    HookEvent, PermissionMode, SdkConfig, SdkEntrypoint, SdkHookCallback, SdkMcpServer, SdkMessage,
    SdkModelUsage, SdkSession, SdkStatus, SdkToolCall, SdkToolCallBuilder, SdkTurnEvent,
    SdkTurnResult,
};

pub use crate::runtime::{TaskHandle, spawn_supervised};
