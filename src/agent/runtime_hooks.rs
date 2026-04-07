// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Runtime integration hooks that wire disconnected modules into the agent loop.
//!
//! This module acts as a bridge between the core agent loop and the various
//! subsystems (learning, optimization, security, events) that need to be
//! invoked at specific points during execution.

use crate::config::Config;
use std::sync::Arc;

/// Aggregated runtime hooks injected into the agent loop.
///
/// Each field is optional — when `None`, the corresponding hook is a no-op.
/// This keeps the core loop clean while allowing all subsystems to participate.
pub struct RuntimeHooks {
    pub token_optimizer: Option<Arc<crate::agent::token_optimizer::TokenOptimizer>>,
    pub guardrails: Option<Arc<crate::guardrails::GuardrailsEngine>>,
}

impl RuntimeHooks {
    /// Build runtime hooks from configuration.
    pub fn from_config(config: &Config) -> Self {
        let token_optimizer = if config.tool_output_compressor.enabled {
            Some(crate::agent::token_optimizer::create_optimizer(
                config.tool_output_compressor.clone(),
                config.token_budget.clone(),
            ))
        } else {
            None
        };

        let guardrails = if config.guardrails.enabled {
            Some(Arc::new(crate::guardrails::GuardrailsEngine::new(
                config.guardrails.clone(),
            )))
        } else {
            None
        };

        Self {
            token_optimizer,
            guardrails,
        }
    }

    /// Compress a tool output through the token optimizer.
    /// Returns the original output unchanged if optimizer is disabled.
    pub fn compress_tool_output(&self, tool_name: &str, output: &str) -> String {
        match &self.token_optimizer {
            Some(opt) => opt.compress_tool_output(tool_name, output),
            None => output.to_string(),
        }
    }

    /// Check if a tool call is allowed by guardrails.
    /// Returns Ok(()) if allowed, Err with reason if denied.
    pub fn check_guardrails(&self, tool_name: &str, context: Option<&str>) -> Result<(), String> {
        match &self.guardrails {
            Some(engine) => {
                let verdict = engine.check(tool_name, context);
                if verdict.allowed {
                    Ok(())
                } else {
                    Err(verdict.reason)
                }
            }
            None => Ok(()),
        }
    }

    /// Record a tool call in guardrails (for rate limiting).
    pub fn record_tool_call(&self, tool_name: &str) {
        if let Some(ref engine) = self.guardrails {
            engine.record_call(tool_name);
        }
    }

    /// Record API usage for token budget tracking.
    pub fn record_api_usage(&self, input_tokens: usize, output_tokens: usize) {
        if let Some(ref opt) = self.token_optimizer {
            opt.record_api_usage(input_tokens, output_tokens);
        }
    }
}

/// Post-turn learning hooks that run after each complete turn.
///
/// These are fire-and-forget: failures are logged but don't affect the
/// agent's response.
pub struct LearningHooks {
    pub self_eval_enabled: bool,
    pub feedback_enabled: bool,
    pub experience_enabled: bool,
    pub reflection_enabled: bool,
    pub skill_evolution_enabled: bool,
}

impl LearningHooks {
    pub fn from_config(config: &Config) -> Self {
        Self {
            self_eval_enabled: config.self_eval.enabled,
            feedback_enabled: config.feedback.enabled,
            experience_enabled: config.experience.enabled,
            reflection_enabled: config.self_reflection.enabled,
            skill_evolution_enabled: config.skill_evolution.enabled,
        }
    }

    /// Run post-turn learning (heuristic eval + feedback signal detection).
    /// This should be called after each assistant response.
    pub fn record_turn_heuristics(
        &self,
        user_message: &str,
        assistant_response: &str,
        tool_results: &[(&str, bool)],
    ) {
        if !self.self_eval_enabled && !self.feedback_enabled {
            return;
        }

        if self.self_eval_enabled {
            let dims = crate::agent::self_eval::heuristic_eval(
                user_message,
                assistant_response,
                tool_results,
            );
            tracing::debug!(
                relevance = dims.relevance,
                completeness = dims.completeness,
                accuracy = dims.accuracy,
                "Self-eval heuristic dimensions for turn"
            );
        }

        if self.feedback_enabled {
            let signal =
                crate::agent::feedback::detect_next_state_signal(assistant_response, user_message);
            let signal_score = signal.to_score();
            tracing::debug!(
                signal_score = signal_score,
                "Feedback next-state signal for turn"
            );
        }
    }

