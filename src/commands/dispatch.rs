// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use std::sync::OnceLock;

use super::registry::{CommandContext, CommandRegistry};

#[derive(Debug, Clone)]
pub enum SlashOutcome {
    NotCommand,
    Quit,
    Clear,
    Handled { success: bool, message: String },
    Followup {
        message: Option<String>,
        prompt: String,
    },
}

pub fn global_registry() -> &'static CommandRegistry {
    static REGISTRY: OnceLock<CommandRegistry> = OnceLock::new();
    REGISTRY.get_or_init(CommandRegistry::from_inventory)
}

pub async fn dispatch_slash_input(raw: &str) -> SlashOutcome {
    let (session_id, cwd, is_interactive, is_remote) = match crate::bootstrap::try_get_state() {
        Some(bs) => bs.read(|state| {
            (
                state.session_id.0.clone(),
                state.cwd.clone(),
                state.is_interactive,
                state.is_remote_mode,
            )
        }),
        None => (
            uuid::Uuid::new_v4().to_string(),
            std::env::current_dir().unwrap_or_default(),
            true,
            false,
        ),
    };
    dispatch_slash_input_scoped(raw, session_id, cwd, is_interactive, is_remote).await
}

pub async fn dispatch_slash_input_scoped(
    raw: &str,
    session_id: String,
    cwd: std::path::PathBuf,
    is_interactive: bool,
    is_remote: bool,
) -> SlashOutcome {
    let trimmed = raw.trim();
    if !trimmed.starts_with('/') {
        return SlashOutcome::NotCommand;
    }

    let registry = global_registry();
    match crate::agent::repl_command::parse_repl_input(trimmed, registry) {
        crate::agent::repl_command::ReplCommand::Quit => SlashOutcome::Quit,
        crate::agent::repl_command::ReplCommand::Clear => SlashOutcome::Clear,
        crate::agent::repl_command::ReplCommand::Slash { name, args } => {
            let ctx = CommandContext {
                session_id,
                cwd,
                args,
                raw_input: trimmed.to_string(),
                is_interactive,
                is_remote,
            };
            let result = registry.execute(&name, ctx).await;
            match result.followup_prompt {
                Some(prompt) => SlashOutcome::Followup {
                    message: result.message,
                    prompt,
                },
                None => SlashOutcome::Handled {
                    success: result.success,
                    message: result.message.unwrap_or_default(),
                },
            }
        }
        _ => {
            let cmd_token = trimmed
                .split_whitespace()
                .next()
                .unwrap_or(trimmed)
                .to_string();
            SlashOutcome::Handled {
                success: false,
                message: format!("Unknown command: {cmd_token}. Type /help to list available commands."),
            }
        }
    }
}
