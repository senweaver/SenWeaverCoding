// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
#[allow(clippy::module_inception)]
mod sqlite_gateway_hydrate;
pub mod activity;
#[allow(clippy::module_inception)]
pub mod agent;
pub mod auto_title;
pub mod bridge_types;
pub mod builtin_skills;
pub mod classifier;
pub mod cli_runtime;
pub mod coding_mode;
pub mod context;
pub mod debug;
pub mod designer;
pub mod coordination;
pub mod dangling_tool_repair;
pub mod dispatcher;
pub mod error_classify;
pub mod eval;
pub mod executor_core;
pub mod intent;

pub mod flows;
pub mod health_signal;
pub mod history;
pub mod event_sink;
pub mod file_edit_emitter;
pub mod loop_;
pub mod streaming_markers;
pub mod think_extractor;
pub mod memory_loader;
pub mod mode;
pub mod model_switch;
pub mod multi_agent_runtime;
pub mod observe;
pub mod plan_mode;
#[cfg(feature = "tool-curator")]
pub mod curator_mode_enforcement;
pub mod profile;
pub mod prompt;
pub mod recovery;
pub mod registry;
pub mod role_pipeline;
pub mod reward;
pub mod repl_command;
pub mod scheduler;
pub mod self_assess;
pub mod skill_evolution;
pub mod subagent;
pub mod suggestions;
pub mod supervisor;
pub mod task_orchestrator;
pub mod team_protocol;
pub mod thinking;
pub mod token;
pub mod tool_authorizer;
pub mod tool_handler;
pub mod web_search_url_guard;

pub mod turn_engine;
pub mod user;

pub mod verification;
pub mod workflow_loader;

pub use agent::{Agent, AgentBuilder, SubagentChunkKind, TurnEvent};
pub use coordination::{Coordinator, CoordinatorHandle};
pub use loop_::{
    ToolLoopCostTrackingContext, process_message, run, scope_tool_loop_cost_tracking,
};

pub use multi_agent_runtime::{
    MultiAgentRuntime, MultiAgentRuntimeBuilder, MultiAgentRuntimeConfig, MultiAgentRuntimeHandle,
    MultiAgentRuntimeManager, MultiAgentRuntimeManagerError, global_manager, global_runtime,
    init_global_runtime,
};
pub use registry::{AgentRegistry, AgentRegistryHandle};
pub use supervisor::{Supervisor, SupervisorHandle};
pub use task_orchestrator::queue::{TaskQueue, TaskQueueHandle};
pub use team_protocol::{
    ChannelType, Goal, GoalPriority, GoalStatus, MessagePayload, Role, Team, TeamConfig,
    TeamMessage,
};

