// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Async bridge between the ratatui event loop and the agent runtime.
//!
//! The TUI runs a synchronous `crossterm::event::poll` loop on the main
//! thread while the agent runs asynchronously in a separate tokio task.
//! This module provides the channel types and spawn logic to connect them.

use crate::config::Config;
use tokio::sync::mpsc;

pub use crate::agent::bridge_types::{AgentEvent, UserInput};

/// Bidirectional bridge between TUI and the agent runtime.
pub struct AgentBridge {
    pub sender: mpsc::Sender<UserInput>,
    pub receiver: mpsc::Receiver<AgentEvent>,
    pub is_busy: bool,
}

impl AgentBridge {
    /// Drain all pending events from the agent (non-blocking).
    pub fn poll_events(&mut self) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        while let Ok(ev) = self.receiver.try_recv() {
            match &ev {
                AgentEvent::Done | AgentEvent::Error(_) => self.is_busy = false,
                AgentEvent::Thinking => self.is_busy = true,
                _ => {}
            }
            events.push(ev);
        }
        events
    }

    /// Send a user input to the agent (non-blocking best-effort).
    pub fn send(&self, input: UserInput) -> bool {
        self.sender.try_send(input).is_ok()
    }
}

/// Spawn the agent loop in a background tokio task and return the bridge.
pub fn spawn_agent_task(config: Config) -> AgentBridge {
    let (user_tx, mut user_rx) = mpsc::channel::<UserInput>(32);
    let (agent_tx, agent_rx) = mpsc::channel::<AgentEvent>(128);

    tokio::spawn(async move {
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
                            let _ = agent_tx.send(AgentEvent::AssistantMessage(response)).await;
                        }
                        Err(e) => {
                            let _ = agent_tx.send(AgentEvent::Error(format!("{e:#}"))).await;
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

    AgentBridge {
        sender: user_tx,
        receiver: agent_rx,
        is_busy: false,
    }
}

/// Execute a slash command and return its output as a string.
async fn execute_slash_command(name: &str, args: &[String], _config: &Config) -> String {
    if let Some(svc) = std::panic::catch_unwind(crate::services::get_services).ok() {
        let ctx = crate::commands::registry::CommandContext {
            session_id: "tui".to_string(),
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
