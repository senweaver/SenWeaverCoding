// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::agent::agent::{Agent, TurnEvent};
use crate::config::Config;
use crate::rpc::codec::{JsonRpcNotification, RpcError};
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use tokio::time::timeout;
use tracing::info;
use uuid::Uuid;

pub type RpcResult<T = Value> = std::result::Result<T, RpcError>;

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

pub(crate) struct Session {
    pub(crate) agent: Agent,
    pub(crate) created_at: Instant,
    pub(crate) last_active: Instant,
    pub(crate) workspace_dir: String,
}

#[derive(Clone)]
pub(crate) struct InflightCancel {
    cancelled: Arc<std::sync::atomic::AtomicBool>,
    signal: Arc<arc_swap::ArcSwap<tokio_util::sync::CancellationToken>>,
    remove_requested: Arc<std::sync::atomic::AtomicBool>,
}

impl InflightCancel {
    fn from_agent(agent: &Agent) -> Self {
        Self {
            cancelled: agent.cancel_token(),
            signal: agent.cancel_signal_handle(),
            remove_requested: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    fn fire(&self, remove_session: bool) {
        if remove_session {
            self.remove_requested
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.signal.load_full().cancel();
    }

    fn remove_requested(&self) -> bool {
        self.remove_requested
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

pub struct RpcState {
    pub(crate) sessions: Arc<Mutex<std::collections::HashMap<String, Session>>>,
    pub(crate) inflight: Arc<Mutex<std::collections::HashMap<String, InflightCancel>>>,
    pub(crate) watchers: Arc<Mutex<std::collections::HashMap<String, crate::runtime::TaskHandle>>>,
    pub session_timeout: Duration,
    pub max_sessions: usize,
}

pub struct RpcCtx {
    pub state: Arc<RwLock<Option<RpcState>>>,
    pub config: Config,

    pub stdout_tx: Arc<Mutex<Option<tokio::sync::mpsc::Sender<String>>>>,

    memory: Arc<tokio::sync::OnceCell<Arc<dyn crate::memory::Memory>>>,
}

impl RpcCtx {
    pub fn new(config: Config) -> Self {
        Self {
            state: Arc::new(RwLock::new(None)),
            config,
            stdout_tx: Arc::new(Mutex::new(None)),
            memory: Arc::new(tokio::sync::OnceCell::new()),
        }
    }

    pub async fn init(&self, max_sessions: usize, session_timeout_secs: u64) {
        let state = RpcState {
            sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
            inflight: Arc::new(Mutex::new(std::collections::HashMap::new())),
            watchers: Arc::new(Mutex::new(std::collections::HashMap::new())),
            session_timeout: Duration::from_secs(session_timeout_secs),
            max_sessions,
        };
        let mut guard = self.state.write().await;
        *guard = Some(state);
    }

    pub async fn set_stdout(&self, tx: tokio::sync::mpsc::Sender<String>) {
        let mut guard = self.stdout_tx.lock().await;
        *guard = Some(tx);
    }

    pub fn with_output(&self, tx: tokio::sync::mpsc::Sender<String>) -> Arc<RpcCtx> {
        Arc::new(RpcCtx {
            state: Arc::clone(&self.state),
            config: self.config.clone(),
            stdout_tx: Arc::new(Mutex::new(Some(tx))),
            memory: Arc::clone(&self.memory),
        })
    }

    async fn shared_memory(&self) -> RpcResult<Arc<dyn crate::memory::Memory>> {
        let mem = self
            .memory
            .get_or_try_init(|| async {
                crate::memory::create_memory_with_storage_and_routes_async(
                    self.config.memory.clone(),
                    self.config.embedding_routes.clone(),
                    Some(self.config.storage.provider.config.clone()),
                    self.config.workspace_dir.clone(),
                    self.config.api_key.clone(),
                )
                .await
                .map(Arc::from)
            })
            .await
            .map_err(|e: anyhow::Error| RpcError::memory(format!("Failed to create memory: {e}")))?;
        Ok(Arc::clone(mem))
    }

    async fn write_json<T: serde::Serialize>(&self, value: &T) {
        if let Ok(json) = serde_json::to_string(value) {
            let json_len = json.len();
            let line = json + "\n";

            {
                let guard = self.stdout_tx.lock().await;
                if let Some(ref tx) = *guard {
                    let _ = tx.send(line).await;
                    return;
                }
            }

            tracing::trace!(json_len, "rpc: tx");
        }
    }

    pub async fn write_response(&self, id: Value, result: Value) {
        use crate::rpc::codec::JsonRpcResponse;
        self.write_json(&JsonRpcResponse::success(id, result)).await;
    }

    pub async fn write_error(&self, id: Value, err: RpcError) {
        use crate::rpc::codec::JsonRpcResponse;
        self.write_json(&JsonRpcResponse::error(id, err)).await;
    }

    pub async fn write_notification(&self, method: &'static str, params: Value) -> Value {
        let v = serde_json::json!({
            "method": method,
            "params": params,
        });
        self.write_json(&JsonRpcNotification::new(method, params))
            .await;
        v
    }

    pub async fn handle_request(&self, method: &str, params: Value, id: Option<Value>) {
        let id = id.unwrap_or(Value::Null);

        let method = &method.replace('.', "/");
        let method = method.as_str();

        let result = match method {
            "initialize" => self.handle_initialize(&params).await,
            "session/new" => self.handle_session_new(&params).await,
            "session/prompt" => self.handle_session_prompt(&params).await,
            "session/prompt_stream" => {
                self.handle_session_prompt_stream(&params).await;
                if !id.is_null() {
                    self.write_response(id, serde_json::json!({ "streamed": true }))
                        .await;
                }
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

    pub async fn handle_http_request(&self, method: &str, params: Value) -> RpcResult {
        let method = &method.replace('.', "/");
        let method = method.as_str();
        match method {
            "initialize" => self.handle_initialize(&params).await,
            "session/new" => self.handle_session_new(&params).await,
            "session/prompt" => self.handle_session_prompt(&params).await,
            "session/prompt_stream" => self.handle_session_prompt(&params).await,
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

    async fn handle_session_new(&self, params: &Value) -> RpcResult {
        let state = self.state.read().await;
        let state = state
            .as_ref()
            .ok_or_else(|| RpcError::internal("RPC server not initialized"))?;

        let mut sessions = state.sessions.lock().await;
        if sessions.len() >= state.max_sessions {
            return Err(RpcError::session_limit_reached(state.max_sessions));
        }

        let requested_workspace = params
            .get("cwd")
            .or_else(|| params.get("workspaceDir"))
            .or_else(|| params.get("workspace_dir"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let model = params
            .get("model")
            .and_then(|v| v.as_str())
            .map(String::from);

        let session_id = Uuid::new_v4().to_string();

        let mut session_config = self.config.clone();
        if let Some(m) = model {
            session_config.default_model = Some(m);
        }

        let workspace_dir = match requested_workspace {
            Some(raw) => {
                let requested = std::path::PathBuf::from(raw);
                if !requested.is_absolute() {
                    return Err(RpcError::invalid_params(format!(
                        "workspace path '{raw}' must be absolute"
                    )));
                }
                let canonical = tokio::fs::canonicalize(&requested).await.map_err(|e| {
                    RpcError::invalid_params(format!(
                        "workspace path '{raw}' is not accessible: {e}"
                    ))
                })?;
                let metadata = tokio::fs::metadata(&canonical).await.map_err(|e| {
                    RpcError::invalid_params(format!(
                        "workspace path '{raw}' could not be inspected: {e}"
                    ))
                })?;
                if !metadata.is_dir() {
                    return Err(RpcError::invalid_params(format!(
                        "workspace path '{raw}' is not a directory"
                    )));
                }
                session_config.workspace_dir = canonical.clone();
                canonical.to_string_lossy().to_string()
            }
            None => self
                .config
                .workspace_dir
                .to_str()
                .unwrap_or(".")
                .to_string(),
        };

        let agent = Agent::from_config(&session_config, None, None)
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

    async fn handle_session_prompt(&self, params: &Value) -> RpcResult {
        let session_id = self.extract_session_id(params)?;
        let prompt = self.extract_prompt(params)?;

        let (sessions, inflight, timeout_duration) = {
            let state = self.state.read().await;
            let state = state
                .as_ref()
                .ok_or_else(|| RpcError::internal("RPC server not initialized"))?;
            (
                Arc::clone(&state.sessions),
                Arc::clone(&state.inflight),
                state.session_timeout,
            )
        };

        let mut session = {
            let mut guard = sessions.lock().await;
            guard
                .remove(&session_id)
                .ok_or_else(|| RpcError::session_not_found(&session_id))?
        };

        session.agent.reset_cancel();
        let cancel = InflightCancel::from_agent(&session.agent);
        {
            let mut guard = inflight.lock().await;
            guard.insert(session_id.clone(), cancel.clone());
        }

        let result = {
            use futures_util::FutureExt as _;
            timeout(
                timeout_duration,
                std::panic::AssertUnwindSafe(session.agent.turn(&prompt)).catch_unwind(),
            )
            .await
        };

        {
            let mut guard = inflight.lock().await;
            guard.remove(&session_id);
        }

        let reinsert = |session: Session| async {
            if !cancel.remove_requested() {
                let mut guard = sessions.lock().await;
                guard.insert(session_id.clone(), session);
            }
        };

        let result = match result {
            Ok(Ok(Ok(text))) => text,
            Ok(Ok(Err(e))) => {
                use crate::error::{ErrorCategory, ErrorClassification};
                if cancel.remove_requested() || e.category() == ErrorCategory::Cancelled {
                    reinsert(session).await;
                    return Ok(serde_json::json!({
                        "sessionId": session_id,
                        "content": "",
                        "response": "",
                        "stopped": true,
                    }));
                }
                reinsert(session).await;
                return Err(RpcError::agent(format!("Turn failed: {e}")));
            }
            Ok(Err(panic_payload)) => {
                let description = crate::util::describe_panic(panic_payload.as_ref());
                tracing::error!(
                    session_id = %session_id,
                    panic = %description,
                    "RPC session prompt panicked; session returned to pool"
                );
                reinsert(session).await;
                return Err(RpcError::agent(format!("Turn panicked: {description}")));
            }
            Err(_) => {
                reinsert(session).await;
                return Err(RpcError::session_timeout(&session_id));
            }
        };

        session.last_active = Instant::now();
        reinsert(session).await;

        Ok(serde_json::json!({
            "sessionId": session_id,
            "content": result,
            "response": result,
        }))
    }

    async fn handle_session_prompt_stream(&self, params: &Value) -> () {
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

        let (sessions, inflight) = {
            let state = self.state.read().await;
            match state.as_ref() {
                Some(s) => (Arc::clone(&s.sessions), Arc::clone(&s.inflight)),
                None => {
                    self.write_error(
                        Value::Null,
                        RpcError::internal("RPC server not initialized"),
                    )
                    .await;
                    return;
                }
            }
        };

        let session_opt = {
            let mut guard = sessions.lock().await;
            guard.remove(&session_id)
        };
        let Some(mut session) = session_opt else {
            self.write_error(Value::Null, RpcError::session_not_found(&session_id))
                .await;
            return;
        };

        session.agent.reset_cancel();
        let cancel = InflightCancel::from_agent(&session.agent);
        {
            let mut guard = inflight.lock().await;
            guard.insert(session_id.clone(), cancel.clone());
        }

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(100);
        let (session_tx, session_rx) = tokio::sync::oneshot::channel();
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();

        let sid = session_id.clone();

        tokio::spawn(async move {
            use futures_util::FutureExt as _;
            let caught = std::panic::AssertUnwindSafe(
                session.agent.turn_streamed(&prompt, event_tx),
            )
            .catch_unwind()
            .await;
            let result = match caught {
                Ok(r) => r,
                Err(panic) => {
                    let detail = crate::util::describe_panic(&*panic);
                    tracing::error!(
                        target: "rpc.session",
                        "turn execution panicked (recovered): {detail}"
                    );
                    Err(crate::error::AgentError::ToolDispatchFailed(format!(
                        "internal error recovered: {detail}"
                    )))
                }
            };
            let _ = session_tx.send(session);
            let _ = result_tx.send(result);
        });

        const STREAM_EVENT_STALL_TIMEOUT: Duration = Duration::from_secs(30 * 60);
        loop {
            let event = match tokio::time::timeout(STREAM_EVENT_STALL_TIMEOUT, event_rx.recv())
                .await
            {
                Ok(Some(event)) => event,
                Ok(None) => break,
                Err(_) => {
                    tracing::warn!(
                        "RPC: session {sid} produced no stream events for {}s; firing cancel to \
                         release the turn",
                        STREAM_EVENT_STALL_TIMEOUT.as_secs()
                    );
                    cancel.fire(false);
                    continue;
                }
            };
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
                TurnEvent::StreamReset => {
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "content_reset",
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
                TurnEvent::ToolCall {
                    name,
                    args,
                    tool_call_id: _,
                } => {
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
                TurnEvent::ToolArgsDelta { .. } => {}
                TurnEvent::ToolResult {
                    name,
                    output,
                    success,
                    tool_call_id: _,
                } => {
                    let is_error = !success
                        || crate::agent::tool_handler::event_status::output_indicates_error(&output);
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "tool_result",
                            "name": name,
                            "output": output,
                            "success": success,
                            "isError": is_error,
                        }),
                    )
                    .await;
                }
                TurnEvent::PlanProgressCommitted {
                    plan_path,
                    title,
                    todos_json,
                } => {
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "plan_progress",
                            "planPath": plan_path,
                            "title": title,
                            "todos": serde_json::from_str::<serde_json::Value>(&todos_json)
                                .unwrap_or(serde_json::Value::Null),
                        }),
                    )
                    .await;
                }
                TurnEvent::Error { message } => {
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "error",
                            "content": message,
                        }),
                    )
                    .await;
                }

                TurnEvent::FileEdit {
                    path,
                    additions,
                    deletions,
                    diff,
                    edit_batch_id,
                } => {
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "file_edit",
                            "path": path,
                            "additions": additions,
                            "deletions": deletions,
                            "diff": diff,
                            "editBatchId": edit_batch_id,
                        }),
                    )
                    .await;
                }
                TurnEvent::StatusUpdate { action, detail } => {
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "status",
                            "action": action,
                            "detail": detail,
                        }),
                    )
                    .await;
                }
                TurnEvent::ProgressTick {
                    iteration,
                    max_iterations,
                    tokens_used,
                } => {
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "progress",
                            "iteration": iteration,
                            "max_iterations": max_iterations,
                            "tokens_used": tokens_used,
                        }),
                    )
                    .await;
                }
                TurnEvent::CommandPreview {
                    tool_name,
                    args,
                    estimated_duration_ms,
                } => {
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "command_preview",
                            "tool_name": tool_name,
                            "args": args,
                            "estimated_duration_ms": estimated_duration_ms,
                        }),
                    )
                    .await;
                }
                TurnEvent::Cancelling { reason } => {
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "cancelling",
                            "reason": reason,
                        }),
                    )
                    .await;
                }
                TurnEvent::ContextCompressed {
                    tokens_before,
                    tokens_after,
                } => {
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "context_compressed",
                            "tokens_before": tokens_before,
                            "tokens_after": tokens_after,
                        }),
                    )
                    .await;
                }
                TurnEvent::SubagentChunk {
                    task_id,
                    agent_id,
                    kind,
                    delta,
                } => {
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "subagent_chunk",
                            "taskId": task_id,
                            "agentId": agent_id,
                            "kind": format!("{kind:?}").to_lowercase(),
                            "content": delta,
                        }),
                    )
                    .await;
                }
                TurnEvent::PermissionRequest {
                    request_id,
                    tool_name,
                    input,
                    description,
                } => {
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "permission_request",
                            "requestId": request_id,
                            "toolName": tool_name,
                            "input": input,
                            "description": description,
                        }),
                    )
                    .await;
                }
                TurnEvent::PiiSanitized { report } => {
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "debug_pii_stats",
                            "total": report.total(),
                            "counts": report.to_label_map(),
                        }),
                    )
                    .await;
                }
                TurnEvent::ProviderRetry {
                    attempt,
                    max_attempts,
                    wait_ms,
                    class,
                    provider,
                    model,
                    message,
                } => {
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "provider_retry",
                            "attempt": attempt,
                            "maxAttempts": max_attempts,
                            "waitMs": wait_ms,
                            "class": class,
                            "provider": provider,
                            "model": model,
                            "message": message,
                        }),
                    )
                    .await;
                }
                TurnEvent::WorkerSpawned {
                    parent_tool_use_id,
                    worker_id,
                    title,
                    model,
                } => {
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "worker_spawned",
                            "parentToolUseId": parent_tool_use_id,
                            "workerId": worker_id,
                            "title": title,
                            "model": model,
                        }),
                    )
                    .await;
                }
                TurnEvent::WorkerStatus { worker_id, status, detail } => {
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "worker_status",
                            "workerId": worker_id,
                            "status": status,
                            "detail": detail,
                        }),
                    )
                    .await;
                }
                TurnEvent::WorkerProgress { worker_id, action, detail } => {
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "worker_progress",
                            "workerId": worker_id,
                            "action": action,
                            "detail": detail,
                        }),
                    )
                    .await;
                }
                TurnEvent::WorkerCompleted { worker_id, success, summary } => {
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "worker_completed",
                            "workerId": worker_id,
                            "success": success,
                            "summary": summary,
                        }),
                    )
                    .await;
                }
                TurnEvent::WorkerStopped { worker_id, reason } => {
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "worker_stopped",
                            "workerId": worker_id,
                            "reason": reason,
                        }),
                    )
                    .await;
                }
                TurnEvent::ParentResumed { reason } => {
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "parent_resumed",
                            "reason": reason,
                        }),
                    )
                    .await;
                }
            }
        }

        {
            let mut guard = inflight.lock().await;
            guard.remove(&sid);
        }

        let mut session = match session_rx.await {
            Ok(s) => s,
            Err(_) => {
                let mut guard = sessions.lock().await;
                guard.remove(&sid);
                tracing::warn!("RPC: session {} task panicked or was cancelled", sid);
                return;
            }
        };

        let result = match result_rx.await {
            Ok(r) => r,
            Err(_) => {
                let mut guard = sessions.lock().await;
                guard.remove(&sid);
                tracing::warn!("RPC: session {} result channel closed unexpectedly", sid);
                return;
            }
        };

        if cancel.remove_requested() {
            self.write_notification(
                "session/event",
                serde_json::json!({
                    "sessionId": sid,
                    "type": "done",
                    "stopped": true,
                }),
            )
            .await;
            tracing::info!("RPC: session {} stopped during streamed turn; not re-inserting", sid);
            return;
        }

        match result {
            Ok(_) => {
                self.write_notification(
                    "session/event",
                    serde_json::json!({
                        "sessionId": sid,
                        "type": "done",
                    }),
                )
                .await;
                session.last_active = Instant::now();
                let mut guard = sessions.lock().await;
                guard.insert(sid, session);
            }
            Err(e) => {
                use crate::error::{ErrorCategory, ErrorClassification};
                if e.category() != ErrorCategory::Cancelled {
                    self.write_notification(
                        "session/event",
                        serde_json::json!({
                            "sessionId": sid,
                            "type": "error",
                            "message": format!("{e}"),
                        }),
                    )
                    .await;
                }
                self.write_notification(
                    "session/event",
                    serde_json::json!({
                        "sessionId": sid,
                        "type": "done",
                        "error": e.category() != ErrorCategory::Cancelled,
                    }),
                )
                .await;
                session.last_active = Instant::now();
                let mut guard = sessions.lock().await;
                guard.insert(sid, session);
            }
        }
    }

    async fn handle_session_stop(&self, params: &Value) -> RpcResult {
        let session_id = self.extract_session_id(params)?;

        let (sessions, inflight) = {
            let state = self.state.read().await;
            let state = state
                .as_ref()
                .ok_or_else(|| RpcError::internal("RPC server not initialized"))?;
            (Arc::clone(&state.sessions), Arc::clone(&state.inflight))
        };

        let cancelled_inflight = {
            let guard = inflight.lock().await;
            if let Some(handle) = guard.get(&session_id) {
                handle.fire(true);
                true
            } else {
                false
            }
        };

        let removed_idle = {
            let mut guard = sessions.lock().await;
            guard.remove(&session_id).is_some()
        };

        if cancelled_inflight || removed_idle {
            info!(
                "RPC: stopped session {session_id} (inflight_cancelled={cancelled_inflight}, idle_removed={removed_idle})"
            );
            Ok(serde_json::json!({
                "sessionId": session_id,
                "stopped": true,
            }))
        } else {
            Err(RpcError::session_not_found(&session_id))
        }
    }

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

    async fn handle_session_kill(&self, params: &Value) -> RpcResult {
        self.handle_session_stop(params).await
    }

    async fn handle_system_info(&self) -> RpcResult {
        Ok(serde_json::json!({
            "name": "sen-rpc",
            "version": env!("CARGO_PKG_VERSION"),
            "protocolVersion": "1.0",
        }))
    }

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

    async fn handle_tool_list(&self) -> RpcResult {
        let sessions = {
            let state = self.state.read().await;
            state.as_ref().map(|s| Arc::clone(&s.sessions))
        };
        if let Some(sessions) = sessions {
            let guard = sessions.lock().await;
            if let Some(session) = guard.values().next() {
                let tools: Vec<serde_json::Value> = session
                    .agent
                    .tool_specs()
                    .iter()
                    .map(|spec| {
                        serde_json::json!({
                            "name": spec.name,
                            "description": spec.description,
                            "parameters": spec.parameters,
                        })
                    })
                    .collect();
                return Ok(serde_json::json!({ "tools": tools }));
            }
        }

        let security = Arc::new(crate::security::SecurityPolicy::from_config(
            &self.config.autonomy,
            &self.config.workspace_dir,
        ));
        let tools: Vec<serde_json::Value> = crate::tools::default_tools(security)
            .iter()
            .map(|tool| {
                let spec = tool.spec();
                serde_json::json!({
                    "name": spec.name,
                    "description": spec.description,
                    "parameters": spec.parameters,
                })
            })
            .collect();
        Ok(serde_json::json!({
            "tools": tools,
            "note": "No active session; listing the default tool registry"
        }))
    }

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

        let sessions = {
            let state = self.state.read().await;
            let state = state
                .as_ref()
                .ok_or_else(|| RpcError::internal("RPC server not initialized"))?;
            Arc::clone(&state.sessions)
        };

        let mut session = {
            let mut guard = sessions.lock().await;
            guard
                .remove(session_id)
                .ok_or_else(|| RpcError::session_not_found(session_id))?
        };

        let scoped_mode = session
            .agent
            .current_coding_mode()
            .or_else(|| {
                crate::services::try_get_services()
                    .and_then(|svc| svc.session_coding_mode(session_id))
            })
            .unwrap_or_default();
        let result = {
            use futures_util::FutureExt as _;
            std::panic::AssertUnwindSafe(crate::agent::coding_mode::scope_coding_mode(
                scoped_mode,
                session.agent.execute_tool(tool_name, args),
            ))
            .catch_unwind()
            .await
        };
        session.last_active = Instant::now();
        {
            let mut guard = sessions.lock().await;
            guard.insert(session_id.to_string(), session);
        }

        let result = match result {
            Ok(result) => result,
            Err(panic_payload) => {
                let description = crate::util::describe_panic(panic_payload.as_ref());
                tracing::error!(
                    session_id = %session_id,
                    tool = %tool_name,
                    panic = %description,
                    "RPC tool exec panicked; session returned to pool"
                );
                return Err(RpcError::agent(format!(
                    "Tool execution panicked: {description}"
                )));
            }
        };

        Ok(serde_json::json!({
            "name": result.name,
            "output": result.output,
            "success": result.success,
        }))
    }

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

        let mem = self.shared_memory().await?;

        let key = Uuid::new_v4().to_string();
        let importance = params.get("importance").and_then(|v| v.as_f64());
        mem.store_with_metadata(&key, content, category, None, Some(namespace), importance)
            .await
            .map_err(|e| RpcError::memory(format!("Store failed: {e}")))?;

        Ok(serde_json::json!({
            "stored": true,
            "id": key,
            "namespace": namespace,
        }))
    }

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

        let mem = self.shared_memory().await?;

        let results = mem
            .recall_namespaced(namespace, query, limit, None, None, None)
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

        let rt = crate::agent::multi_agent_runtime::init_global_runtime();
        rt.blackboard.inner().write(key, value, "rpc", namespace);
        Ok(serde_json::json!({ "key": key, "namespace": namespace }))
    }

    async fn handle_blackboard_get(&self, params: &Value) -> RpcResult {
        let key = params
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::invalid_params("Missing required: key"))?;

        let rt = crate::agent::multi_agent_runtime::init_global_runtime();
        let entry = rt.blackboard.inner().read(key);
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
        let rt = crate::agent::multi_agent_runtime::init_global_runtime();
        let keys = rt.blackboard.inner().keys_in_namespace("default");
        Ok(serde_json::json!({ "keys": keys }))
    }

    async fn handle_blackboard_watch(&self, params: &Value) -> RpcResult {
        let key = params
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::invalid_params("Missing required: key"))?;
        let namespace = params
            .get("namespace")
            .and_then(|v| v.as_str())
            .unwrap_or("default");

        let tx = {
            let guard = self.stdout_tx.lock().await;
            guard.clone()
        };
        let Some(tx) = tx else {
            return Err(RpcError::invalid_params(
                "blackboard/watch requires a streaming transport (stdio or unix socket); \
                 the HTTP transport cannot deliver change notifications",
            ));
        };

        let watchers = {
            let state = self.state.read().await;
            let state = state
                .as_ref()
                .ok_or_else(|| RpcError::internal("RPC server not initialized"))?;
            Arc::clone(&state.watchers)
        };

        let watch_id = format!("{namespace}::{key}");
        let mut guard = watchers.lock().await;
        if let Some(existing) = guard.get(&watch_id) {
            if !existing.is_finished() {
                return Ok(serde_json::json!({
                    "watching": true,
                    "key": key,
                    "namespace": namespace,
                    "alreadyWatching": true,
                }));
            }
        }

        let rt = crate::agent::multi_agent_runtime::init_global_runtime();
        let mut rx = rt.blackboard.inner().subscribe();
        let task_key = key.to_string();
        let task_namespace = namespace.to_string();
        let handle = crate::runtime::spawn_supervised("rpc.blackboard_watch", async move {
            loop {
                match rx.recv().await {
                    Ok(change) => {
                        if !blackboard_change_matches(
                            &change.key,
                            &change.namespace,
                            &task_key,
                            &task_namespace,
                        ) {
                            continue;
                        }
                        let Ok(change_value) = serde_json::to_value(&change) else {
                            continue;
                        };
                        let notification =
                            JsonRpcNotification::new("blackboard/changed", change_value);
                        let Ok(line) = serde_json::to_string(&notification) else {
                            continue;
                        };
                        if tx.send(line + "\n").await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(
                            key = %task_key,
                            skipped,
                            "blackboard watch lagged behind; some change notifications were dropped"
                        );
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        guard.insert(watch_id, handle);

        Ok(serde_json::json!({
            "watching": true,
            "key": key,
            "namespace": namespace,
        }))
    }

    async fn handle_blackboard_unwatch(&self, params: &Value) -> RpcResult {
        let key = params
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::invalid_params("Missing required: key"))?;
        let namespace = params
            .get("namespace")
            .and_then(|v| v.as_str())
            .unwrap_or("default");

        let watchers = {
            let state = self.state.read().await;
            let state = state
                .as_ref()
                .ok_or_else(|| RpcError::internal("RPC server not initialized"))?;
            Arc::clone(&state.watchers)
        };

        let watch_id = format!("{namespace}::{key}");
        let removed = {
            let mut guard = watchers.lock().await;
            guard.remove(&watch_id)
        };
        let unwatched = match removed {
            Some(handle) => {
                handle.abort();
                true
            }
            None => false,
        };

        Ok(serde_json::json!({
            "unwatched": unwatched,
            "key": key,
            "namespace": namespace,
        }))
    }

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
            .or_else(|| params.get("message"))
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| RpcError::invalid_params("Missing required: prompt (or message)"))
    }
}

fn blackboard_change_matches(
    change_key: &str,
    change_namespace: &str,
    watch_key: &str,
    watch_namespace: &str,
) -> bool {
    let key_matches = change_key == watch_key
        || change_key
            .split_once("::")
            .is_some_and(|(_, tail)| tail == watch_key);
    let namespace_matches = change_namespace == watch_namespace
        || change_namespace
            .strip_prefix(watch_namespace)
            .is_some_and(|rest| rest.starts_with(':'));
    key_matches && namespace_matches
}
