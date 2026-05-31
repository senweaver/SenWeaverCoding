// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub use crate::agent::coordination::{Coordinator, CoordinatorHandle};
pub use crate::agent::multi_agent_runtime::{
    MultiAgentRuntime, RuntimeHealthSummary, global_runtime, init_global_runtime,
};
pub use crate::agent::registry::{AgentRegistry, AgentRegistryHandle};
pub use crate::agent::supervisor::{Supervisor, SupervisorHandle};
pub use crate::agent::task_orchestrator::queue::{TaskQueue, TaskQueueHandle};
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
