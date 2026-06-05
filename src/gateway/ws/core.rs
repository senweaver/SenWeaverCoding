// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::AppState;
use axum::{
    Json,
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::sync::OnceLock;
use tracing::debug;

static GATEWAY_APPROVAL_TX: OnceLock<
    tokio::sync::broadcast::Sender<crate::session::SessionEvent>,
> = OnceLock::new();

fn gateway_approval_sender(
) -> &'static tokio::sync::broadcast::Sender<crate::session::SessionEvent> {
    GATEWAY_APPROVAL_TX.get_or_init(|| {
        let (tx, _) = tokio::sync::broadcast::channel(256);
        tx
    })
}

pub fn subscribe_gateway_approval_events(
) -> tokio::sync::broadcast::Receiver<crate::session::SessionEvent> {
    gateway_approval_sender().subscribe()
}

pub fn gateway_approval_bus(
) -> &'static tokio::sync::broadcast::Sender<crate::session::SessionEvent> {
    gateway_approval_sender()
}

pub fn gateway_approval_sink_handle() -> crate::session::SessionEventSink {
    crate::session::SessionEventSink::new(gateway_approval_sender().clone())
}

pub fn approval_sender_for_desktop(
) -> &'static tokio::sync::broadcast::Sender<crate::session::SessionEvent> {
    gateway_approval_sender()
}

fn gateway_approval_sink() -> crate::session::SessionEventSink {
    crate::session::SessionEventSink::new(gateway_approval_sender().clone())
}

#[derive(Deserialize)]
pub struct ApprovalRespondBody {
    decision: String,
}

pub async fn handle_approval_respond(
    Path(approval_id): Path<String>,
    Json(body): Json<ApprovalRespondBody>,
) -> impl IntoResponse {
    let decision = body.decision.to_ascii_lowercase();
    if !matches!(decision.as_str(), "yes" | "always" | "no") {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid_decision",
                "message": "decision must be \"yes\", \"always\", or \"no\""
            })),
        )
            .into_response();
    }

    if !crate::approval::claim_pending_gateway_approval(&approval_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "approval_not_found",
                "message": format!("no pending approval with id \"{approval_id}\"")
            })),
        )
            .into_response();
    }

    gateway_approval_sink().emit_kind(crate::session::SessionEventKind::ApprovalResponded {
        id: approval_id,
        decision,
        responder: Some("http_gateway".to_string()),
        updated_input: None,
    });

    crate::observability::session_write_mode_metrics::incr_approval_responded_via_session();

    (
        StatusCode::OK,
        Json(serde_json::json!({ "status": "accepted" })),
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
struct ConnectParams {
    #[serde(rename = "type")]
    msg_type: String,

    #[serde(default)]
    session_id: Option<String>,

    #[serde(default)]
    device_name: Option<String>,

    #[serde(default)]
    capabilities: Vec<String>,
}

const WS_PROTOCOL: &str = "sen.v1";

const BEARER_SUBPROTO_PREFIX: &str = "bearer.";

#[derive(Deserialize)]
pub struct WsQuery {
    pub token: Option<String>,
    pub session_id: Option<String>,

    pub name: Option<String>,
}

pub fn extract_ws_token<'a>(headers: &'a HeaderMap, query_token: Option<&'a str>) -> Option<&'a str> {

    if let Some(t) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|auth| auth.strip_prefix("Bearer "))
    {
        if !t.is_empty() {
            return Some(t);
        }
    }

    if let Some(t) = headers
        .get("sec-websocket-protocol")
        .and_then(|v| v.to_str().ok())
        .and_then(|protos| {
            protos
                .split(',')
                .map(|p| p.trim())
                .find_map(|p| p.strip_prefix(BEARER_SUBPROTO_PREFIX))
        })
    {
        if !t.is_empty() {
            return Some(t);
        }
    }

    if let Some(t) = query_token {
        if !t.is_empty() {
            return Some(t);
        }
    }

    None
}

