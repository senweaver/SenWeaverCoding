// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Entrypoints module — mirrors claude-code's `entrypoints/` directory.
//
// Provides multiple entry points for the agent runtime:
// - CLI: interactive terminal REPL
// - MCP: Model Context Protocol server mode
// - SDK: programmatic embedding API
// - Init: project initialization and setup

pub mod cli;
pub mod init;
pub mod mcp_server;
pub mod sdk;
pub mod sdk_types;
pub mod session_driven;

pub use cli::CliEntrypoint;
pub use init::InitEntrypoint;
pub use mcp_server::McpServerEntrypoint;
pub use sdk::SdkEntrypoint;
pub use sdk::SdkSession;
pub use sdk::SdkToolCallBuilder;
pub use sdk_types::{
    HookEvent, PermissionMode, SdkConfig, SdkHookCallback, SdkMcpServer, SdkMessage, SdkModelUsage,
    SdkStatus, SdkToolCall, SdkTurnEvent, SdkTurnResult,
};
