// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// SDK types — mirrors claude-code-typescript-src/entrypoints/agentSdkTypes.ts.
// Public types for the programmatic SDK embedding API.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// SDK session status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SdkStatus {
    Idle,
    Running,
    Waiting,
    Stopped,
    Error,
}

/// SDK configuration for creating an agent session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkConfig {
    /// Model to use (e.g. "anthropic/claude-sonnet-4-20250514").
    /// When None, uses the model from the config file.
    pub model: Option<String>,
    /// Working directory for the agent. Defaults to the current process directory.
    pub cwd: Option<PathBuf>,
    /// Additional system prompt content appended to the default prompt.
    pub system_prompt: Option<String>,
    /// Maximum number of agent tool-call iterations per message.
    /// When None, uses the default from the config file.
    pub max_turns: Option<u32>,
    /// Allowlist of tool names the agent may use. Empty = all tools allowed.
    pub allowed_tools: Vec<String>,
    /// Denylist of tool names the agent may NOT use.
    pub denied_tools: Vec<String>,
    /// MCP servers to connect for SDK-specific tool access.
    pub mcp_servers: Vec<SdkMcpServer>,
    /// Controls how the agent handles tool execution permissions.
    pub permission_mode: PermissionMode,
    /// JSON Schema for structured output (provider must support it).
    pub structured_output_schema: Option<serde_json::Value>,
    /// Free-form key-value metadata passed through to hooks and logging.
    pub metadata: HashMap<String, String>,
}

impl Default for SdkConfig {
    fn default() -> Self {
        Self {
            model: None,
            cwd: None,
            system_prompt: None,
            max_turns: None,
            allowed_tools: Vec::new(),
            denied_tools: Vec::new(),
            mcp_servers: Vec::new(),
            permission_mode: PermissionMode::default(),
            structured_output_schema: None,
            metadata: HashMap::new(),
        }
    }
}

impl SdkConfig {
    /// Apply SDK configuration overrides to a loaded `Config`.
    ///
    /// Returns a clone of `base` with fields overridden by this `SdkConfig`.
    /// Priority: explicit SDK fields (Some) override the base config.
    pub fn apply_to_config(&self, mut base: crate::config::Config) -> crate::config::Config {
        if let Some(model) = &self.model {
            base.default_model = Some(model.clone());
        }
        if let Some(cwd) = &self.cwd {
            base.workspace_dir = cwd.clone();
        }
        if let Some(max_turns) = self.max_turns {
            base.agent.max_tool_iterations = max_turns as usize;
        }
        base.autonomy.level = self.permission_mode.to_autonomy_level();

        if !self.mcp_servers.is_empty() {
            base.mcp.enabled = true;
            base.mcp.deferred_loading = true;
            base.mcp.servers = self
                .mcp_servers
                .iter()
                .cloned()
                .map(Into::into)
                .collect();
        }

        base
    }
}

/// Permission mode for tool execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    /// Default: supervised mode — asks for approval on destructive operations.
    #[default]
    Default,
    /// Auto-approve every tool call (use with caution).
    AutoApprove,
    /// Deny all tool executions (read-only agent).
    DenyAll,
    /// Plan-only mode — generates plans but does not execute any tools.
    PlanOnly,
}

impl PermissionMode {
    /// Maps the SDK-facing permission mode to the internal autonomy level.
    pub(crate) fn to_autonomy_level(self) -> crate::security::AutonomyLevel {
        use crate::security::AutonomyLevel;
        match self {
            Self::Default => AutonomyLevel::Supervised,
            Self::AutoApprove => AutonomyLevel::Full,
            Self::DenyAll | Self::PlanOnly => AutonomyLevel::ReadOnly,
        }
    }
}

/// An MCP server configuration for SDK usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkMcpServer {
    /// Display name used as a tool prefix (`<name>__<tool>`).
    pub name: String,
    /// Executable to spawn (e.g. "npx", "uvx").
    pub command: String,
    /// Command-line arguments (e.g. ["-m", "mcp-server-example"]).
    pub args: Vec<String>,
    /// Environment variables passed to the subprocess.
    pub env: HashMap<String, String>,
}

impl From<SdkMcpServer> for crate::config::McpServerConfig {
    fn from(sdk: SdkMcpServer) -> Self {
        Self {
            name: sdk.name,
            transport: crate::config::McpTransport::Stdio,
            url: None,
            command: sdk.command,
            args: sdk.args,
            env: sdk.env,
            headers: HashMap::new(),
            tool_timeout_secs: None,
        }
    }
}