pub async fn handle_ws_chat(
    State(state): State<AppState>,
    Query(params): Query<WsQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    tracing::info!(
        session_id = ?params.session_id,
        name = ?params.name,
        "WebSocket chat upgrade requested"
    );

    if state.pairing.require_pairing() {
        let token = extract_ws_token(&headers, params.token.as_deref()).unwrap_or("");
        if !state.pairing.is_authenticated(token) {
            return (
                axum::http::StatusCode::UNAUTHORIZED,
                "Unauthorized  - provide Authorization header, Sec-WebSocket-Protocol bearer, or ?token= query param",
            )
                .into_response();
        }
    }

    let ws = if headers
        .get("sec-websocket-protocol")
        .and_then(|v| v.to_str().ok())
        .map_or(false, |protos| {
            protos.split(',').any(|p| p.trim() == WS_PROTOCOL)
        }) {
        ws.protocols([WS_PROTOCOL])
    } else {
        ws
    };

    let session_id = params.session_id;
    let session_name = params.name;
    ws.on_upgrade(move |socket| handle_socket(socket, state, session_id, session_name))
        .into_response()
}

const GW_SESSION_PREFIX: &str = "gw_";

async fn handle_socket(
    socket: WebSocket,
    state: AppState,
    session_id: Option<String>,
    session_name: Option<String>,
) {
    let (mut sender, mut receiver) = socket.split();

    tracing::info!("WebSocket socket upgraded, session_id={:?}", session_id);

    let session_id = session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let session_key = format!("{GW_SESSION_PREFIX}{session_id}");

    let config = state.config.lock().clone();
    let mut agent = match crate::agent::Agent::from_config(&config, None, None).await {
        Ok(a) => a,
        Err(e) => {
            tracing::error!(error = %e, "Agent initialization failed");
            let err = serde_json::json!({
                "type": "error",
                "message": format!("Failed to initialise agent: {e}"),
                "code": "AGENT_INIT_FAILED"
            });
            let _ = sender.send(Message::Text(err.to_string().into())).await;
            let _ = sender
                .send(Message::Close(Some(axum::extract::ws::CloseFrame {
                    code: 1011,
                    reason: axum::extract::ws::Utf8Bytes::from_static(
                        "Agent initialization failed",
                    ),
                })))
                .await;
            return;
        }
    };
    agent.set_memory_session_id(Some(session_id.clone()));
    if config.nodes.enabled {
        agent.add_node_tools_from_registry(std::sync::Arc::clone(&state.node_registry));
    }

    if config.rbac.enabled {
        if let Some(ref engine) = state.rbac {
            let identity = crate::security::rbac::CallerIdentity::from_gateway_session(&session_id);
            agent.set_rbac_session(Some(std::sync::Arc::clone(engine)), Some(identity));
        }
    }

    let mut resumed = false;
    let mut message_count: usize = 0;
    let mut effective_name: Option<String> = None;
    if let Some(backend) = state.session_backend.clone() {
        let session_key_load = session_key.clone();
        let backend_load = backend.clone();
        let messages = match tokio::task::spawn_blocking(move || backend_load.load(&session_key_load))
            .await
        {
            Ok(messages) => messages,
            Err(e) => {
                tracing::warn!(
                    target: "ws_persist",
                    error = %e,
                    "session history load task panicked; starting with empty history"
                );
                Vec::new()
            }
        };
        if !messages.is_empty() {
            message_count = messages.len();
            agent.seed_history(&messages);
            resumed = true;
        }

        if let Some(ref name) = session_name {
            if !name.is_empty() {
                let session_key_set = session_key.clone();
                let backend_set = backend.clone();
                let name_owned = name.clone();
                match tokio::task::spawn_blocking(move || {
                    backend_set.set_session_name(&session_key_set, &name_owned)
                })
                .await
                {
                    Ok(Err(e)) => tracing::warn!(
                        target: "ws_persist",
                        error = %e,
                        "failed to persist session name on resume"
                    ),
                    Err(e) => tracing::warn!(
                        target: "ws_persist",
                        error = %e,
                        "session name persist task panicked on resume"
                    ),
                    Ok(Ok(())) => {}
                }
                effective_name = Some(name.clone());
            }
        }

        if effective_name.is_none() {
            let session_key_get = session_key.clone();
            let backend_get = backend.clone();
            effective_name = tokio::task::spawn_blocking(move || {
                backend_get.get_session_name(&session_key_get).unwrap_or(None)
            })
            .await
            .ok()
            .flatten();
        }
    }

    let mut session_start = serde_json::json!({
        "type": "session_start",
        "session_id": session_id,
        "resumed": resumed,
        "message_count": message_count,
    });
    if let Some(ref name) = effective_name {
        session_start["name"] = serde_json::Value::String(name.clone());
    }
    let _ = sender
        .send(Message::Text(session_start.to_string().into()))
        .await;

    let mut first_msg_fallback: Option<String> = None;
    let mut pending_inbound: std::collections::VecDeque<String> = std::collections::VecDeque::new();

    if let Some(first) = receiver.next().await {
        match first {
            Ok(Message::Text(text)) => {
                if let Ok(cp) = serde_json::from_str::<ConnectParams>(&text) {
                    if cp.msg_type == "connect" {
                        debug!(
                            session_id = ?cp.session_id,
                            device_name = ?cp.device_name,
                            capabilities = ?cp.capabilities,
                            "WebSocket connect params received"
                        );

                        if let Some(sid) = &cp.session_id {
                            agent.set_memory_session_id(Some(sid.clone()));
                        }
                        let ack = serde_json::json!({
                            "type": "connected",
                            "message": "Connection established"
                        });
                        let _ = sender.send(Message::Text(ack.to_string().into())).await;
                    } else {

                        first_msg_fallback = Some(text.to_string());
                    }
                } else {

                    first_msg_fallback = Some(text.to_string());
                }
            }
            Ok(Message::Close(_)) | Err(_) => return,
            _ => {}
        }
    }

    if let Some(ref text) = first_msg_fallback {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(text) {
            if parsed["type"].as_str() == Some("message") {
                let content = parsed["content"].as_str().unwrap_or("").to_string();
                if !content.is_empty() {

                    if let Some(backend) = state.session_backend.clone() {
                        let user_msg = crate::providers::ChatMessage::user(&content);
                        let session_key_owned = session_key.clone();
                        match tokio::task::spawn_blocking(move || {
                            backend.append(&session_key_owned, &user_msg)
                        })
                        .await
                        {
                            Ok(Err(e)) => tracing::warn!(
                                target: "ws_persist",
                                error = %e,
                                "failed to persist user message to session backend"
                            ),
                            Err(e) => tracing::warn!(
                                target: "ws_persist",
                                error = %e,
                                "session backend append task panicked for user message"
                            ),
                            Ok(Ok(())) => {}
                        }
                    }
                    agent.reset_cancel();
                    process_chat_message(
                        &state,
                        &mut agent,
                        &mut sender,
                        &mut receiver,
                        &mut pending_inbound,
                        &content,
                        &session_key,
                    )
                    .await;
                }
            } else {
                let unknown_type = parsed["type"].as_str().unwrap_or("unknown");
                let err = serde_json::json!({
                    "type": "error",
                    "message": format!(
                        "Unsupported message type \"{unknown_type}\". Send {{\"type\":\"message\",\"content\":\"your text\"}}"
                    )
                });
                let _ = sender.send(Message::Text(err.to_string().into())).await;
            }
        } else {
            let err = serde_json::json!({
                "type": "error",
                "message": "Invalid JSON. Send {\"type\":\"message\",\"content\":\"your text\"}"
            });
            let _ = sender.send(Message::Text(err.to_string().into())).await;
        }
    }

    loop {
        let msg = if let Some(buffered) = pending_inbound.pop_front() {
            buffered
        } else {
            match receiver.next().await {
                Some(Ok(Message::Text(text))) => text.to_string(),
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => continue,
            }
        };

        let parsed: serde_json::Value = match serde_json::from_str(&msg) {
            Ok(v) => v,
            Err(e) => {
                let err = serde_json::json!({
                    "type": "error",
                    "message": format!("Invalid JSON: {}", e),
                    "code": "INVALID_JSON"
                });
                let _ = sender.send(Message::Text(err.to_string().into())).await;
                continue;
            }
        };

        let msg_type = parsed["type"].as_str().unwrap_or("");
        if msg_type != "message" {
            let err = serde_json::json!({
                "type": "error",
                "message": format!(
                    "Unsupported message type \"{msg_type}\". Send {{\"type\":\"message\",\"content\":\"your text\"}}"
                ),
                "code": "UNKNOWN_MESSAGE_TYPE"
            });
            let _ = sender.send(Message::Text(err.to_string().into())).await;
            continue;
        }

        let content = parsed["content"].as_str().unwrap_or("").to_string();
        if content.is_empty() {
            let err = serde_json::json!({
                "type": "error",
                "message": "Message content cannot be empty",
                "code": "EMPTY_CONTENT"
            });
            let _ = sender.send(Message::Text(err.to_string().into())).await;
            continue;
        }

        if let Some(backend) = state.session_backend.clone() {
            let user_msg = crate::providers::ChatMessage::user(&content);
            let session_key_owned = session_key.clone();
            match tokio::task::spawn_blocking(move || {
                backend.append(&session_key_owned, &user_msg)
            })
            .await
            {
                Ok(Err(e)) => tracing::warn!(
                    target: "ws_persist",
                    error = %e,
                    "failed to persist user message to session backend"
                ),
                Err(e) => tracing::warn!(
                    target: "ws_persist",
                    error = %e,
                    "session backend append task panicked for user message"
                ),
                Ok(Ok(())) => {}
            }
        }

        agent.reset_cancel();
        process_chat_message(
            &state,
            &mut agent,
            &mut sender,
            &mut receiver,
            &mut pending_inbound,
            &content,
            &session_key,
        )
        .await;
    }
}

