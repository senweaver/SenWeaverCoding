// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//!
//! RPC method handlers and shared context.
//!
//! Each method handler takes the parsed `Value` params and returns a
//! `RpcResult<Value>` (compatible with the codec layer's error type).
//!
//! ## Adding a new method
//!
//! 1. Add the method name string constant to `METHODS`.
//! 2. Implement an async handler fn: `async fn handle_<name>(&self, params) -> RpcResult`.
//! 3. Add the match arm in `RpcCtx::handle_method`.

use crate::agent::agent::{Agent, TurnEvent};
use crate::config::Config;
use crate::memory::blackboard::Blackboard;
use crate::rpc::codec::{JsonRpcNotification, RpcError};
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use tokio::time::timeout;
use tracing::info;
use uuid::Uuid;

/// Result alias matching the codec error type.
pub type RpcResult<T = Value> = std::result::Result<T, RpcError>;

// ── Method registry ───────────────────────────────────────────────────────────

/// All supported method names, returned in `initialize.capabilities.methods`.
pub const METHODS: &[&str] = &[
    "initialize",
    "session/new",
    "session/prompt",
    "session/prompt_stream",
    "session/stop",
    "session/list",
    "session/kill",
    "system/info",
    "system/health",
    "tool/list",
    "tool/exec",
    "memory/store",
    "memory/recall",
    "blackboard/put",
    "blackboard/get",
    "blackboard/list",
    "blackboard/watch",
    "blackboard/unwatch",
];

// ── Session ──────────────────────────────────────────────────────────────────

pub(crate) struct Session {
    pub(crate) agent: Agent,
    pub(crate) created_at: Instant,
    pub(crate) last_active: Instant,
    pub(crate) workspace_dir: String,
}

// ── Shared mutable RPC state ─────────────────────────────────────────────────

pub struct RpcState {
    pub(crate) sessions: Arc<Mutex<std::collections::HashMap<String, Session>>>,
    pub session_timeout: Duration,
    pub max_sessions: usize,
}

// ── RPC Context ───────────────────────────────────────────────────────────────

/// Context passed to every method handler.
pub struct RpcCtx {
    pub state: Arc<RwLock<Option<RpcState>>>,
    pub config: Config,
    /// Channel for writing responses. Set by the transport layer.
    pub stdout_tx: Arc<Mutex<Option<tokio::sync::mpsc::Sender<String>>>>,
}

impl RpcCtx {
    pub fn new(config: Config) -> Self {
        Self {
            state: Arc::new(RwLock::new(None)),
            config,
            stdout_tx: Arc::new(Mutex::new(None)),
        }
    }

    /// Initialize shared state (call once after construction).
    pub async fn init(&self, max_sessions: usize, session_timeout_secs: u64) {
        let state = RpcState {
            sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
            session_timeout: Duration::from_secs(session_timeout_secs),
            max_sessions,
        };
        let mut guard = self.state.write().await;
        *guard = Some(state);
    }

    /// Set the stdout writer (used by the Stdio transport).
    pub async fn set_stdout(&self, tx: tokio::sync::mpsc::Sender<String>) {
        let mut guard = self.stdout_tx.lock().await;
        *guard = Some(tx);
    }

    // ── Low-level I/O ──────────────────────────────────────────────────────

    async fn write_json<T: serde::Serialize>(&self, value: &T) {
        if let Ok(json) = serde_json::to_string(value) {
            let json_len = json.len();
            let line = json + "\n";
            // Try stdio first
            {
                let guard = self.stdout_tx.lock().await;
                if let Some(ref tx) = *guard {
                    let _ = tx.send(line).await;
                    return;
                }
            }
            // Fallback to tracing when no stdio is set
            tracing::trace!(json_len, "rpc: tx");
        }
    }

    /// Write a JSON-RPC response (success or error) to stdout.
    pub async fn write_response(&self, id: Value, result: Value) {
        use crate::rpc::codec::JsonRpcResponse;
        self.write_json(&JsonRpcResponse::success(id, result)).await;
    }

    /// Write a JSON-RPC error response to stdout.
    pub async fn write_error(&self, id: Value, err: RpcError) {
        use crate::rpc::codec::JsonRpcResponse;
        self.write_json(&JsonRpcResponse::error(id, err)).await;
    }

    /// Write a JSON-RPC notification (no id).
    pub async fn write_notification(&self, method: &'static str, params: Value) {
        self.write_json(&JsonRpcNotification::new(method, params))
            .await;
    }

