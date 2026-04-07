// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Main application struct implementing `eframe::App`.

use super::bridge::{self, GuiBridge};
use super::pages;
use super::state::{AppState, ChatMessage, MessageRole, Page};
use super::theme;
use crate::agent::bridge_types::AgentEvent;

pub struct SenApp {
    pub state: AppState,
    bridge: GuiBridge,
    #[allow(dead_code)]
    tokio_rt: tokio::runtime::Runtime,
    theme_applied: bool,
}

impl SenApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime");

        let config = rt.block_on(async {
            crate::config::Config::load_or_init().await.unwrap_or_default()
        });

        // Ensure global services are initialized so slash commands work
        // even before the first `agent::run` call (which also calls
        // init_services internally).
        let _ = std::panic::catch_unwind(|| {
            crate::services::init_services(
                crate::services::container::ServiceContainerConfig::default(),
            );
        });
        let _ = std::panic::catch_unwind(|| {
            crate::event_bus::integration::init_global_bus();
        });

        let bridge = bridge::spawn_bridge(&rt, config);

        Self {
            state: AppState::default(),
            bridge,
            tokio_rt: rt,
            theme_applied: false,
        }
    }

    fn process_agent_events(&mut self) {
        let events = self.bridge.poll_events();
        self.state.is_agent_busy = self.bridge.is_busy;
        for event in events {
            match event {
                AgentEvent::AssistantMessage(text) => {
                    self.state.messages.push(ChatMessage {
                        role: MessageRole::Assistant,
                        content: text,
                    });
                }
                AgentEvent::ToolUse { name, id } => {
                    self.state.messages.push(ChatMessage {
                        role: MessageRole::ToolUse,
                        content: format!("Using tool: {name} (id: {id})"),
                    });
                }
                AgentEvent::ToolResult { output, success, .. } => {
                    self.state.messages.push(ChatMessage {
                        role: MessageRole::ToolResult,
                        content: if success {
                            output
                        } else {
                            format!("Error: {output}")
                        },
                    });
                }
                AgentEvent::Error(e) => {
                    self.state.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: format!("Error: {e}"),
                    });
                }
                AgentEvent::CommandOutput(text) => {
                    self.state.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: text,
                    });
                }
                AgentEvent::Thinking | AgentEvent::Done => {}
            }
        }
    }

    fn send_chat(&mut self) {
        let input = self.state.chat_input.trim().to_string();
        if input.is_empty() {
            return;
        }
        self.state.messages.push(ChatMessage {
            role: MessageRole::User,
            content: input.clone(),
        });

        if input.starts_with('/') {
            let parts: Vec<&str> = input[1..].splitn(2, ' ').collect();
            let name = parts[0].to_string();
            let args: Vec<String> = parts
                .get(1)
                .map(|a| a.split_whitespace().map(String::from).collect())
                .unwrap_or_default();
            self.bridge
                .send(crate::agent::bridge_types::UserInput::SlashCommand {
                    name,
                    args,
                });
        } else {
            self.bridge
                .send(crate::agent::bridge_types::UserInput::Chat(input));
        }
        self.state.chat_input.clear();
    }
}

impl eframe::App for SenApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.theme_applied {
            theme::apply_theme(ctx);
            self.theme_applied = true;
        }

        self.process_agent_events();

        // Request continuous repaint while agent is busy (for spinner)
        if self.state.is_agent_busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        match self.state.current_page {
            Page::Home => pages::home::show(ctx, &mut self.state, &self.bridge),
            Page::Marketplace => pages::marketplace::show(ctx, &mut self.state),
            Page::Settings => pages::settings::show(ctx, &mut self.state),
        }

        // Handle Enter key to send chat (global, only on Home page)
        if self.state.current_page == Page::Home
            && ctx.input(|i| i.key_pressed(egui::Key::Enter))
            && !ctx.input(|i| i.modifiers.shift)
        {
            self.send_chat();
        }
    }
}
