// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! ACP (Agent Control Protocol) Server — JSON-RPC 2.0 over stdio.
//!
//! Provides an IDE-friendly interface for spawning and managing isolated agent
//! sessions. Each session wraps an [`Agent`] built from the global config with
//! streaming support via JSON-RPC notifications.
//!
//! ## Protocol
//!
//! Requests and responses are newline-delimited JSON objects on stdin/stdout.
//!
//! | Method            | Description                              |
//! |-------------------|------------------------------------------|
//! | `initialize`      | Handshake — returns server capabilities  |
//! | `session/new`     | Create an isolated agent session          |
//! | `session/prompt`  | Send a prompt, stream back events         |
//! | `session/stop`    | Gracefully terminate a session            |

use crate::agent::agent::{Agent, TurnEvent};
use crate::config::Config;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AcpServerConfig {

    pub max_sessions: usize,

    pub session_timeout_secs: u64,
}

impl Default for AcpServerConfig {
    fn default() -> Self {
        Self {
            max_sessions: 10,
            session_timeout_secs: 3600,
        }
    }
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    #[serde(default)]
    params: Value,
    id: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
    id: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcNotification {
    jsonrpc: &'static str,
    method: &'static str,
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

const PARSE_ERROR: i32 = -32700;
const INVALID_REQUEST: i32 = -32600;
const METHOD_NOT_FOUND: i32 = -32601;
const INVALID_PARAMS: i32 = -32602;
const INTERNAL_ERROR: i32 = -32603;

const SESSION_NOT_FOUND: i32 = -32000;
const SESSION_LIMIT_REACHED: i32 = -32001;

struct Session {
    agent: Agent,
    created_at: Instant,
    last_active: Instant,
    workspace_dir: String,
}

pub struct AcpServer {
    config: Config,
    acp_config: AcpServerConfig,
    sessions: Arc<Mutex<HashMap<String, Session>>>,
}

impl AcpServer {
    pub fn new(config: Config, acp_config: AcpServerConfig) -> Self {
        Self {
            config,
            acp_config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn run(&self) -> Result<()> {
        info!(
            "ACP server starting (max_sessions={}, timeout={}s)",
            self.acp_config.max_sessions, self.acp_config.session_timeout_secs
        );

        let stdin = tokio::io::stdin();
        let mut reader = BufReader::new(stdin);
        let mut line = String::new();

        let sessions = Arc::clone(&self.sessions);
        let timeout = Duration::from_secs(self.acp_config.session_timeout_secs);
        let _handle =
            crate::runtime::spawn_supervised("channels.acp_server.session_reaper", async move {
                let mut interval = tokio::time::interval(Duration::from_secs(60));
                loop {
                    interval.tick().await;
                    let mut sessions = sessions.lock().await;
                    let before = sessions.len();
                    sessions.retain(|id, session| {
                        let expired = session.last_active.elapsed() > timeout;
                        if expired {
                            info!("Session {id} expired after inactivity");
                        }
                        !expired
                    });
                    let reaped = before - sessions.len();
                    if reaped > 0 {
                        debug!("Reaped {reaped} expired session(s)");
                    }
                }
            });

        loop {
            line.clear();
            let bytes_read = reader.read_line(&mut line).await?;
            if bytes_read == 0 {
                info!("ACP server: stdin closed, shutting down");
                break;
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            match serde_json::from_str::<JsonRpcRequest>(trimmed) {
                Ok(request) => {
                    if request.jsonrpc != "2.0" {
                        if let Some(id) = request.id {
                            self.write_error(id, INVALID_REQUEST, "Invalid JSON-RPC version")
                                .await;
                        }
                        continue;
                    }
                    self.handle_request(request).await;
                }
                Err(e) => {
                    warn!("Failed to parse JSON-RPC request: {e}");
                    self.write_error(Value::Null, PARSE_ERROR, &format!("Parse error: {e}"))
                        .await;
                }
            }
        }

        Ok(())
    }

    async fn handle_request(&self, request: JsonRpcRequest) {
        let id = request.id.clone().unwrap_or(Value::Null);
        let is_notification = request.id.is_none();

        let result = match request.method.as_str() {
            "initialize" => self.handle_initialize(&request.params),
            "session/new" => self.handle_session_new(&request.params).await,
            "session/prompt" => self.handle_session_prompt(&request.params, &id).await,
            "session/stop" => self.handle_session_stop(&request.params).await,
            _ => Err(RpcError {
                code: METHOD_NOT_FOUND,
                message: format!("Method not found: {}", request.method),
                data: None,
            }),
        };

        if !is_notification {
            match result {
                Ok(value) => self.write_result(id, value).await,
                Err(e) => self.write_error(id, e.code, &e.message).await,
            }
        }
    }

    fn handle_initialize(&self, _params: &Value) -> RpcResult {
        Ok(serde_json::json!({
            "protocolVersion": "1.0",
            "serverInfo": {
                "name": "sen-acp",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "capabilities": {
                "streaming": true,
                "maxSessions": self.acp_config.max_sessions,
                "sessionTimeoutSecs": self.acp_config.session_timeout_secs,
            },
            "methods": [
                "initialize",
                "session/new",
                "session/prompt",
                "session/stop",
            ],
        }))
    }

    async fn handle_session_new(&self, params: &Value) -> RpcResult {
        let mut sessions = self.sessions.lock().await;

        if sessions.len() >= self.acp_config.max_sessions {
            return Err(RpcError {
                code: SESSION_LIMIT_REACHED,
                message: format!(
                    "Maximum session limit reached ({})",
                    self.acp_config.max_sessions
                ),
                data: None,
            });
        }

        let workspace_dir = params
            .get("cwd")
            .or_else(|| params.get("workspaceDir"))
            .or_else(|| params.get("workspace_dir"))
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| self.config.workspace_dir.to_str().unwrap_or("."))
            .to_string();

        let session_id = Uuid::new_v4().to_string();

        let agent = Agent::from_config(&self.config, None, None)
            .await
            .map_err(|e| RpcError {
                code: INTERNAL_ERROR,
                message: format!("Failed to create agent: {e}"),
                data: None,
            })?;

        let now = Instant::now();
        sessions.insert(
            session_id.clone(),
            Session {
                agent,
                created_at: now,
                last_active: now,
                workspace_dir: workspace_dir.clone(),
            },
        );

        info!("Created session {session_id} (workspace: {workspace_dir})");

        Ok(serde_json::json!({
            "sessionId": session_id,
            "workspaceDir": workspace_dir,
        }))
    }

    async fn handle_session_prompt(&self, params: &Value, _request_id: &Value) -> RpcResult {
        let session_id = params
            .get("sessionId")
            .or_else(|| params.get("session_id"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError {
                code: INVALID_PARAMS,
                message: "Missing required parameter: sessionId".to_string(),
                data: None,
            })?
            .to_string();

        let prompt = params
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError {
                code: INVALID_PARAMS,
                message: "Missing required parameter: prompt".to_string(),
                data: None,
            })?
            .to_string();

        let mut session = {
            let mut sessions = self.sessions.lock().await;
            sessions.remove(&session_id).ok_or_else(|| RpcError {
                code: SESSION_NOT_FOUND,
                message: format!("Session not found: {session_id}"),
                data: None,
            })?
        };

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(100);
        let (session_tx, session_rx) = tokio::sync::oneshot::channel();
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();

        let sid = session_id.clone();

        crate::runtime::task_manager::spawn_supervised("acp.session_turn", async move {
            let result = session.agent.turn_streamed(&prompt, event_tx).await;
            let _ = session_tx.send(session);
            let _ = result_tx.send(result);
        });

        while let Some(event) = event_rx.recv().await {
            let notification = match &event {
                TurnEvent::Chunk { delta } => JsonRpcNotification {
                    jsonrpc: "2.0",
                    method: "session/event",
                    params: serde_json::json!({
                        "sessionId": session_id,
                        "type": "chunk",
                        "content": delta,
                    }),
                },
                TurnEvent::ToolCall { name, args } => JsonRpcNotification {
                    jsonrpc: "2.0",
                    method: "session/event",
                    params: serde_json::json!({
                        "sessionId": session_id,
                        "type": "tool_call",
                        "name": name,
                        "args": args,
                    }),
                },
                TurnEvent::ToolResult { name, output } => JsonRpcNotification {
                    jsonrpc: "2.0",
                    method: "session/event",
                    params: serde_json::json!({
                        "sessionId": session_id,
                        "type": "tool_result",
                        "name": name,
                        "output": output,
                    }),
                },
                TurnEvent::Thinking { delta } => JsonRpcNotification {
                    jsonrpc: "2.0",
                    method: "session/event",
                    params: serde_json::json!({
                        "sessionId": session_id,
                        "type": "thinking",
                        "content": delta,
                    }),
                },
                TurnEvent::Error { message } => JsonRpcNotification {
                    jsonrpc: "2.0",
                    method: "session/event",
                    params: serde_json::json!({
                        "sessionId": session_id,
                        "type": "error",
                        "content": message,
                    }),
                },

                TurnEvent::FileEdit {
                    path,
                    additions,
                    deletions,
                    diff,
                    edit_batch_id,
                } => JsonRpcNotification {
                    jsonrpc: "2.0",
                    method: "session/event",
                    params: serde_json::json!({
                        "sessionId": session_id,
                        "type": "file_edit",
                        "path": path,
                        "additions": additions,
                        "deletions": deletions,
                        "diff": diff,
                        "editBatchId": edit_batch_id,
                    }),
                },
                TurnEvent::StatusUpdate { action, detail } => JsonRpcNotification {
                    jsonrpc: "2.0",
                    method: "session/event",
                    params: serde_json::json!({
                        "sessionId": session_id,
                        "type": "status",
                        "action": action,
                        "detail": detail,
                    }),
                },
                TurnEvent::ProgressTick {
                    iteration,
                    max_iterations,
                    tokens_used,
                } => JsonRpcNotification {
                    jsonrpc: "2.0",
                    method: "session/event",
                    params: serde_json::json!({
                        "sessionId": session_id,
                        "type": "progress",
                        "iteration": iteration,
                        "max_iterations": max_iterations,
                        "tokens_used": tokens_used,
                    }),
                },
                TurnEvent::CommandPreview {
                    tool_name,
                    args,
                    estimated_duration_ms,
                } => JsonRpcNotification {
                    jsonrpc: "2.0",
                    method: "session/event",
                    params: serde_json::json!({
                        "sessionId": session_id,
                        "type": "command_preview",
                        "tool_name": tool_name,
                        "args": args,
                        "estimated_duration_ms": estimated_duration_ms,
                    }),
                },
                TurnEvent::Cancelling { reason } => JsonRpcNotification {
                    jsonrpc: "2.0",
                    method: "session/event",
                    params: serde_json::json!({
                        "sessionId": session_id,
                        "type": "cancelling",
                        "reason": reason,
                    }),
                },
                TurnEvent::ContextCompressed {
                    tokens_before,
                    tokens_after,
                } => JsonRpcNotification {
                    jsonrpc: "2.0",
                    method: "session/event",
                    params: serde_json::json!({
                        "sessionId": session_id,
                        "type": "context_compressed",
                        "tokens_before": tokens_before,
                        "tokens_after": tokens_after,
                    }),
                },
                TurnEvent::SubagentChunk {
                    task_id,
                    agent_id,
                    kind,
                    delta,
                } => JsonRpcNotification {
                    jsonrpc: "2.0",
                    method: "session/event",
                    params: serde_json::json!({
                        "sessionId": session_id,
                        "type": "subagent_chunk",
                        "taskId": task_id,
                        "agentId": agent_id,
                        "kind": format!("{kind:?}").to_lowercase(),
                        "content": delta,
                    }),
                },
                TurnEvent::PermissionRequest {
                    request_id,
                    tool_name,
                    input,
                    description,
                } => JsonRpcNotification {
                    jsonrpc: "2.0",
                    method: "session/event",
                    params: serde_json::json!({
                        "sessionId": session_id,
                        "type": "permission_request",
                        "requestId": request_id,
                        "toolName": tool_name,
                        "input": input,
                        "description": description,
                    }),
                },
            };
            self.write_notification(&notification).await;
        }

        let mut session = match session_rx.await {
            Ok(s) => s,
            Err(_) => {
                return Err(RpcError {
                    code: INTERNAL_ERROR,
                    message: "Agent session task panicked or was cancelled".to_string(),
                    data: None,
                });
            }
        };

        let result = match result_rx.await {
            Ok(r) => r,
            Err(_) => {
                return Err(RpcError {
                    code: INTERNAL_ERROR,
                    message: "Agent result channel closed unexpectedly".to_string(),
                    data: None,
                });
            }
        };

        let result = result.map_err(|e| RpcError {
            code: INTERNAL_ERROR,
            message: format!("Agent turn failed: {e}"),
            data: None,
        })?;

        {
            session.last_active = Instant::now();
            let mut sessions = self.sessions.lock().await;
            sessions.insert(sid, session);
        }

        Ok(serde_json::json!({
            "sessionId": session_id,
            "content": result,
        }))
    }

    async fn handle_session_stop(&self, params: &Value) -> RpcResult {
        let session_id = params
            .get("sessionId")
            .or_else(|| params.get("session_id"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError {
                code: INVALID_PARAMS,
                message: "Missing required parameter: sessionId".to_string(),
                data: None,
            })?;

        let mut sessions = self.sessions.lock().await;
        if sessions.remove(session_id).is_some() {
            info!("Stopped session {session_id}");
            Ok(serde_json::json!({
                "sessionId": session_id,
                "stopped": true,
            }))
        } else {
            Err(RpcError {
                code: SESSION_NOT_FOUND,
                message: format!("Session not found: {session_id}"),
                data: None,
            })
        }
    }

    async fn write_result(&self, id: Value, result: Value) {
        let response = JsonRpcResponse {
            jsonrpc: "2.0",
            result: Some(result),
            error: None,
            id,
        };
        self.write_json(&response).await;
    }

    async fn write_error(&self, id: Value, code: i32, message: &str) {
        let response = JsonRpcResponse {
            jsonrpc: "2.0",
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.to_string(),
                data: None,
            }),
            id,
        };
        self.write_json(&response).await;
    }

    async fn write_notification(&self, notification: &JsonRpcNotification) {
        self.write_json(notification).await;
    }

    async fn write_json<T: Serialize>(&self, value: &T) {
        match serde_json::to_string(value) {
            Ok(json) => {
                let mut stdout = tokio::io::stdout();

                if let Err(e) = stdout.write_all(json.as_bytes()).await {
                    error!("Failed to write to stdout: {e}");
                    return;
                }
                if let Err(e) = stdout.write_all(b"\n").await {
                    error!("Failed to write newline to stdout: {e}");
                    return;
                }
                if let Err(e) = stdout.flush().await {
                    error!("Failed to flush stdout: {e}");
                }
            }
            Err(e) => {
                error!("Failed to serialize JSON-RPC message: {e}");
            }
        }
    }
}

struct RpcError {
    code: i32,
    message: String,
    data: Option<Value>,
}

type RpcResult = std::result::Result<Value, RpcError>;