    // ── Method dispatcher ──────────────────────────────────────────────────

    /// Dispatch a JSON-RPC request to the appropriate handler.
    pub async fn handle_request(&self, method: &str, params: Value, id: Option<Value>) {
        let id = id.unwrap_or(Value::Null);

        let result = match method {
            "initialize" => self.handle_initialize(&params).await,
            "session/new" => self.handle_session_new(&params).await,
            "session/prompt" => self.handle_session_prompt(&params).await,
            "session/prompt_stream" => {
                self.handle_session_prompt_stream(&params).await;
                return;
            }
            "session/stop" => self.handle_session_stop(&params).await,
            "session/list" => self.handle_session_list().await,
            "session/kill" => self.handle_session_kill(&params).await,
            "system/info" => self.handle_system_info().await,
            "system/health" => self.handle_system_health().await,
            "tool/list" => self.handle_tool_list().await,
            "tool/exec" => self.handle_tool_exec(&params).await,
            "memory/store" => self.handle_memory_store(&params).await,
            "memory/recall" => self.handle_memory_recall(&params).await,
            "blackboard/put" => self.handle_blackboard_put(&params).await,
            "blackboard/get" => self.handle_blackboard_get(&params).await,
            "blackboard/list" => self.handle_blackboard_list().await,
            "blackboard/watch" => self.handle_blackboard_watch(&params).await,
            "blackboard/unwatch" => self.handle_blackboard_unwatch(&params).await,
            _ => Err(RpcError::method_not_found(method)),
        };

        if id.is_null() {
            return;
        }

        match result {
            Ok(value) => self.write_response(id, value).await,
            Err(err) => self.write_error(id, err).await,
        }
    }

    /// HTTP-specific request handler that returns Result directly (no notification routing).
    pub async fn handle_http_request(&self, method: &str, params: Value) -> RpcResult {
        match method {
            "initialize" => self.handle_initialize(&params).await,
            "session/new" => self.handle_session_new(&params).await,
            "session/prompt" => self.handle_session_prompt(&params).await,
            "session/stop" => self.handle_session_stop(&params).await,
            "session/list" => self.handle_session_list().await,
            "session/kill" => self.handle_session_kill(&params).await,
            "system/info" => self.handle_system_info().await,
            "system/health" => self.handle_system_health().await,
            "tool/list" => self.handle_tool_list().await,
            "tool/exec" => self.handle_tool_exec(&params).await,
            "memory/store" => self.handle_memory_store(&params).await,
            "memory/recall" => self.handle_memory_recall(&params).await,
            "blackboard/put" => self.handle_blackboard_put(&params).await,
            "blackboard/get" => self.handle_blackboard_get(&params).await,
            "blackboard/list" => self.handle_blackboard_list().await,
            "blackboard/watch" => self.handle_blackboard_watch(&params).await,
            "blackboard/unwatch" => self.handle_blackboard_unwatch(&params).await,
            _ => Err(RpcError::method_not_found(method)),
        }
    }

    // ── initialize ─────────────────────────────────────────────────────────

    async fn handle_initialize(&self, _params: &Value) -> RpcResult {
        let state = self.state.read().await;
        let state = state
            .as_ref()
            .ok_or_else(|| RpcError::internal("RPC server not initialized"))?;
        Ok(serde_json::json!({
            "protocolVersion": "1.0",
            "serverInfo": {
                "name": "sen-rpc",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "capabilities": {
                "streaming": true,
                "maxSessions": state.max_sessions,
                "sessionTimeoutSecs": state.session_timeout.as_secs(),
                "methods": METHODS,
            },
        }))
    }

    // ── session/new ────────────────────────────────────────────────────────

    async fn handle_session_new(&self, params: &Value) -> RpcResult {
        let state = self.state.read().await;
        let state = state
            .as_ref()
            .ok_or_else(|| RpcError::internal("RPC server not initialized"))?;

        let mut sessions = state.sessions.lock().await;
        if sessions.len() >= state.max_sessions {
            return Err(RpcError::session_limit_reached(state.max_sessions));
        }

        let workspace_dir = params
            .get("cwd")
            .or_else(|| params.get("workspaceDir"))
            .or_else(|| params.get("workspace_dir"))
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| self.config.workspace_dir.to_str().unwrap_or("."))
            .to_string();

        let model = params
            .get("model")
            .and_then(|v| v.as_str())
            .map(String::from);

        let session_id = Uuid::new_v4().to_string();

        let mut session_config = self.config.clone();
        if let Some(m) = model {
            session_config.default_model = Some(m);
        }

        let agent = Agent::from_config(&session_config, None)
            .await
            .map_err(|e| RpcError::agent(format!("Failed to create agent: {e}")))?;

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

        info!("RPC: created session {session_id} (workspace: {workspace_dir})");

        Ok(serde_json::json!({
            "sessionId": session_id,
            "workspaceDir": workspace_dir,
        }))
    }

