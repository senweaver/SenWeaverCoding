// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
#[macro_use]
mod macros;

pub mod self_test;
pub mod update;

pub mod add_dir;
pub mod agent_exec;
pub mod clear;
pub mod color;
pub mod compact;
pub mod config_cmd;
pub mod context;
pub mod cost;
pub mod diff;
pub mod dispatch;
pub mod doctor_cmd;
pub mod effort;
pub mod export;
pub mod fast;
pub mod help;
pub mod history;
pub mod hooks;
pub mod memory_cmd;
pub mod metrics;
pub mod mode;
pub mod model;
pub mod multi_agent;
pub mod permissions;
pub mod plan;
pub mod plugin_cmd;
pub mod registry;
pub mod resume;
pub mod review;
pub mod session;
pub mod skills_cmd;
pub mod stats;
pub mod status;
pub mod tasks_cmd;
pub mod theme;
pub mod vector_cmd;
pub mod vim;
pub mod voice_cmd;
pub mod workflow_cmd;
pub use registry::{
    CommandContext, CommandRegistry, CommandResult, SlashCommand, StaticSlashCommand,
};
