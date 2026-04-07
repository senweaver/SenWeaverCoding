// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Structured I/O — NDJSON protocol for SDK / headless communication.
//!
//! Mirrors cc-typescript-src's `cli/structuredIO.ts`. Reads NDJSON messages
//! from stdin and writes NDJSON messages to stdout. Implements the control
//! protocol (permission requests/responses, session state, MCP lifecycle)
//! that allows external tools and IDEs to drive the agent.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, oneshot};

// ── Message types (stdin → agent) ──────────────────────────────────

/// Messages received from stdin (SDK host → agent).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StdinMessage {
    /// A user message to send to the model.
    #[serde(rename = "user_message")]
    UserMessage {
        content: String,
        #[serde(default)]
        images: Vec<String>,
    },
    /// Response to a control_request (permission decision, etc.).
    #[serde(rename = "control_response")]
    ControlResponse {
        request_id: String,
        #[serde(flatten)]
        payload: ControlResponsePayload,
    },
    /// SDK-specific message (configuration, lifecycle).
    #[serde(rename = "sdk_message")]
    SdkMessage {
        action: String,
        #[serde(default)]
        data: serde_json::Value,
    },
}

/// Payload for control responses.
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

// ── Message types (agent → stdout) ─────────────────────────────────

/// Messages written to stdout (agent → SDK host).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StdoutMessage {
    /// Assistant text response.
    #[serde(rename = "assistant_message")]
    AssistantMessage {
        content: String,
        #[serde(default)]
        stop_reason: Option<String>,
    },
    /// Tool use request.
    #[serde(rename = "tool_use")]
    ToolUse {
        tool_use_id: String,
        tool_name: String,
        input: serde_json::Value,
    },
    /// Tool execution result.
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        success: bool,
        output: String,
        #[serde(default)]
        error: Option<String>,
    },
    /// Control request requiring SDK host action (e.g. permission prompt).
    #[serde(rename = "control_request")]
    ControlRequest {
        request_id: String,
        #[serde(flatten)]
        payload: ControlRequestPayload,
    },
    /// Session state update.
    #[serde(rename = "session_state")]
    SessionState {
        session_id: String,
        #[serde(default)]
        status: String,
        #[serde(default)]
        metadata: serde_json::Value,
    },
    /// System/debug message.
    #[serde(rename = "system")]
    System { content: String },
    /// Stream result for streaming mode.
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

/// Payload for control requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum ControlRequestPayload {
    /// Ask the SDK host to approve a tool use.
    #[serde(rename = "can_use_tool")]
    CanUseTool {
        tool_name: String,
        input: serde_json::Value,
        #[serde(default)]
        tool_use_id: Option<String>,
    },
    /// Notify the SDK host that session state changed.
    #[serde(rename = "session_state_changed")]
    SessionStateChanged {
        session_id: String,
        #[serde(default)]
        status: String,
    },
    /// MCP server lifecycle event.
    #[serde(rename = "mcp_set_servers")]
    McpSetServers { servers: Vec<serde_json::Value> },
}

// ── Pending request tracking ───────────────────────────────────────

type PendingRequests = Arc<RwLock<HashMap<String, oneshot::Sender<ControlResponsePayload>>>>;

// ── StructuredIO ───────────────────────────────────────────────────

/// The core structured I/O driver.
///
/// Reads NDJSON from an async reader, dispatches messages to the
/// appropriate handler, and writes NDJSON to stdout. Pending
/// control_request/control_response pairs are tracked for
/// request-response correlation.
pub struct StructuredIO {
    pending: PendingRequests,
    inbox: mpsc::Receiver<StdinMessage>,
    _reader_handle: tokio::task::JoinHandle<()>,
}