    // ── session/prompt ─────────────────────────────────────────────────────

    async fn handle_session_prompt(&self, params: &Value) -> RpcResult {
        let session_id = self.extract_session_id(params)?;
        let prompt = self.extract_prompt(params)?;

        let state = self.state.read().await;
        let state = state
            .as_ref()
            .ok_or_else(|| RpcError::internal("RPC server not initialized"))?;

        // Remove session from map so we have mutable ownership of Agent
        let mut session = {
            let mut sessions = state.sessions.lock().await;
            sessions
                .remove(&session_id)
                .ok_or_else(|| RpcError::session_not_found(&session_id))?
        };

        let timeout_duration = state.session_timeout;
        let result = timeout(timeout_duration, session.agent.turn(&prompt)).await;

        let result = match result {
            Ok(Ok(text)) => text,
            Ok(Err(e)) => return Err(RpcError::agent(format!("Turn failed: {e}"))),
            Err(_) => return Err(RpcError::session_timeout(&session_id)),
        };

        session.last_active = Instant::now();
        let mut sessions = state.sessions.lock().await;
        sessions.insert(session_id.clone(), session);

        Ok(serde_json::json!({
            "sessionId": session_id,
            "content": result,
        }))
    }

    // ── session/prompt_stream ───────────────────────────────────────────────

