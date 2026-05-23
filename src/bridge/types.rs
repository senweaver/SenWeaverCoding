// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeStatus {
    Disconnected,
    Connecting,
    Connected,
    Paired,
    Error,
}

impl std::fmt::Display for BridgeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disconnected => write!(f, "disconnected"),
            Self::Connecting => write!(f, "connecting"),
            Self::Connected => write!(f, "connected"),
            Self::Paired => write!(f, "paired"),
            Self::Error => write!(f, "error"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeEvent {
    StatusChanged { status: BridgeStatus },
    MessageReceived { message: BridgeMessage },
    DevicePaired { device_id: String },
    DeviceDisconnected { device_id: String },
    SessionCreated { session_id: String },
    Error { error: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeMessage {
    pub id: String,
    pub session_id: String,
    pub sender: MessageSender,
    pub content: MessageContent,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSender {
    User,
    Agent,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageContent {
    Text {
        text: String,
    },
    ToolUse {
        tool_name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        output: String,
        is_error: bool,
    },
    Attachment {
        filename: String,
        media_type: String,
        data: Vec<u8>,
    },
    Command {
        name: String,
        args: Option<String>,
    },
    PermissionRequest {
        tool_name: String,
        description: String,
    },
    PermissionResponse {
        approved: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundAttachment {
    pub filename: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollConfig {
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub backoff_multiplier: f64,
    pub max_retries: Option<u32>,
    pub jitter_fraction: f64,
}

impl Default for PollConfig {
    fn default() -> Self {
        Self {
            initial_delay_ms: 1000,
            max_delay_ms: 30_000,
            backoff_multiplier: 1.5,
            max_retries: None,
            jitter_fraction: 0.1,
        }
    }
}
