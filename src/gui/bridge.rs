// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Bridge between the egui GUI and the agent runtime.
//!
//! Mirrors the TUI `agent_bridge` pattern — spawns the full agent loop
//! (`crate::agent::run` from `loop_.rs`) in a tokio task and communicates
//! via mpsc channels. This ensures the GUI has **identical** backend
//! capabilities to the CLI/TUI: all tools, memory, compaction, slash
//! commands, etc.

use crate::agent::bridge_types::{AgentEvent, UserInput};
use crate::config::Config;
use tokio::sync::mpsc;

/// Bidirectional bridge for GUI <-> agent communication.
pub struct GuiBridge {
    user_tx: mpsc::Sender<UserInput>,
    agent_rx: mpsc::Receiver<AgentEvent>,
    pub is_busy: bool,
}

impl GuiBridge {
    /// Drain all pending events (non-blocking).
    pub fn poll_events(&mut self) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        while let Ok(ev) = self.agent_rx.try_recv() {
            match &ev {
                AgentEvent::Done | AgentEvent::Error(_) => self.is_busy = false,
                AgentEvent::Thinking => self.is_busy = true,
                _ => {}
            }
            events.push(ev);
        }
        events
    }

    /// Send a user input to the agent (non-blocking).
    pub fn send(&self, input: UserInput) -> bool {
        self.user_tx.try_send(input).is_ok()
    }
}

/// Spawn the full agent loop in a tokio task and return the bridge.
///
/// Uses the same `crate::agent::run` (from `loop_.rs`) that the TUI and
/// CLI REPL use, so the GUI gets all tools, memory, compaction, provider
/// resilience, hardware RAG, and slash commands.
pub fn spawn_bridge(rt: &tokio::runtime::Runtime, config: Config) -> GuiBridge {
    let (user_tx, mut user_rx) = mpsc::channel::<UserInput>(32);
    let (agent_tx, agent_rx) = mpsc::channel::<AgentEvent>(128);

    rt.spawn(async move {
        while let Some(input) = user_rx.recv().await {
            match input {
                UserInput::Chat(message) => {
                    let _ = agent_tx.send(AgentEvent::Thinking).await;

                    let result = Box::pin(crate::agent::run(
                        config.clone(),
                        Some(message),
                        None,
                        None,
                        config.default_temperature,
                        Vec::new(),
                        false,
                        None,
                        None,
                    ))
                    .await;

                    match result {
                        Ok(response) => {
                            let _ = agent_tx
                                .send(AgentEvent::AssistantMessage(response))
                                .await;
                        }
                        Err(e) => {
                            let _ = agent_tx
                                .send(AgentEvent::Error(format!("{e:#}")))
                                .await;
                        }
                    }
                    let _ = agent_tx.send(AgentEvent::Done).await;
                }

                UserInput::SlashCommand { name, args } => {
                    let _ = agent_tx.send(AgentEvent::Thinking).await;

                    let output = execute_slash_command(&name, &args, &config).await;
                    let _ = agent_tx.send(AgentEvent::CommandOutput(output)).await;
                    let _ = agent_tx.send(AgentEvent::Done).await;
                }

                UserInput::Cancel => {
                    let _ = agent_tx.send(AgentEvent::Done).await;
                }
            }
        }
    });

    GuiBridge {
        user_tx,
        agent_rx,
        is_busy: false,
    }
}

/// Execute a slash command and return its output as a string.
///
/// Mirrors the TUI's `execute_slash_command` — uses the `CommandRegistry`
/// from the global `ServiceContainer`.
async fn execute_slash_command(name: &str, args: &[String], _config: &Config) -> String {
    if let Ok(svc) = std::panic::catch_unwind(crate::services::get_services) {
        let ctx = crate::commands::registry::CommandContext {
            session_id: "gui".to_string(),
            cwd: std::env::current_dir().unwrap_or_default(),
            args: args.to_vec(),
            raw_input: format!("/{name} {}", args.join(" ")),
            is_interactive: true,
            is_remote: false,
        };
        let result = svc.command_registry.execute(name, ctx).await;
        result.message.unwrap_or_default()
    } else {
        format!("Services not initialized — cannot run /{name}")
    }
}
