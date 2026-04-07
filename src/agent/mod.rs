// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
#[allow(clippy::module_inception)]
pub mod agent;
pub mod auto_title;
pub mod bridge_types;
pub mod builtin_skills;
pub mod classifier;
pub mod coding_mode;
pub mod context_analyzer;
pub mod context_compressor;
pub mod coordination;
pub mod dangling_tool_repair;
pub mod dispatcher;
pub mod eval;
pub mod experience;
pub mod feedback;
pub mod history_pruner;
pub mod loop_;
pub mod loop_detector;
pub mod memory_loader;
pub mod multi_agent_runtime;
pub mod personality;
pub mod plan_mode;
pub mod profiles;
pub mod prompt;
pub mod prompt_optimizer;
pub mod registry;
pub mod reinforcement;
pub mod runtime_hooks;
pub mod self_eval;
pub mod self_reflection;
pub mod skill_evolution;
pub mod subagent_limiter;
pub mod suggestions;
pub mod supervisor;
pub mod task_queue;
pub mod thinking;
pub mod token_budget;
pub mod token_optimizer;
pub mod tool_error_handler;
pub mod tool_output_compressor;
pub mod user_profile;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use agent::{Agent, AgentBuilder, TurnEvent};
#[allow(unused_imports)]
pub use coordination::{Coordinator, CoordinatorHandle};
#[allow(unused_imports)]
pub use loop_::{process_message, run};
#[allow(unused_imports)]
pub use multi_agent_runtime::{MultiAgentRuntime, global_runtime, init_global_runtime};
#[allow(unused_imports)]
pub use registry::{AgentRegistry, AgentRegistryHandle};
#[allow(unused_imports)]
pub use supervisor::{Supervisor, SupervisorHandle};
#[allow(unused_imports)]
pub use task_queue::{TaskQueue, TaskQueueHandle};