    /// Record tool execution for skill evolution tracking.
    pub fn record_tool_execution(&self, tool_name: &str, success: bool, duration_ms: u64) {
        if !self.skill_evolution_enabled {
            return;
        }

        let engine = crate::agent::skill_evolution::global_engine();
        engine.record_execution(tool_name, success, duration_ms, None, "general", 0.0);
    }
}

/// Fire-and-forget event bus publishing for agent lifecycle changes.
/// Spawns a background task so the caller doesn't need to await.
pub fn publish_lifecycle_event(phase: &str) {
    let phase_enum = match phase {
        "started" => crate::event_bus::types::LifecyclePhase::Started,
        "stopped" => crate::event_bus::types::LifecyclePhase::Stopped,
        "error" => crate::event_bus::types::LifecyclePhase::Error,
        _ => crate::event_bus::types::LifecyclePhase::Spawned,
    };
    tokio::spawn(async move {
        crate::event_bus::integration::publish_lifecycle("agent_loop", phase_enum, None).await;
    });
}

/// Fire-and-forget event bus publishing for tool calls.
/// Spawns a background task so the caller doesn't need to await.
pub fn publish_tool_event(tool_name: &str, success: bool, duration_ms: u64) {
    let name = tool_name.to_string();
    tokio::spawn(async move {
        crate::event_bus::integration::publish_tool_call("agent_loop", &name, success, duration_ms)
            .await;
    });
}

/// Fire-and-forget event bus publishing for memory operations.
/// Spawns a background task so the caller doesn't need to await.
pub fn publish_memory_event(operation: &str, key: Option<&str>) {
    let op = match operation {
        "store" => crate::event_bus::types::MemoryOperation::Store,
        "recall" => crate::event_bus::types::MemoryOperation::Recall,
        "forget" => crate::event_bus::types::MemoryOperation::Forget,
        "consolidate" => crate::event_bus::types::MemoryOperation::Consolidate,
        _ => crate::event_bus::types::MemoryOperation::Store,
    };
    let key_owned = key.map(|k| k.to_string());
    tokio::spawn(async move {
        crate::event_bus::integration::publish_memory_op("agent", op, key_owned).await;
    });
}

/// Track a delegate sub-agent spawn in the global multi-agent runtime.
/// Registers the sub-agent in the AgentRegistry with its capabilities.
pub fn track_delegate_spawn(agent_name: &str, provider: &str, model: &str) {
    if let Some(rt) = crate::agent::multi_agent_runtime::global_runtime() {
        use crate::agent::registry::{AgentCapability, AgentInfo};
        let id = format!(
            "delegate-{}-{}",
            agent_name,
            uuid::Uuid::new_v4().as_simple()
        );
        let mut info = AgentInfo::new(&id, agent_name, "delegate");
        info.capabilities.push(AgentCapability {
            name: "delegate".to_string(),
            description: format!("Delegate sub-agent ({provider}/{model})"),
            proficiency: 1.0,
        });
        let _ = rt.registry.register(info);
        rt.registry
            .set_state(&id, crate::agent::registry::AgentState::Active);
        tracing::debug!(agent_name, delegate_id = %id, "Tracked delegate sub-agent spawn");
    }
}

/// Track a delegate sub-agent completion in the global multi-agent runtime.
pub fn track_delegate_complete(agent_name: &str, success: bool) {
    if let Some(rt) = crate::agent::multi_agent_runtime::global_runtime() {
        // Find delegate agents matching this name and mark them completed
        let agents = rt.registry.all();
        for agent in agents {
            if agent.name == agent_name && agent.role == "delegate" {
                rt.registry.complete_task(&agent.id, success);
                rt.registry
                    .set_state(&agent.id, crate::agent::registry::AgentState::Terminated);
                break;
            }
        }
    }
}

/// Fire-and-forget event bus publishing for messages.
pub fn publish_message_event(direction: &str, channel: &str) {
    let dir = direction.to_string();
    let ch = channel.to_string();
    tokio::spawn(async move {
        match dir.as_str() {
            "received" => {
                crate::event_bus::integration::publish_message_received("agent", &ch, "").await;
            }
            "sent" => {
                crate::event_bus::integration::publish_message_sent("agent", &ch, "").await;
            }
            _ => {}
        }
    });
}