async fn process_chat_message(
    state: &AppState,
    agent: &mut crate::agent::Agent,
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    receiver: &mut futures_util::stream::SplitStream<WebSocket>,
    pending_inbound: &mut std::collections::VecDeque<String>,
    content: &str,
    session_key: &str,
) {
    use crate::agent::TurnEvent;

    let provider_label = state
        .config
        .lock()
        .default_provider
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    let _ = state.event_tx.send(serde_json::json!({
        "type": "agent_start",
        "provider": provider_label,
        "model": state.current_model(),
    }));

    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);

    let cancel_signal_handle = agent.cancel_signal_handle();
    let cancelled_atomic = agent.cancel_token();

    let content_owned = content.to_string();
    let turn_fut = async { agent.turn_streamed(&content_owned, event_tx).await };

    let forward_fut = async {
        let mut accumulated_text = String::new();
        while let Some(event) = event_rx.recv().await {
            let ws_msg = match event {
                TurnEvent::Chunk { delta } => {
                    const MAX_ACCUMULATED_TEXT_BYTES: usize = 2 * 1024 * 1024;
                    if accumulated_text.len() + delta.len() <= MAX_ACCUMULATED_TEXT_BYTES {
                        accumulated_text.push_str(&delta);
                    }
                    serde_json::json!({ "type": "chunk", "content": delta })
                }
                TurnEvent::StreamReset => {
                    serde_json::json!({ "type": "content_reset" })
                }
                TurnEvent::Thinking { delta } => {
                    serde_json::json!({ "type": "thinking", "content": delta })
                }
                TurnEvent::ToolCall {
                    name,
                    args,
                    tool_call_id: _,
                } => {
                    serde_json::json!({ "type": "tool_call", "name": name, "args": args })
                }
                TurnEvent::ToolResult {
                    name,
                    output,
                    success,
                    tool_call_id: _,
                } => {
                    serde_json::json!({
                        "type": "tool_result",
                        "name": name,
                        "output": output,
                        "success": success,
                        "isError": crate::agent::tool_handler::event_status::tool_result_is_error(
                            &name,
                            success,
                            &output,
                        ),
                    })
                }
                TurnEvent::PlanProgressCommitted {
                    plan_path,
                    title,
                    todos_json,
                } => {
                    serde_json::json!({
                        "type": "plan_progress",
                        "planPath": plan_path,
                        "title": title,
                        "todos": serde_json::from_str::<serde_json::Value>(&todos_json)
                            .unwrap_or(serde_json::Value::Null),
                    })
                }
                TurnEvent::Error { message } => {
                    serde_json::json!({ "type": "error", "content": message })
                }

                TurnEvent::FileEdit {
                    path,
                    additions,
                    deletions,
                    diff,
                    edit_batch_id,
                } => {
                    serde_json::json!({
                        "type": "file_edit",
                        "path": path,
                        "additions": additions,
                        "deletions": deletions,
                        "diff": diff,
                        "editBatchId": edit_batch_id,
                    })
                }
                TurnEvent::StatusUpdate { action, detail } => {
                    serde_json::json!({ "type": "status", "action": action, "detail": detail })
                }
                TurnEvent::ProgressTick {
                    iteration,
                    max_iterations,
                    tokens_used,
                } => {
                    serde_json::json!({
                        "type": "progress",
                        "iteration": iteration,
                        "max_iterations": max_iterations,
                        "tokens_used": tokens_used,
                    })
                }
                TurnEvent::CommandPreview {
                    tool_name,
                    args,
                    estimated_duration_ms,
                } => {
                    serde_json::json!({
                        "type": "command_preview",
                        "tool_name": tool_name,
                        "args": args,
                        "estimated_duration_ms": estimated_duration_ms,
                    })
                }
                TurnEvent::Cancelling { reason } => {
                    serde_json::json!({ "type": "cancelling", "reason": reason })
                }
                TurnEvent::ContextCompressed {
                    tokens_before,
                    tokens_after,
                } => {
                    serde_json::json!({
                        "type": "context_compressed",
                        "tokens_before": tokens_before,
                        "tokens_after": tokens_after,
                    })
                }
                TurnEvent::SubagentChunk {
                    task_id,
                    agent_id,
                    kind,
                    delta,
                } => {
                    serde_json::json!({
                        "type": "subagent_chunk",
                        "taskId": task_id,
                        "agentId": agent_id,
                        "kind": format!("{kind:?}").to_lowercase(),
                        "content": delta,
                    })
                }
                TurnEvent::PermissionRequest {
                    request_id,
                    tool_name,
                    input,
                    description,
                } => {

                    serde_json::json!({
                        "type": "permission_request",
                        "requestId": request_id,
                        "toolName": tool_name,
                        "input": input,
                        "description": description,
                    })
                }
                TurnEvent::PiiSanitized { report } => {
                    serde_json::json!({
                        "type": "debug_pii_stats",
                        "total": report.total(),
                        "counts": report.to_label_map(),
                    })
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
                    serde_json::json!({
                        "type": "provider_retry",
                        "attempt": attempt,
                        "maxAttempts": max_attempts,
                        "waitMs": wait_ms,
                        "class": class,
                        "provider": provider,
                        "model": model,
                        "message": message,
                    })
                }
                TurnEvent::WorkerSpawned {
                    parent_tool_use_id,
                    worker_id,
                    title,
                    model,
                } => serde_json::json!({
                    "type": "worker_spawned",
                    "parentToolUseId": parent_tool_use_id,
                    "workerId": worker_id,
                    "title": title,
                    "model": model,
                }),
                TurnEvent::WorkerStatus { worker_id, status, detail } => serde_json::json!({
                    "type": "worker_status",
                    "workerId": worker_id,
                    "status": status,
                    "detail": detail,
                }),
                TurnEvent::WorkerProgress { worker_id, action, detail } => serde_json::json!({
                    "type": "worker_progress",
                    "workerId": worker_id,
                    "action": action,
                    "detail": detail,
                }),
                TurnEvent::WorkerCompleted { worker_id, success, summary } => serde_json::json!({
                    "type": "worker_completed",
                    "workerId": worker_id,
                    "success": success,
                    "summary": summary,
                }),
                TurnEvent::WorkerStopped { worker_id, reason } => serde_json::json!({
                    "type": "worker_stopped",
                    "workerId": worker_id,
                    "reason": reason,
                }),
                TurnEvent::ParentResumed { reason } => serde_json::json!({
                    "type": "parent_resumed",
                    "reason": reason,
                }),
            };
            let _ = sender.send(Message::Text(ws_msg.to_string().into())).await;
        }
        accumulated_text
    };

    use futures_util::FutureExt as _;
    let (turn_caught, forwarded_text) = {
        let joined = async {
            tokio::join!(
                std::panic::AssertUnwindSafe(turn_fut).catch_unwind(),
                forward_fut,
            )
        };
        tokio::pin!(joined);

        let disconnect_watch = async {
            loop {
                match receiver.next().await {
                    Some(Ok(Message::Text(text))) => {
                        pending_inbound.push_back(text.to_string());
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return,
                    Some(Ok(_)) => continue,
                }
            }
        };
        tokio::pin!(disconnect_watch);

        tokio::select! {
        joined_out = &mut joined => (joined_out.0, joined_out.1),
        _ = &mut disconnect_watch => {
            cancelled_atomic.store(true, std::sync::atomic::Ordering::Relaxed);
            cancel_signal_handle.load_full().cancel();
            tracing::info!(
                target: "agent_cancel",
                session_key,
                "websocket disconnected mid-turn: firing cancel to stop orphaned turn"
            );
            joined.await
        }
        }
    };
    let result: Result<String, String> = match turn_caught {
        Ok(inner) => inner.map_err(|e| e.to_string()),
        Err(panic) => {
            let detail = crate::util::describe_panic(&*panic);
            tracing::error!(
                target: "ws_core_turn",
                session_key,
                "turn execution panicked (recovered): {detail}"
            );
            if forwarded_text.trim().is_empty() {
                Err(format!("internal error recovered: {detail}"))
            } else {
                tracing::warn!(
                    target: "ws_core_turn",
                    session_key,
                    "salvaging partially streamed content after panic ({} bytes)",
                    forwarded_text.len()
                );
                Ok(forwarded_text)
            }
        }
    };

    match result {
        Ok(response) => {

            if let Some(backend) = state.session_backend.clone() {
                let assistant_msg = crate::providers::ChatMessage::assistant(&response);
                let session_key_owned = session_key.to_string();
                match tokio::task::spawn_blocking(move || {
                    backend.append(&session_key_owned, &assistant_msg)
                })
                .await
                {
                    Ok(Err(e)) => tracing::warn!(
                        target: "ws_persist",
                        error = %e,
                        "failed to persist assistant message to session backend"
                    ),
                    Err(e) => tracing::warn!(
                        target: "ws_persist",
                        error = %e,
                        "session backend append task panicked for assistant message"
                    ),
                    Ok(Ok(())) => {}
                }
            }

            let reset = serde_json::json!({ "type": "chunk_reset" });
            let _ = sender.send(Message::Text(reset.to_string().into())).await;

            let done = serde_json::json!({
                "type": "done",
                "full_response": response,
            });
            let _ = sender.send(Message::Text(done.to_string().into())).await;

            let _ = state.event_tx.send(serde_json::json!({
                "type": "agent_end",
                "provider": provider_label,
                "model": state.current_model(),
            }));

            let auto_title_config = state.config.lock().auto_title.clone();
            if auto_title_config.enabled {
                if let Some(backend) = state.session_backend.clone() {
                    let session_key_get = session_key.to_string();
                    let backend_get = backend.clone();
                    let existing_name = tokio::task::spawn_blocking(move || {
                        backend_get.get_session_name(&session_key_get).ok().flatten()
                    })
                    .await
                    .ok()
                    .flatten();
                    if existing_name.is_none() {
                        let provider_for_title = state.current_provider();
                        let model_for_title = state.current_model();
                        if let Some(title) = crate::agent::auto_title::generate_title(
                            provider_for_title.as_ref(),
                            content,
                            &response,
                            &model_for_title,
                            &auto_title_config,
                        )
                        .await
                        {
                            let session_key_set = session_key.to_string();
                            let backend_set = backend.clone();
                            let title_for_set = title.clone();
                            let title_persisted = match tokio::task::spawn_blocking(move || {
                                backend_set.set_session_name(&session_key_set, &title_for_set)
                            })
                            .await
                            {
                                Ok(Ok(())) => true,
                                Ok(Err(e)) => {
                                    tracing::warn!(
                                        target: "ws_persist",
                                        error = %e,
                                        "failed to persist auto-generated session title"
                                    );
                                    false
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        target: "ws_persist",
                                        error = %e,
                                        "session title persist task panicked"
                                    );
                                    false
                                }
                            };
                            if title_persisted {
                                let title_msg = serde_json::json!({
                                    "type": "session_title",
                                    "title": title,
                                });
                                let _ = sender
                                    .send(Message::Text(title_msg.to_string().into()))
                                    .await;
                            }
                        }
                    }
                }
            }

            crate::agent::profile::runtime_hooks::publish_message_event("received", "gateway_ws");
            crate::agent::profile::runtime_hooks::publish_message_event("sent", "gateway_ws");

            let config_snapshot = state.config.lock().clone();
            {
                let hooks =
                    crate::agent::profile::runtime_hooks::LearningHooks::from_config(&config_snapshot);
                hooks.record_turn_heuristics(content, &response, &[]);
            }

            if config_snapshot.suggestions.enabled {
                let tool_names: Vec<String> = Vec::new();
                let suggestions = crate::agent::suggestions::generate_rule_based_suggestions(
                    content,
                    &response,
                    &tool_names,
                    &config_snapshot.suggestions,
                );
                if !suggestions.is_empty() {
                    let suggestion_data: Vec<serde_json::Value> = suggestions
                        .iter()
                        .map(|s| {
                            serde_json::json!({
                                "label": s.label,
                                "prompt": s.prompt,
                                "relevance": s.relevance,
                            })
                        })
                        .collect();
                    let suggestions_msg = serde_json::json!({
                        "type": "suggestions",
                        "suggestions": suggestion_data,
                    });
                    let _ = sender
                        .send(Message::Text(suggestions_msg.to_string().into()))
                        .await;
                }
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "Agent turn failed");
            let raw = e.to_string();
            let code = crate::agent::error_classify::classify_turn_error_code(&raw);
            let err = crate::agent::error_classify::user_facing_error_json(&raw, code);
            let _ = sender.send(Message::Text(err.to_string().into())).await;

            let _ = state.event_tx.send(serde_json::json!({
                "type": "error",
                "component": "ws_chat",
                "message": err.get("detail").and_then(|v| v.as_str()).unwrap_or(&raw),
            }));
        }
    }
}

