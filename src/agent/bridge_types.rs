// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Shared bridge types for communication between UI frontends (TUI / GUI)
//! and the agent runtime. Both `tui` and `gui` features reference these types.

/// Message sent from a UI frontend to the agent task.
#[derive(Debug, Clone)]
pub enum UserInput {
    /// A chat message to send to the model.
    Chat(String),
    /// A slash command (without the leading `/`).
    SlashCommand { name: String, args: Vec<String> },
    /// Cancel the current operation.
    Cancel,
}

/// Event sent from the agent task back to a UI frontend.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// Assistant text response (may arrive incrementally).
    AssistantMessage(String),
    /// The agent is invoking a tool.
    ToolUse { name: String, id: String },
    /// A tool has produced output.
    ToolResult {
        id: String,
        output: String,
        success: bool,
    },
    /// The agent is thinking / processing.
    Thinking,
    /// The current turn is complete.
    Done,
    /// An error occurred.
    Error(String),
    /// Output from a slash command.
    CommandOutput(String),
}