/// A message in the SDK API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkMessage {
    /// Message role — "user" or "assistant".
    pub role: String,
    /// Text content of the message.
    pub content: String,
    /// Tool calls made during this turn (populated in assistant responses).
    pub tool_calls: Vec<SdkToolCall>,
    /// Usage and timing metadata for this message.
    pub metadata: Option<SdkMessageMetadata>,
}

impl SdkMessage {
    /// Create a new user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
            tool_calls: Vec::new(),
            metadata: None,
        }
    }

    /// Create a new assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
            tool_calls: Vec::new(),
            metadata: None,
        }
    }
}

/// A tool call made by the agent during a turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkToolCall {
    /// Unique identifier for this tool call (matches the tool result).
    pub id: String,
    /// Name of the tool invoked.
    pub name: String,
    /// Tool input arguments as a JSON object.
    pub input: serde_json::Value,
    /// Tool output as a string (None if still in flight or errored).
    pub output: Option<String>,
    /// Whether the tool call returned an error.
    pub is_error: bool,
}

/// Metadata attached to SDK messages (usage, cost, timing).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkMessageMetadata {
    /// Model that generated this response.
    pub model: Option<String>,
    /// Number of input tokens consumed.
    pub input_tokens: Option<u64>,
    /// Number of output tokens generated.
    pub output_tokens: Option<u64>,
    /// Estimated cost in USD.
    pub cost_usd: Option<f64>,
    /// Wall-clock time in milliseconds for this turn.
    pub duration_ms: Option<u64>,
}

impl SdkMessageMetadata {
    /// Create metadata with only duration (used when usage info is unavailable).
    fn with_duration(duration_ms: u64, model: Option<String>) -> Self {
        Self {
            model,
            input_tokens: None,
            output_tokens: None,
            cost_usd: None,
            duration_ms: Some(duration_ms),
        }
    }
}

/// Model usage statistics for SDK consumers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SdkModelUsage {
    /// Model name.
    pub model: String,
    /// Total input tokens across all turns in the session.
    pub input_tokens: u64,
    /// Total output tokens across all turns in the session.
    pub output_tokens: u64,
    /// Tokens consumed during cache creation.
    pub cache_creation_tokens: u64,
    /// Tokens served from provider cache.
    pub cache_read_tokens: u64,
    /// Total estimated cost in USD.
    pub total_cost_usd: f64,
    /// Total number of LLM API requests made.
    pub request_count: u64,
}

/// Hook events that SDK consumers can register callbacks for.
///
/// Note: callbacks are invoked synchronously during agent execution.
/// Keep callbacks fast and non-blocking to avoid degrading agent performance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    /// Fired before a tool is executed. Return `false` to deny the call.
    PreToolUse,
    /// Fired after a tool returns (success or failure).
    PostToolUse,
    /// Fired when the agent emits a notification message.
    Notification,
    /// Fired when the agent session is stopped.
    Stop,
    /// Fired when a sub-agent session ends.
    SubagentStop,
}

/// Events emitted during a streamed `send_message_streamed` turn.
///
/// These mirror `agent::TurnEvent` for SDK consumers.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum SdkTurnEvent {
    /// A text chunk from the LLM response.
    Chunk { delta: String },
    /// A reasoning/thinking chunk from a thinking-enabled model.
    Thinking { delta: String },
    /// The agent is about to invoke a tool.
    ToolCall { name: String, args: serde_json::Value },
    /// A tool has returned a result.
    ToolResult { name: String, output: String },
}

impl From<crate::agent::TurnEvent> for SdkTurnEvent {
    fn from(event: crate::agent::TurnEvent) -> Self {
        use crate::agent::TurnEvent as T;
        match event {
            T::Chunk { delta } => Self::Chunk { delta },
            T::Thinking { delta } => Self::Thinking { delta },
            T::ToolCall { name, args } => Self::ToolCall { name, args },
            T::ToolResult { name, output } => Self::ToolResult { name, output },
        }
    }
}

/// A hook callback registered by an SDK consumer.
///
/// The boolean return value: `true` = allow, `false` = deny (for `PreToolUse`).
pub type SdkHookCallback = Box<dyn Fn(HookEvent, serde_json::Value) -> bool + Send + Sync>;

/// Result of a completed agent turn, including usage metadata.
#[derive(Debug, Clone)]
pub struct SdkTurnResult {
    /// The final text response from the agent.
    pub content: String,
    /// Tool calls made during this turn.
    pub tool_calls: Vec<SdkToolCall>,
    /// Usage statistics for this turn.
    pub metadata: SdkMessageMetadata,
}
