// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Command registry — mirrors claude-code-typescript-src`commands.ts`.
// Central registry for slash commands with filtering and execution.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Result of a command execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub success: bool,
    pub message: Option<String>,
    pub data: Option<serde_json::Value>,
}

impl CommandResult {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: Some(message.into()),
            data: None,
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: Some(message.into()),
            data: None,
        }
    }
}

/// Context passed to slash command handlers.
#[derive(Clone)]
pub struct CommandContext {
    pub session_id: String,
    pub cwd: std::path::PathBuf,
    pub args: Vec<String>,
    pub raw_input: String,
    pub is_interactive: bool,
    pub is_remote: bool,
}

/// A slash command definition.
#[derive(Clone)]
pub struct SlashCommand {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
    pub usage: String,
    pub category: CommandCategory,
    pub hidden: bool,
    pub requires_interactive: bool,
    pub remote_safe: bool,
    pub handler: Arc<
        dyn Fn(CommandContext) -> Pin<Box<dyn Future<Output = CommandResult> + Send>> + Send + Sync,
    >,
}

impl std::fmt::Debug for SlashCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SlashCommand")
            .field("name", &self.name)
            .field("description", &self.description)
            .finish()
    }
}

/// Command categories for grouping in help output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandCategory {
    General,
    Session,
    Configuration,
    Memory,
    Skills,
    Tasks,
    Tools,
    Debug,
    Internal,
}

impl std::fmt::Display for CommandCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::General => write!(f, "General"),
            Self::Session => write!(f, "Session"),
            Self::Configuration => write!(f, "Configuration"),
            Self::Memory => write!(f, "Memory"),
            Self::Skills => write!(f, "Skills"),
            Self::Tasks => write!(f, "Tasks"),
            Self::Tools => write!(f, "Tools"),
            Self::Debug => write!(f, "Debug"),
            Self::Internal => write!(f, "Internal"),
        }
    }
}

/// Central registry for all slash commands.
pub struct CommandRegistry {
    commands: Vec<SlashCommand>,
    name_index: HashMap<String, usize>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            name_index: HashMap::new(),
        }
    }

    /// Register a slash command.
    pub fn register(&mut self, cmd: SlashCommand) {
        let idx = self.commands.len();
        self.name_index.insert(cmd.name.clone(), idx);
        for alias in &cmd.aliases {
            self.name_index.insert(alias.clone(), idx);
        }
        self.commands.push(cmd);
    }

    /// Look up a command by name or alias.
    pub fn find(&self, name: &str) -> Option<&SlashCommand> {
        // Strip leading '/' if present
        let name = name.strip_prefix('/').unwrap_or(name);
        self.name_index
            .get(name)
            .and_then(|&idx| self.commands.get(idx))
    }

    /// Execute a command by name.
    pub async fn execute(&self, name: &str, ctx: CommandContext) -> CommandResult {
        match self.find(name) {
            Some(cmd) => {
                if cmd.requires_interactive && !ctx.is_interactive {
                    return CommandResult::err(format!(
                        "Command '/{name}' requires interactive mode"
                    ));
                }
                if !cmd.remote_safe && ctx.is_remote {
                    return CommandResult::err(format!(
                        "Command '/{name}' is not available in remote mode"
                    ));
                }
                (cmd.handler)(ctx).await
            }
            None => CommandResult::err(format!("Unknown command: /{name}")),
        }
    }

    /// List all visible commands, optionally filtered by category.
    pub fn list(&self, category: Option<CommandCategory>) -> Vec<&SlashCommand> {
        self.commands
            .iter()
            .filter(|c| !c.hidden)
            .filter(|c| category.map_or(true, |cat| c.category == cat))
            .collect()
    }

    /// List commands available in the current context.
    pub fn available_commands(&self, is_interactive: bool, is_remote: bool) -> Vec<&SlashCommand> {
        self.commands
            .iter()
            .filter(|c| !c.hidden)
            .filter(|c| !c.requires_interactive || is_interactive)
            .filter(|c| c.remote_safe || !is_remote)
            .collect()
    }

    /// Get command names for tab completion.
    pub fn completions(&self, prefix: &str) -> Vec<String> {
        let prefix = prefix.strip_prefix('/').unwrap_or(prefix);
        self.commands
            .iter()
            .filter(|c| !c.hidden)
            .filter(|c| c.name.starts_with(prefix))
            .map(|c| format!("/{}", c.name))
            .collect()
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}
