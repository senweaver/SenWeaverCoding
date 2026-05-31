// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::config::Config;
use std::sync::Arc;

pub struct RuntimeHooks {
    pub token_optimizer: Option<Arc<crate::agent::token::optimizer::TokenOptimizer>>,
    pub guardrails: Option<Arc<crate::guardrails::GuardrailsEngine>>,
}

impl RuntimeHooks {

    pub fn from_config(config: &Config) -> Self {
        let token_optimizer = if config.tool_output_compressor.enabled {
            Some(crate::agent::token::optimizer::create_optimizer(
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

    pub fn compress_tool_output(&self, tool_name: &str, output: &str) -> String {
        match &self.token_optimizer {
            Some(opt) => opt.compress_tool_output(tool_name, output),
            None => output.to_string(),
        }
    }

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

    pub fn record_tool_call(&self, tool_name: &str) {
        if let Some(ref engine) = self.guardrails {
            engine.record_call(tool_name);
        }
    }

    pub fn record_api_usage(&self, input_tokens: usize, output_tokens: usize) {
        if let Some(ref opt) = self.token_optimizer {
            opt.record_api_usage(input_tokens, output_tokens);
        }
    }
}

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
            let dims = crate::agent::self_assess::eval::heuristic_eval(
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
                crate::agent::reward::feedback::detect_next_state_signal(assistant_response, user_message);
            let signal_score = signal.to_score();
            tracing::debug!(
                signal_score = signal_score,
                "Feedback next-state signal for turn"
            );
        }
    }

    pub fn record_tool_execution(&self, tool_name: &str, success: bool, duration_ms: u64) {
        if !self.skill_evolution_enabled {
            return;
        }

        let engine = crate::agent::skill_evolution::global_engine();
        engine.record_execution(tool_name, success, duration_ms, None, "general", 0.0);
    }
}

pub fn publish_lifecycle_event(phase: &str) {
    let phase_enum = match phase {
        "started" => crate::event_bus::types::LifecyclePhase::Started,
        "stopped" => crate::event_bus::types::LifecyclePhase::Stopped,
        "error" => crate::event_bus::types::LifecyclePhase::Error,
        _ => crate::event_bus::types::LifecyclePhase::Spawned,
    };
    crate::runtime::task_manager::spawn_supervised("agent.lifecycle", async move {
        crate::event_bus::integration::publish_lifecycle("agent_loop", phase_enum, None).await;
    });
}

pub fn publish_tool_event(tool_name: &str, success: bool, duration_ms: u64) {
    let name = tool_name.to_string();
    crate::runtime::task_manager::spawn_supervised("agent.tool_event", async move {
        crate::event_bus::integration::publish_tool_call("agent_loop", &name, success, duration_ms)
            .await;
    });
}

pub fn publish_memory_event(operation: &str, key: Option<&str>) {
    let op = match operation {
        "store" => crate::event_bus::types::MemoryOperation::Store,
        "recall" => crate::event_bus::types::MemoryOperation::Recall,
        "forget" => crate::event_bus::types::MemoryOperation::Forget,
        "consolidate" => crate::event_bus::types::MemoryOperation::Consolidate,
        _ => crate::event_bus::types::MemoryOperation::Store,
    };
    let key_owned = key.map(|k| k.to_string());
    crate::runtime::task_manager::spawn_supervised("agent.memory_event", async move {
        crate::event_bus::integration::publish_memory_op("agent", op, key_owned).await;
    });
}

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
        let _ = rt
            .registry
            .set_state(&id, crate::agent::registry::AgentState::Active);
        tracing::debug!(agent_name, delegate_id = %id, "Tracked delegate sub-agent spawn");
    }
}

pub fn track_delegate_complete(agent_name: &str, success: bool) {
    if let Some(rt) = crate::agent::multi_agent_runtime::global_runtime() {

        let agents = rt.registry.all();
        for agent in agents {
            if agent.name == agent_name && agent.role == "delegate" {
                rt.registry.complete_task(&agent.id, success);
                let _ = rt
                    .registry
                    .set_state(&agent.id, crate::agent::registry::AgentState::Terminated);
                break;
            }
        }
    }
}

pub fn publish_message_event(direction: &str, channel: &str) {
    let dir = direction.to_string();
    let ch = channel.to_string();
    crate::runtime::task_manager::spawn_supervised("agent.message_event", async move {
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
