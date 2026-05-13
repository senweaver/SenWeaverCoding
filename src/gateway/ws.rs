// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! WebSocket agent chat handler.
//!
//! Connect: `ws://host:port/ws/chat?session_id=ID&name=My+Session`
//!
//! Protocol:
//! ```text
//! Server -> Client: {"type":"session_start","session_id":"...","name":"...","resumed":true,"message_count":42}
//! Client -> Server: {"type":"message","content":"Hello"}
//! Server -> Client: {"type":"chunk","content":"Hi! "}
//! Server -> Client: {"type":"tool_call","name":"shell","args":{...}}
//! Server -> Client: {"type":"tool_result","name":"shell","output":"..."}
//! Server -> Client: {"type":"done","full_response":"..."}
//! ```
//!
//! Query params:
//! - `session_id` — resume or create a session (default: new UUID)
//! - `name` — optional human-readable label for the session
//! - `token` — bearer auth token (alternative to Authorization header)

use super::AppState;
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

pub(super) fn approval_sender_for_desktop(
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

pub(super) fn extract_ws_token<'a>(headers: &'a HeaderMap, query_token: Option<&'a str>) -> Option<&'a str> {

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
                "Unauthorized — provide Authorization header, Sec-WebSocket-Protocol bearer, or ?token= query param",
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
        let messages = tokio::task::spawn_blocking(move || backend_load.load(&session_key_load))
            .await
            .unwrap_or_default();
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
                let _ = tokio::task::spawn_blocking(move || {
                    backend_set.set_session_name(&session_key_set, &name_owned)
                })
                .await;
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
                        let _ = tokio::task::spawn_blocking(move || {
                            backend.append(&session_key_owned, &user_msg)
                        })
                        .await;
                    }
                    process_chat_message(&state, &mut agent, &mut sender, &content, &session_key)
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

    while let Some(msg) = receiver.next().await {
        let msg = match msg {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(_)) | Err(_) => break,
            _ => continue,
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
            let _ = tokio::task::spawn_blocking(move || {
                backend.append(&session_key_owned, &user_msg)
            })
            .await;
        }

        process_chat_message(&state, &mut agent, &mut sender, &content, &session_key).await;
    }
}

async fn process_chat_message(
    state: &AppState,
    agent: &mut crate::agent::Agent,
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
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
        "model": state.model,
    }));

    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);

    let content_owned = content.to_string();
    let turn_fut = async { agent.turn_streamed(&content_owned, event_tx).await };

    let forward_fut = async {
        while let Some(event) = event_rx.recv().await {
            let ws_msg = match event {
                TurnEvent::Chunk { delta } => {
                    serde_json::json!({ "type": "chunk", "content": delta })
                }
                TurnEvent::Thinking { delta } => {
                    serde_json::json!({ "type": "thinking", "content": delta })
                }
                TurnEvent::ToolCall { name, args } => {
                    serde_json::json!({ "type": "tool_call", "name": name, "args": args })
                }
                TurnEvent::ToolResult { name, output, success } => {
                    serde_json::json!({
                        "type": "tool_result",
                        "name": name,
                        "output": output,
                        "success": success,
                        "isError": !success
                            || crate::agent::tool_event_status::output_indicates_error(&output),
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
            };
            let _ = sender.send(Message::Text(ws_msg.to_string().into())).await;
        }
    };

    let (result, ()) = tokio::join!(turn_fut, forward_fut);

    match result {
        Ok(response) => {

            if let Some(backend) = state.session_backend.clone() {
                let assistant_msg = crate::providers::ChatMessage::assistant(&response);
                let session_key_owned = session_key.to_string();
                let _ = tokio::task::spawn_blocking(move || {
                    backend.append(&session_key_owned, &assistant_msg)
                })
                .await;
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
                "model": state.model,
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
                        if let Some(title) = crate::agent::auto_title::generate_title(
                            state.provider.as_ref(),
                            content,
                            &response,
                            &state.model,
                            &auto_title_config,
                        )
                        .await
                        {
                            let session_key_set = session_key.to_string();
                            let backend_set = backend.clone();
                            let title_for_set = title.clone();
                            let _ = tokio::task::spawn_blocking(move || {
                                backend_set.set_session_name(&session_key_set, &title_for_set)
                            })
                            .await;
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

            crate::agent::runtime_hooks::publish_message_event("received", "gateway_ws");
            crate::agent::runtime_hooks::publish_message_event("sent", "gateway_ws");

            let config_snapshot = state.config.lock().clone();
            {
                let hooks =
                    crate::agent::runtime_hooks::LearningHooks::from_config(&config_snapshot);
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
            let sanitized = crate::providers::sanitize_api_error(&e.to_string());
            let error_code = if sanitized.to_lowercase().contains("api key")
                || sanitized.to_lowercase().contains("authentication")
                || sanitized.to_lowercase().contains("unauthorized")
            {
                "AUTH_ERROR"
            } else if sanitized.to_lowercase().contains("provider")
                || sanitized.to_lowercase().contains("model")
            {
                "PROVIDER_ERROR"
            } else {
                "AGENT_ERROR"
            };
            let err = serde_json::json!({
                "type": "error",
                "message": sanitized,
                "code": error_code,
            });
            let _ = sender.send(Message::Text(err.to_string().into())).await;

            let _ = state.event_tx.send(serde_json::json!({
                "type": "error",
                "component": "ws_chat",
                "message": sanitized,
            }));
        }
    }
}
