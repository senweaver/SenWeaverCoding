// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
#[allow(clippy::module_inception)]
mod sqlite_gateway_hydrate;
pub mod agent;
pub mod auto_title;
pub mod bridge_types;
pub mod budget_ledger;
pub mod builtin_skills;
pub mod classifier;
pub mod cli_runtime;
pub mod coding_mode;
pub mod context_analyzer;
pub mod context_compressor;

pub mod context_expansion;
pub mod context_manager;
pub mod context_pipeline;
pub mod coordination;
pub mod dangling_tool_repair;
pub mod dispatcher;
pub mod error_classify;
pub mod eval;
pub mod executor_core;
pub mod experience;
pub mod feedback;

pub mod flows;
pub mod health_signal;
pub mod history_pruner;
pub mod loop_;
pub mod loop_control;
pub mod loop_core;
pub mod loop_ctx;
pub mod loop_detector;
pub mod loop_services;
pub mod streaming_markers;
pub mod memory_loader;
pub mod mode_effects;
pub mod mode_transition;
pub mod model_switch_guard;
pub mod multi_agent_runtime;
pub mod parallel_executor;
pub mod personality;
pub mod pipeline;
pub mod plan_mode;
pub mod plan_mode_enforcement;
pub mod plan_execution_enforcement;
pub mod profiles;
pub mod prompt;
pub mod prompt_optimizer;
pub mod recovery;
pub mod registry;
pub mod role_pipeline;
pub mod reinforcement;
pub mod repl_command;
pub mod runtime_hooks;
pub mod scheduler;
pub mod scheduler_runtime;
pub mod self_consistency;
pub mod self_eval;
pub mod self_reflection;
pub mod skill_evolution;
pub mod subagent_limiter;
pub mod suggestions;
pub mod supervisor;
pub mod task_queue;
pub mod task_router;
pub mod team_protocol;
pub mod thinking;
pub mod token_budget;
pub mod token_optimizer;
pub mod tool_error_handler;
pub mod tool_event_status;
pub mod tool_output_compressor;
pub mod web_search_url_guard;
pub mod turn_common;

pub mod turn_engine;
pub mod turn_finalize;
pub mod turn_loop_state;
pub mod user_profile;
pub mod user_turn;

pub mod verification;
pub mod workflow_loader;

#[allow(unused_imports)]
pub use agent::{Agent, AgentBuilder, SubagentChunkKind, TurnEvent};
#[allow(unused_imports)]
pub use context_manager::{
    AgentId, ConflictStrategy, ContextSnapshot, ContextValue, LayerPriority, LayeredContext, Scope,
    TeamId,
};
#[allow(unused_imports)]
pub use coordination::{Coordinator, CoordinatorHandle};
#[allow(unused_imports)]
pub use loop_::{
    ToolLoopCostTrackingContext, process_message, run, scope_tool_loop_cost_tracking,
};

#[inline]
pub(crate) fn scope_record_tool_loop_cost_usage(
    provider_name: &str,
    model: &str,
    usage: &crate::providers::traits::TokenUsage,
) -> Option<(u64, f64)> {
    loop_::record_tool_loop_cost_usage(provider_name, model, usage)
}
#[allow(unused_imports)]
pub use multi_agent_runtime::{
    MultiAgentRuntime, MultiAgentRuntimeBuilder, MultiAgentRuntimeConfig, MultiAgentRuntimeHandle,
    MultiAgentRuntimeManager, MultiAgentRuntimeManagerError, global_manager, global_runtime,
    init_global_runtime,
};
#[allow(unused_imports)]
pub use parallel_executor::{
    AggregationStrategy, ExecutorConfig, ExecutorStats, ParallelExecutor, Priority, TaskHandle,
    TaskOutput,
};
#[allow(unused_imports)]
pub use pipeline::{
    Pipeline, PipelineBuilder, PipelineConfig, PipelineResult, PipelineStage, PipelineTask,
    StageErrorStrategy, StageKind, StageResult, TaskResult,
};
#[allow(unused_imports)]
pub use registry::{AgentRegistry, AgentRegistryHandle};
#[allow(unused_imports)]
pub use supervisor::{Supervisor, SupervisorHandle};
#[allow(unused_imports)]
pub use task_queue::{TaskQueue, TaskQueueHandle};
#[allow(unused_imports)]
pub use task_router::{RoutingDecision, RoutingStrategy, Task, TaskRouter, TaskRouterConfig};
#[allow(unused_imports)]
pub use team_protocol::{
    ChannelType, Goal, GoalPriority, GoalStatus, MessagePayload, Role, Team, TeamConfig,
    TeamMessage,
};
