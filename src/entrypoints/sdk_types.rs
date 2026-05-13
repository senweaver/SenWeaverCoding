// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// SDK types — mirrors claude-code-typescript-src/entrypoints/agentSdkTypes.ts.
// Public types for the programmatic SDK embedding API.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SdkStatus {
    Idle,
    Running,
    Waiting,
    Stopped,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkConfig {

    pub model: Option<String>,

    pub cwd: Option<PathBuf>,

    pub system_prompt: Option<String>,

    pub max_turns: Option<u32>,

    pub allowed_tools: Vec<String>,

    pub denied_tools: Vec<String>,

    pub mcp_servers: Vec<SdkMcpServer>,

    pub permission_mode: PermissionMode,

    pub structured_output_schema: Option<serde_json::Value>,

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
            base.mcp.servers = self.mcp_servers.iter().cloned().map(Into::into).collect();
        }

        base
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {

    #[default]
    Default,

    AutoApprove,

    DenyAll,

    PlanOnly,
}

impl PermissionMode {

    pub(crate) fn to_autonomy_level(self) -> crate::security::AutonomyLevel {
        use crate::security::AutonomyLevel;
        match self {
            Self::Default => AutonomyLevel::Supervised,
            Self::AutoApprove => AutonomyLevel::Full,
            Self::DenyAll | Self::PlanOnly => AutonomyLevel::ReadOnly,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkMcpServer {

    pub name: String,

    pub command: String,

    pub args: Vec<String>,

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
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkMessage {

    pub role: String,

    pub content: String,

    pub tool_calls: Vec<SdkToolCall>,

    pub metadata: Option<SdkMessageMetadata>,
}

impl SdkMessage {

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
            tool_calls: Vec::new(),
            metadata: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
            tool_calls: Vec::new(),
            metadata: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkToolCall {

    pub id: String,

    pub name: String,

    pub input: serde_json::Value,

    pub output: Option<String>,

    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkMessageMetadata {

    pub model: Option<String>,

    pub input_tokens: Option<u64>,

    pub output_tokens: Option<u64>,

    pub cost_usd: Option<f64>,

    pub duration_ms: Option<u64>,
}

impl SdkMessageMetadata {

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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SdkModelUsage {

    pub model: String,

    pub input_tokens: u64,

    pub output_tokens: u64,

    pub cache_creation_tokens: u64,

    pub cache_read_tokens: u64,

    pub total_cost_usd: f64,

    pub request_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {

    PreToolUse,

    PostToolUse,

    Notification,

    Stop,

    SubagentStop,
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum SdkTurnEvent {

    Chunk { delta: String },

    Thinking { delta: String },

    ToolCall {
        name: String,
        args: serde_json::Value,
    },

    ToolResult { name: String, output: String, success: bool },

    Error { message: String },
}

impl From<crate::agent::TurnEvent> for SdkTurnEvent {
    fn from(event: crate::agent::TurnEvent) -> Self {
        use crate::agent::TurnEvent as T;
        match event {
            T::Chunk { delta } => Self::Chunk { delta },
            T::Thinking { delta } => Self::Thinking { delta },
            T::ToolCall { name, args } => Self::ToolCall { name, args },
            T::ToolResult { name, output, success } => Self::ToolResult { name, output, success },
            T::Error { message } => Self::Error { message },

            T::FileEdit {
                path,
                additions,
                deletions,
                ..
            } => Self::ToolResult {
                name: "file_edit".into(),
                output: format!("{path} (+{additions}/-{deletions})"),
                success: true,
            },
            T::StatusUpdate { action, detail } => Self::Chunk {
                delta: format!("[{action}] {detail}"),
            },
            T::ProgressTick {
                iteration,
                max_iterations,
                tokens_used,
            } => Self::Chunk {
                delta: format!(
                    "[progress] iter {iteration}/{max_iterations} · {tokens_used} tokens"
                ),
            },
            T::CommandPreview { tool_name, .. } => Self::Chunk {
                delta: format!("[preview] {tool_name}"),
            },
            T::Cancelling { reason } => Self::Chunk {
                delta: format!("[cancelling] {reason}"),
            },
            T::ContextCompressed {
                tokens_before,
                tokens_after,
            } => Self::Chunk {
                delta: format!("[compressed] {tokens_before} → {tokens_after} tokens"),
            },
            T::PermissionRequest {
                tool_name,
                request_id,
                ..
            } => Self::Chunk {
                delta: format!("[permission_request:{request_id}] {tool_name}"),
            },
            T::SubagentChunk {
                task_id,
                agent_id,
                kind,
                delta,
            } => {

                let label = format!("[{agent_id}::{task_id}]");
                match kind {
                    crate::agent::SubagentChunkKind::Chunk => Self::Chunk {
                        delta: format!("{label} {delta}"),
                    },
                    crate::agent::SubagentChunkKind::Thinking => Self::Thinking {
                        delta: format!("{label} {delta}"),
                    },
                    crate::agent::SubagentChunkKind::ToolCall => Self::ToolCall {
                        name: format!("{label} {delta}"),
                        args: serde_json::Value::Null,
                    },
                    crate::agent::SubagentChunkKind::ToolResult => Self::ToolResult {
                        name: format!("{label} subagent_tool"),
                        output: delta,
                        success: true,
                    },
                    crate::agent::SubagentChunkKind::Status => Self::Chunk {
                        delta: format!("{label} [status] {delta}"),
                    },
                }
            }
            T::PiiSanitized { report } => Self::Chunk {
                delta: format!(
                    "[debug_pii_stats] redacted {} item(s)",
                    report.total()
                ),
            },
        }
    }
}

pub type SdkHookCallback = Box<dyn Fn(HookEvent, serde_json::Value) -> bool + Send + Sync>;

#[derive(Debug, Clone)]
pub struct SdkTurnResult {

    pub content: String,

    pub tool_calls: Vec<SdkToolCall>,

    pub metadata: SdkMessageMetadata,
}