    async fn handle_session_prompt_stream(&self, params: &Value) {
        let session_id = match self.extract_session_id(params) {
            Ok(id) => id,
            Err(e) => {
                self.write_error(Value::Null, e).await;
                return;
            }
        };
        let prompt = match self.extract_prompt(params) {
            Ok(p) => p,
            Err(e) => {
                self.write_error(Value::Null, e).await;
                return;
            }
        };

        let state = self.state.read().await;
        let state = match state.as_ref() {
            Some(s) => s,
            None => {
                self.write_error(
                    Value::Null,
                    RpcError::internal("RPC server not initialized"),
                )
                .await;
                return;
            }
        };

        // Remove session from map (ACP pattern: mutable ownership for the turn)
        let mut session = match {
            let mut sessions = state.sessions.lock().await;
            sessions.remove(&session_id)
        } {
            Some(s) => s,
            None => {
                self.write_error(Value::Null, RpcError::session_not_found(&session_id))
                    .await;
                return;
            }
        };

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(100);
        let sessions_ref = Arc::clone(&state.sessions);
        let sid = session_id.clone();

        // Spawn the turn; the async block takes ownership of session and returns it
        let turn_handle = tokio::spawn(async move {
            let result = session.agent.turn_streamed(&prompt, event_tx).await;
            (session, result)
        });

        // Forward events as they arrive
        while let Some(event) = event_rx.recv().await {
            match event {
                TurnEvent::Chunk { delta } => {
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "chunk",
                            "content": delta,
                        }),
                    )
                    .await;
                }
                TurnEvent::Thinking { delta } => {
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "thinking",
                            "content": delta,
                        }),
                    )
                    .await;
                }
                TurnEvent::ToolCall { name, args } => {
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "tool_call",
                            "name": name,
                            "args": args,
                        }),
                    )
                    .await;
                }
                TurnEvent::ToolResult { name, output } => {
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "tool_result",
                            "name": name,
                            "output": output,
                        }),
                    )
                    .await;
                }
            }
        }

        // Wait for turn to complete and recover the session
        let turn_result = turn_handle.await;

        match turn_result {
            Ok((mut session, Ok(_))) => {
                // Turn completed normally — put session back
                let mut sessions = sessions_ref.lock().await;
                sessions.remove(&sid);
                session.last_active = Instant::now();
                sessions.insert(sid, session);
            }
            Ok((_session, Err(e))) => {
                self.write_notification(
                    "session/event",
                    serde_json::json!({
                        "sessionId": sid,
                        "type": "error",
                        "message": format!("{e}"),
                    }),
                )
                .await;
                // Discard session on error
                let mut sessions = sessions_ref.lock().await;
                sessions.remove(&sid);
            }
            Err(e) => {
                self.write_notification(
                    "session/event",
                    serde_json::json!({
                        "sessionId": sid,
                        "type": "error",
                        "message": format!("Session panicked: {e}"),
                    }),
                )
                .await;
            }
        }
    }

    // ── session/stop ────────────────────────────────────────────────────────

    async fn handle_session_stop(&self, params: &Value) -> RpcResult {
        let session_id = self.extract_session_id(params)?;

        let state = self.state.read().await;
        let state = state
            .as_ref()
            .ok_or_else(|| RpcError::internal("RPC server not initialized"))?;

        let removed = {
            let mut sessions = state.sessions.lock().await;
            sessions.remove(&session_id).is_some()
        };

        if removed {
            info!("RPC: stopped session {session_id}");
            Ok(serde_json::json!({
                "sessionId": session_id,
                "stopped": true,
            }))
        } else {
            Err(RpcError::session_not_found(&session_id))
        }
    }

    // ── session/list ────────────────────────────────────────────────────────

    async fn handle_session_list(&self) -> RpcResult {
        let state = self.state.read().await;
        let state = state
            .as_ref()
            .ok_or_else(|| RpcError::internal("RPC server not initialized"))?;

        let sessions = state.sessions.lock().await;
        let list: Vec<Value> = sessions
            .iter()
            .map(|(id, s)| {
                serde_json::json!({
                    "sessionId": id,
                    "workspaceDir": s.workspace_dir,
                    "createdAt": s.created_at.elapsed().as_secs(),
                    "lastActive": s.last_active.elapsed().as_secs(),
                    "idleSecs": s.last_active.elapsed().as_secs(),
                })
            })
            .collect();

        Ok(serde_json::json!({
            "sessions": list,
            "count": list.len(),
            "maxSessions": state.max_sessions,
        }))
    }

    // ── session/kill ────────────────────────────────────────────────────────

    async fn handle_session_kill(&self, params: &Value) -> RpcResult {
        self.handle_session_stop(params).await
    }

    // ── system/info ────────────────────────────────────────────────────────

    async fn handle_system_info(&self) -> RpcResult {
        Ok(serde_json::json!({
            "name": "sen-rpc",
            "version": env!("CARGO_PKG_VERSION"),
            "protocolVersion": "1.0",
        }))
    }

    // ── system/health ──────────────────────────────────────────────────────

    async fn handle_system_health(&self) -> RpcResult {
        let state = self.state.read().await;
        let session_count = if let Some(ref s) = *state {
            s.sessions.lock().await.len()
        } else {
            0
        };

        Ok(serde_json::json!({
            "status": "ok",
            "activeSessions": session_count,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }))
    }

    // ── tool/list ─────────────────────────────────────────────────────────

    async fn handle_tool_list(&self) -> RpcResult {
        // Tool introspection is available once a session is created.
        Ok(serde_json::json!({
            "tools": [],
            "note": "Use session/new to create an agent and inspect its tools via session/prompt"
        }))
    }

    // ── tool/exec ─────────────────────────────────────────────────────────

    async fn handle_tool_exec(&self, params: &Value) -> RpcResult {
        let session_id = params
            .get("sessionId")
            .or_else(|| params.get("session_id"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::invalid_params("Missing required: sessionId or session_id"))?;

        let tool_name = params
            .get("tool")
            .or_else(|| params.get("name"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::invalid_params("Missing required: tool or name"))?;

        let args = params
            .get("args")
            .or_else(|| params.get("arguments"))
            .cloned()
            .unwrap_or(Value::Null);

        let state = self.state.read().await;
        let state = state
            .as_ref()
            .ok_or_else(|| RpcError::internal("RPC server not initialized"))?;

        // Remove session from map for mutable access to Agent
        let mut session = {
            let mut sessions = state.sessions.lock().await;
            sessions
                .remove(session_id)
                .ok_or_else(|| RpcError::session_not_found(session_id))?
        };

        let result = session.agent.execute_tool(tool_name, args).await;
        session.last_active = Instant::now();
        let mut sessions = state.sessions.lock().await;
        sessions.insert(session_id.to_string(), session);

        Ok(serde_json::json!({
            "name": result.name,
            "output": result.output,
            "success": result.success,
        }))
    }

    // ── memory/store ──────────────────────────────────────────────────────

    async fn handle_memory_store(&self, params: &Value) -> RpcResult {
        let content = params
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::invalid_params("Missing required: content"))?;

        let namespace = params
            .get("namespace")
            .and_then(|v| v.as_str())
            .unwrap_or("default");

        let category = match params
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("conversation")
        {
            "core" => crate::memory::MemoryCategory::Core,
            "daily" => crate::memory::MemoryCategory::Daily,
            "conversation" => crate::memory::MemoryCategory::Conversation,
            other => crate::memory::MemoryCategory::Custom(other.to_string()),
        };

        let mem: Arc<dyn crate::memory::Memory> = Arc::from(
            crate::memory::create_memory_with_storage_and_routes(
                &self.config.memory,
                &self.config.embedding_routes,
                Some(&self.config.storage.provider.config),
                &self.config.workspace_dir,
                self.config.api_key.as_deref(),
            )
            .map_err(|e| RpcError::memory(format!("Failed to create memory: {e}")))?,
        );

        mem.store(
            &Uuid::new_v4().to_string(),
            content,
            category,
            Some(namespace),
        )
        .await
        .map_err(|e| RpcError::memory(format!("Store failed: {e}")))?;

        Ok(serde_json::json!({
            "stored": true,
            "namespace": namespace,
        }))
    }

    // ── memory/recall ─────────────────────────────────────────────────────

    async fn handle_memory_recall(&self, params: &Value) -> RpcResult {
        let query = params
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::invalid_params("Missing required: query"))?;

        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;

        let namespace = params
            .get("namespace")
            .and_then(|v| v.as_str())
            .unwrap_or("default");

        let mem: Arc<dyn crate::memory::Memory> = Arc::from(
            crate::memory::create_memory_with_storage_and_routes(
                &self.config.memory,
                &self.config.embedding_routes,
                Some(&self.config.storage.provider.config),
                &self.config.workspace_dir,
                self.config.api_key.as_deref(),
            )
            .map_err(|e| RpcError::memory(format!("Failed to create memory: {e}")))?,
        );

        let results = mem
            .recall(query, limit, Some(namespace), None, None)
            .await
            .map_err(|e| RpcError::memory(format!("Recall failed: {e}")))?;

        let items: Vec<Value> = results
            .into_iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "key": r.key,
                    "content": r.content,
                    "score": r.score,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "results": items,
            "count": items.len(),
            "query": query,
        }))
    }

    // ── blackboard ─────────────────────────────────────────────────────────

    async fn handle_blackboard_put(&self, params: &Value) -> RpcResult {
        let key = params
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::invalid_params("Missing required: key"))?;

        let value = params
            .get("value")
            .cloned()
            .ok_or_else(|| RpcError::invalid_params("Missing required: value"))?;

        let namespace = params
            .get("namespace")
            .and_then(|v| v.as_str())
            .unwrap_or("default");

        let bb = Blackboard::new();
        bb.write(key, value, "rpc", namespace);
        Ok(serde_json::json!({ "key": key, "namespace": namespace }))
    }

    async fn handle_blackboard_get(&self, params: &Value) -> RpcResult {
        let key = params
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::invalid_params("Missing required: key"))?;

        let bb = Blackboard::new();
        let entry = bb.read(key);
        match entry {
            Some(e) => Ok(serde_json::json!({
                "key": e.key,
                "value": e.value,
                "version": e.version,
                "namespace": e.namespace,
            })),
            None => Ok(Value::Null),
        }
    }

    async fn handle_blackboard_list(&self) -> RpcResult {
        let bb = Blackboard::new();
        let keys = bb.keys_in_namespace("default");
        Ok(serde_json::json!({ "keys": keys }))
    }

    async fn handle_blackboard_watch(&self, params: &Value) -> RpcResult {
        let _key = params
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::invalid_params("Missing required: key"))?;
        Ok(serde_json::json!({ "watching": true }))
    }

    async fn handle_blackboard_unwatch(&self, params: &Value) -> RpcResult {
        let _key = params
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::invalid_params("Missing required: key"))?;
        Ok(serde_json::json!({ "unwatched": true }))
    }

    // ── Helpers ────────────────────────────────────────────────────────────

    fn extract_session_id(&self, params: &Value) -> RpcResult<String> {
        params
            .get("sessionId")
            .or_else(|| params.get("session_id"))
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| RpcError::invalid_params("Missing required: sessionId or session_id"))
    }

    fn extract_prompt(&self, params: &Value) -> RpcResult<String> {
        params
            .get("prompt")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| RpcError::invalid_params("Missing required: prompt"))
    }
}
