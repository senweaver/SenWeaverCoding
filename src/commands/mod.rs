// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Commands module — mirrors claude-code's `commands/` and `commands.ts`.
//
// Provides slash-command infrastructure: registration, discovery,
// filtering by availability/context, and execution. Each submodule
// implements one or more slash commands.

pub mod self_test;
pub mod update;

// -- New commands ported from claude-code-typescript-src--
pub mod add_dir;
pub mod clear;
pub mod color;
pub mod compact;
pub mod config_cmd;
pub mod context;
pub mod cost;
pub mod diff;
pub mod doctor_cmd;
pub mod effort;
pub mod export;
pub mod fast;
pub mod help;
pub mod history;
pub mod hooks;
pub mod memory_cmd;
pub mod mode;
pub mod model;
pub mod permissions;
pub mod plan;
pub mod plugin_cmd;
pub mod registry;
pub mod resume;
pub mod review;
pub mod skills_cmd;
pub mod stats;
pub mod status;
pub mod tasks_cmd;
pub mod theme;
pub mod vim;
pub mod voice_cmd;

#[allow(unused_imports)]
pub use registry::{CommandContext, CommandRegistry, CommandResult, SlashCommand};