pub fn ask_response_to_user_text(updated_input: &serde_json::Value) -> Option<String> {
    let questions = updated_input
        .get("questions")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let answers = updated_input
        .get("answers")
        .and_then(|v| v.as_object())
        .cloned();
    let details = updated_input
        .get("details")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let skipped = updated_input
        .get("skipped")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let free_text = updated_input
        .get("response")
        .or_else(|| updated_input.get("text"))
        .or_else(|| updated_input.get("answer"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    if questions.is_empty() && answers.is_none() && details.is_none() && free_text.is_none() {
        return None;
    }

    let mut buf = String::new();
    if !questions.is_empty() {
        buf.push_str("Here are my answers to your clarifying questions:\n\n");
        for (idx, q) in questions.iter().enumerate() {
            let qid = q
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| format!("q-{idx}"));
            let prompt = q
                .get("prompt")
                .or_else(|| q.get("question"))
                .and_then(|v| v.as_str())
                .unwrap_or("(no prompt)");
            let allow_multiple = q
                .get("allow_multiple")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let raw = answers
                .as_ref()
                .and_then(|a| a.get(&qid).or_else(|| a.get(prompt)));
            let labels: Vec<String> = match raw {
                Some(v) if v.is_string() => v
                    .as_str()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| vec![s.to_string()])
                    .unwrap_or_default(),
                Some(serde_json::Value::Array(arr)) => arr
                    .iter()
                    .filter_map(|item| match item {
                        serde_json::Value::String(s) => {
                            let t = s.trim();
                            if t.is_empty() {
                                None
                            } else {
                                Some(t.to_string())
                            }
                        }
                        serde_json::Value::Object(map) => map
                            .get("label")
                            .or_else(|| map.get("text"))
                            .or_else(|| map.get("id"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty()),
                        other => {
                            let s = other.to_string();
                            if s.is_empty() { None } else { Some(s) }
                        }
                    })
                    .collect(),
                Some(other) => {
                    let s = other.to_string();
                    if s.is_empty() {
                        Vec::new()
                    } else {
                        vec![s]
                    }
                }
                None => Vec::new(),
            };
            if labels.is_empty() {
                let placeholder = if skipped { "(skipped)" } else { "(no answer)" };
                let _ = std::fmt::Write::write_fmt(
                    &mut buf,
                    format_args!("{}. {prompt}\n   -> {placeholder}\n", idx + 1),
                );
            } else if allow_multiple || labels.len() > 1 {
                let _ = std::fmt::Write::write_fmt(
                    &mut buf,
                    format_args!("{}. {prompt}\n", idx + 1),
                );
                for label in &labels {
                    let _ = std::fmt::Write::write_fmt(
                        &mut buf,
                        format_args!("   -> {label}\n"),
                    );
                }
            } else {
                let _ = std::fmt::Write::write_fmt(
                    &mut buf,
                    format_args!("{}. {prompt}\n   -> {}\n", idx + 1, labels[0]),
                );
            }
        }
    }
    if let Some(d) = details {
        if !buf.is_empty() {
            buf.push('\n');
        }
        buf.push_str("Additional context: ");
        buf.push_str(&d);
        buf.push('\n');
    }
    if let Some(t) = free_text {
        if !buf.is_empty() {
            buf.push('\n');
        }
        buf.push_str(&t);
        buf.push('\n');
    }
    if skipped && buf.is_empty() {
        buf.push_str("(I skipped your questions  -  please proceed with reasonable defaults.)\n");
    }
    if buf.trim().is_empty() {
        None
    } else {
        Some(buf.trim_end().to_string())
    }
}
