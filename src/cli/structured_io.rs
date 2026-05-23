// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StdinMessage {

    #[serde(rename = "user_message")]
    UserMessage {
        content: String,
        #[serde(default)]
        images: Vec<String>,
    },

    #[serde(rename = "control_response")]
    ControlResponse {
        request_id: String,
        #[serde(flatten)]
        payload: ControlResponsePayload,
    },

    #[serde(rename = "sdk_message")]
    SdkMessage {
        action: String,
        #[serde(default)]
        data: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "decision")]
pub enum ControlResponsePayload {
    #[serde(rename = "allow")]
    Allow {
        #[serde(default)]
        updated_input: Option<serde_json::Value>,
    },
    #[serde(rename = "deny")]
    Deny {
        #[serde(default)]
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StdoutMessage {

    #[serde(rename = "assistant_message")]
    AssistantMessage {
        content: String,
        #[serde(default)]
        stop_reason: Option<String>,
    },

    #[serde(rename = "tool_use")]
    ToolUse {
        tool_use_id: String,
        tool_name: String,
        input: serde_json::Value,
    },

    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        success: bool,
        output: String,
        #[serde(default)]
        error: Option<String>,
    },

    #[serde(rename = "control_request")]
    ControlRequest {
        request_id: String,
        #[serde(flatten)]
        payload: ControlRequestPayload,
    },

    #[serde(rename = "session_state")]
    SessionState {
        session_id: String,
        #[serde(default)]
        status: String,
        #[serde(default)]
        metadata: serde_json::Value,
    },

    #[serde(rename = "system")]
    System { content: String },

    #[serde(rename = "result")]
    Result {
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        cost: Option<f64>,
        #[serde(default)]
        duration_ms: Option<u64>,
        #[serde(default)]
        num_turns: Option<u32>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum ControlRequestPayload {

    #[serde(rename = "can_use_tool")]
    CanUseTool {
        tool_name: String,
        input: serde_json::Value,
        #[serde(default)]
        tool_use_id: Option<String>,
    },

    #[serde(rename = "session_state_changed")]
    SessionStateChanged {
        session_id: String,
        #[serde(default)]
        status: String,
    },

    #[serde(rename = "mcp_set_servers")]
    McpSetServers { servers: Vec<serde_json::Value> },
}

type PendingRequests = Arc<RwLock<HashMap<String, oneshot::Sender<ControlResponsePayload>>>>;

pub struct StructuredIO {
    pending: PendingRequests,
    inbox: mpsc::Receiver<StdinMessage>,
    _reader_handle: crate::runtime::TaskHandle,
}

impl StructuredIO {

    pub fn from_stdin() -> Self {
        let (tx, rx) = mpsc::channel(64);
        let pending: PendingRequests = Arc::new(RwLock::new(HashMap::new()));
        let pending_clone = Arc::clone(&pending);

        let handle = crate::runtime::spawn_supervised("cli.stdin_reader", async move {
            let stdin = tokio::io::stdin();
            let reader = BufReader::new(stdin);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                match serde_json::from_str::<StdinMessage>(&line) {
                    Ok(StdinMessage::ControlResponse {
                        request_id,
                        payload,
                    }) => {
                        let sender = pending_clone.write().remove(&request_id);
                        if let Some(sender) = sender {
                            let _ = sender.send(payload);
                        } else {
                            tracing::warn!(
                                request_id,
                                "Received control_response for unknown request"
                            );
                        }
                    }
                    Ok(msg) => {
                        if tx.send(msg).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::warn!(line = %line.chars().take(200).collect::<String>(), error = %e, "Failed to parse stdin NDJSON");
                    }
                }
            }
        });

        Self {
            pending,
            inbox: rx,
            _reader_handle: handle,
        }
    }

    pub fn from_reader<R: tokio::io::AsyncRead + Unpin + Send + 'static>(reader: R) -> Self {
        let (tx, rx) = mpsc::channel(64);
        let pending: PendingRequests = Arc::new(RwLock::new(HashMap::new()));
        let pending_clone = Arc::clone(&pending);

        let handle = crate::runtime::spawn_supervised("cli.reader", async move {
            let buf = BufReader::new(reader);
            let mut lines = buf.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                match serde_json::from_str::<StdinMessage>(&line) {
                    Ok(StdinMessage::ControlResponse {
                        request_id,
                        payload,
                    }) => {
                        let sender = pending_clone.write().remove(&request_id);
                        if let Some(sender) = sender {
                            let _ = sender.send(payload);
                        }
                    }
                    Ok(msg) => {
                        if tx.send(msg).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, "Skipping malformed stdin line");
                    }
                }
            }
        });

        Self {
            pending,
            inbox: rx,
            _reader_handle: handle,
        }
    }

    pub async fn recv(&mut self) -> Option<StdinMessage> {
        self.inbox.recv().await
    }

    pub fn write(&self, msg: &StdoutMessage) -> std::io::Result<()> {
        super::ndjson::write_ndjson_stdout(msg)
    }

    pub async fn request_permission(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
        tool_use_id: Option<&str>,
    ) -> anyhow::Result<ControlResponsePayload> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();

        self.pending.write().insert(request_id.clone(), tx);

        let msg = StdoutMessage::ControlRequest {
            request_id: request_id.clone(),
            payload: ControlRequestPayload::CanUseTool {
                tool_name: tool_name.to_string(),
                input: input.clone(),
                tool_use_id: tool_use_id.map(|s| s.to_string()),
            },
        };
        self.write(&msg)?;

        let response = tokio::time::timeout(std::time::Duration::from_secs(300), rx)
            .await
            .map_err(|_| {
                self.pending.write().remove(&request_id);
                anyhow::anyhow!("Permission request timed out after 300s")
            })?
            .map_err(|_| {
                self.pending.write().remove(&request_id);
                anyhow::anyhow!("Permission request channel closed")
            })?;

        Ok(response)
    }

    pub fn notify_session_state(
        &self,
        session_id: &str,
        status: &str,
        metadata: serde_json::Value,
    ) {
        let _ = self.write(&StdoutMessage::SessionState {
            session_id: session_id.to_string(),
            status: status.to_string(),
            metadata,
        });
    }

    pub fn emit_system(&self, content: &str) {
        let _ = self.write(&StdoutMessage::System {
            content: content.to_string(),
        });
    }

    pub fn pending_count(&self) -> usize {
        self.pending.read().len()
    }
}
