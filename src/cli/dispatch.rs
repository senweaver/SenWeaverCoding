// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! CLI slash-command dispatch — looks the command up in the global
//! [`crate::commands::registry::CommandRegistry`] (built at start-up
//! inside [`crate::services::container::ServiceContainer::new`]) and
//! executes it against a supplied
//! [`crate::commands::registry::CommandContext`].
//!
//! This replaces the previous placeholder that just `bail!`-ed with a
//! real dispatcher: every registered `/slash` command is reachable
//! here.  Binaries that embed the agent (CLI, TUI, channel adapters)
//! should prefer this entry point over hand-written `match` arms so
//! new commands are picked up automatically.

use anyhow::{Result, anyhow};

use crate::commands::registry::{CommandContext, CommandResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispatch {

    Found,

    Unknown,
}

pub async fn dispatch(name: &str, ctx: CommandContext) -> Result<CommandResult> {
    let svc = crate::services::try_get_services()
        .ok_or_else(|| anyhow!("dispatch: service container not initialised"))?;
    let trimmed = name.strip_prefix('/').unwrap_or(name).trim();
    let cmd = svc
        .command_registry
        .find(trimmed)
        .ok_or_else(|| anyhow!("dispatch: unknown command `{trimmed}`"))?;
    let handler = std::sync::Arc::clone(&cmd.handler);
    Ok(handler(ctx).await)
}

pub fn resolve(name: &str) -> Dispatch {
    let Some(svc) = crate::services::try_get_services() else {
        return Dispatch::Unknown;
    };
    let trimmed = name.strip_prefix('/').unwrap_or(name).trim();
    if svc.command_registry.find(trimmed).is_some() {
        Dispatch::Found
    } else {
        Dispatch::Unknown
    }
}

pub fn list_commands() -> Vec<String> {
    match crate::services::try_get_services() {
        Some(svc) => svc
            .command_registry
            .list(None)
            .iter()
            .map(|c| c.name.clone())
            .collect(),
        None => Vec::new(),
    }
}
