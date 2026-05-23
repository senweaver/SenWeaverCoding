// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

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

#[derive(Clone)]
pub struct CommandContext {
    pub session_id: String,
    pub cwd: std::path::PathBuf,
    pub args: Vec<String>,
    pub raw_input: String,
    pub is_interactive: bool,
    pub is_remote: bool,
}

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

pub type BoxedHandler = Arc<
    dyn Fn(CommandContext) -> Pin<Box<dyn Future<Output = CommandResult> + Send>> + Send + Sync,
>;

#[derive(Clone)]
pub struct StaticSlashCommand {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub usage: &'static str,
    pub category: CommandCategory,
    pub hidden: bool,
    pub requires_interactive: bool,
    pub remote_safe: bool,
    pub handler: HandlerPtr,
}

inventory::collect!(StaticSlashCommand);

pub struct HandlerPtr(*const ());

#[allow(unsafe_code)]
unsafe impl Send for HandlerPtr {}
#[allow(unsafe_code)]
unsafe impl Sync for HandlerPtr {}

impl Clone for HandlerPtr {
    fn clone(&self) -> Self {
        HandlerPtr(self.0)
    }
}

impl HandlerPtr {

    #[doc(hidden)]
    pub const fn from_lazy_ptr(lazy_ptr: *const std::sync::LazyLock<BoxedHandler>) -> Self {
        Self(lazy_ptr.cast::<()>())
    }

    pub fn resolve(&self) -> &'static BoxedHandler {

        #[allow(unsafe_code)]
        unsafe {
            let lazy_ptr = self.0.cast::<std::sync::LazyLock<BoxedHandler>>();
            &*lazy_ptr
        }
    }
}

pub struct HandlerWrapper {
    pub handler: BoxedHandler,
}

impl HandlerWrapper {

    pub fn new<H, F>(handler: H) -> Self
    where
        H: Fn(CommandContext) -> F + Send + Sync + 'static,
        F: std::future::Future<Output = CommandResult> + Send + 'static,
    {
        Self {
            handler: Arc::new(
                move |ctx: CommandContext| -> Pin<
                    Box<dyn std::future::Future<Output = CommandResult> + Send + 'static>,
                > { Box::pin(handler(ctx)) },
            ),
        }
    }
}

#[doc(hidden)]
#[inline]
pub fn make_handler<H, F>(handler: H) -> BoxedHandler
where
    H: Fn(CommandContext) -> F + Send + Sync + 'static,
    F: std::future::Future<Output = CommandResult> + Send + 'static,
{
    HandlerWrapper::new(handler).handler
}

impl StaticSlashCommand {

    pub fn to_slash_command(&self) -> SlashCommand {
        SlashCommand {
            name: self.name.to_string(),
            aliases: self.aliases.iter().map(|s| s.to_string()).collect(),
            description: self.description.to_string(),
            usage: self.usage.to_string(),
            category: self.category,
            hidden: self.hidden,
            requires_interactive: self.requires_interactive,
            remote_safe: self.remote_safe,
            handler: Arc::clone(self.handler.resolve()),
        }
    }
}

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

    pub fn from_inventory() -> Self {
        let mut registry = Self::new();
        for static_cmd in inventory::iter::<StaticSlashCommand> {
            let cmd = static_cmd.to_slash_command();
            let idx = registry.commands.len();
            registry.name_index.insert(cmd.name.clone(), idx);
            for alias in &cmd.aliases {
                registry.name_index.insert(alias.clone(), idx);
            }
            registry.commands.push(cmd);
        }
        registry
    }

    pub fn register(&mut self, cmd: SlashCommand) {
        let idx = self.commands.len();
        self.name_index.insert(cmd.name.clone(), idx);
        for alias in &cmd.aliases {
            self.name_index.insert(alias.clone(), idx);
        }
        self.commands.push(cmd);
    }

    pub fn find(&self, name: &str) -> Option<&SlashCommand> {
        let name = name.strip_prefix('/').unwrap_or(name);
        self.name_index
            .get(name)
            .and_then(|&idx| self.commands.get(idx))
    }

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

    pub fn list(&self, category: Option<CommandCategory>) -> Vec<&SlashCommand> {
        self.commands
            .iter()
            .filter(|c| !c.hidden)
            .filter(|c| category.map_or(true, |cat| c.category == cat))
            .collect()
    }

    pub fn available_commands(&self, is_interactive: bool, is_remote: bool) -> Vec<&SlashCommand> {
        self.commands
            .iter()
            .filter(|c| !c.hidden)
            .filter(|c| !c.requires_interactive || is_interactive)
            .filter(|c| c.remote_safe || !is_remote)
            .collect()
    }

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