impl StructuredIO {
    /// Create a new StructuredIO reading from tokio stdin.
    pub fn from_stdin() -> Self {
        let (tx, rx) = mpsc::channel(64);
        let pending: PendingRequests = Arc::new(RwLock::new(HashMap::new()));
        let pending_clone = Arc::clone(&pending);

        let handle = tokio::spawn(async move {
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

    /// Create a StructuredIO from an arbitrary async reader (for testing / remote).
    pub fn from_reader<R: tokio::io::AsyncRead + Unpin + Send + 'static>(reader: R) -> Self {
        let (tx, rx) = mpsc::channel(64);
        let pending: PendingRequests = Arc::new(RwLock::new(HashMap::new()));
        let pending_clone = Arc::clone(&pending);

        let handle = tokio::spawn(async move {
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

    /// Receive the next non-control message from the inbox.
    pub async fn recv(&mut self) -> Option<StdinMessage> {
        self.inbox.recv().await
    }

    /// Write a structured message to stdout as NDJSON.
    pub fn write(&self, msg: &StdoutMessage) -> std::io::Result<()> {
        super::ndjson::write_ndjson_stdout(msg)
    }

    /// Send a control_request and wait for the matching control_response.
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

    /// Emit a session state update.
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

    /// Emit a system/debug message.
    pub fn emit_system(&self, content: &str) {
        let _ = self.write(&StdoutMessage::System {
            content: content.to_string(),
        });
    }

    /// Number of pending control requests awaiting responses.
    pub fn pending_count(&self) -> usize {
        self.pending.read().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_message_deserializes() {
        let json = r#"{"type":"user_message","content":"hello","images":[]}"#;
        let msg: StdinMessage = serde_json::from_str(json).unwrap();
        match msg {
            StdinMessage::UserMessage { content, .. } => assert_eq!(content, "hello"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn control_response_allow_deserializes() {
        let json = r#"{"type":"control_response","request_id":"r1","decision":"allow"}"#;
        let msg: StdinMessage = serde_json::from_str(json).unwrap();
        match msg {
            StdinMessage::ControlResponse {
                request_id,
                payload,
            } => {
                assert_eq!(request_id, "r1");
                assert!(matches!(payload, ControlResponsePayload::Allow { .. }));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn assistant_message_serializes() {
        let msg = StdoutMessage::AssistantMessage {
            content: "hi".into(),
            stop_reason: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("assistant_message"));
        assert!(json.contains("hi"));
    }

    #[test]
    fn tool_use_serializes() {
        let msg = StdoutMessage::ToolUse {
            tool_use_id: "t1".into(),
            tool_name: "shell".into(),
            input: serde_json::json!({"command": "ls"}),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("tool_use"));
        assert!(json.contains("shell"));
    }

    #[test]
    fn control_request_serializes() {
        let msg = StdoutMessage::ControlRequest {
            request_id: "req-1".into(),
            payload: ControlRequestPayload::CanUseTool {
                tool_name: "file_write".into(),
                input: serde_json::json!({"path": "test.txt"}),
                tool_use_id: Some("tu-1".into()),
            },
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("can_use_tool"));
        assert!(json.contains("file_write"));
    }

    #[tokio::test]
    async fn structured_io_from_reader() {
        let input = r#"{"type":"user_message","content":"test","images":[]}"#;
        let cursor = std::io::Cursor::new(format!("{input}\n"));
        let mut io = StructuredIO::from_reader(cursor);
        let msg = io.recv().await.unwrap();
        match msg {
            StdinMessage::UserMessage { content, .. } => assert_eq!(content, "test"),
            _ => panic!("wrong variant"),
        }
    }

    #[tokio::test]
    async fn structured_io_skips_empty_lines() {
        let input = "\n\n{\"type\":\"user_message\",\"content\":\"x\",\"images\":[]}\n\n";
        let cursor = std::io::Cursor::new(input);
        let mut io = StructuredIO::from_reader(cursor);
        let msg = io.recv().await.unwrap();
        match msg {
            StdinMessage::UserMessage { content, .. } => assert_eq!(content, "x"),
            _ => panic!("wrong variant"),
        }
    }
}
