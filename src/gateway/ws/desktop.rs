// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::AppState;
use axum::{
    extract::{
        Path, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::HeaderMap,
    response::IntoResponse,
};
use futures_util::{FutureExt, SinkExt, StreamExt};
use serde_json::json;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use tokio::sync::mpsc;

const GW_SESSION_PREFIX: &str = "gw_";

#[derive(Debug)]
enum OutboundFrame {
    Text(String),
    Pong(Vec<u8>),
}

type OutboundSender = tokio::sync::mpsc::Sender<OutboundFrame>;

pub async fn handle_ws_desktop(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if state.pairing.require_pairing() {
        let token = super::extract_ws_token(&headers, None).unwrap_or("");
        if !state.pairing.is_authenticated(token) {
            return (
                axum::http::StatusCode::UNAUTHORIZED,
                "Unauthorized  - provide Authorization header or pairing token",
            )
                .into_response();
        }
    }

    ws.on_upgrade(move |socket| handle_socket(socket, state, session_id))
        .into_response()
}

async fn handle_socket(socket: WebSocket, state: AppState, session_id: String) {
    let (mut sink, mut receiver) = socket.split();

    let (outbound_tx, mut outbound_rx) =
        tokio::sync::mpsc::channel::<OutboundFrame>(1024);

    let writer_handle = tokio::spawn(async move {
        const COALESCE_WINDOW_MS: u64 = 24;
        const COALESCE_MAX_FRAMES: usize = 64;
        let mut delta_buf = String::new();
        loop {
            let frame = match outbound_rx.recv().await {
                Some(f) => f,
                None => break,
            };
            let mut frames: Vec<OutboundFrame> = Vec::new();
            frames.push(frame);
            let deadline = tokio::time::Instant::now()
                + std::time::Duration::from_millis(COALESCE_WINDOW_MS);
            while frames.len() < COALESCE_MAX_FRAMES {
                match tokio::time::timeout_at(deadline, outbound_rx.recv()).await {
                    Ok(Some(f)) => frames.push(f),
                    _ => break,
                }
            }
            delta_buf.clear();
            let mut send_failed = false;
            for f in frames.drain(..) {
                match f {
                    OutboundFrame::Text(s) => {
                        if let Some(rest) = s.strip_prefix("{\"type\":\"content_delta\",\"text\":")
                            && rest.ends_with('}')
                        {
                            let body = &rest[..rest.len() - 1];
                            if let Ok(text) = serde_json::from_str::<String>(body.trim_end_matches(','))
                            {
                                delta_buf.push_str(&text);
                                continue;
                            }
                        }
                        if !delta_buf.is_empty() {
                            let coalesced = serde_json::json!({
                                "type": "content_delta",
                                "text": delta_buf.clone(),
                            })
                            .to_string();
                            delta_buf.clear();
                            if sink.send(Message::Text(coalesced.into())).await.is_err() {
                                send_failed = true;
                                break;
                            }
                        }
                        if sink.send(Message::Text(s.into())).await.is_err() {
                            send_failed = true;
                            break;
                        }
                    }
                    OutboundFrame::Pong(p) => {
                        if sink.send(Message::Pong(p.into())).await.is_err() {
                            send_failed = true;
                            break;
                        }
                    }
                }
            }
            if !delta_buf.is_empty() && !send_failed {
                let coalesced = serde_json::json!({
                    "type": "content_delta",
                    "text": delta_buf.clone(),
                })
                .to_string();
                delta_buf.clear();
                if sink.send(Message::Text(coalesced.into())).await.is_err() {
                    send_failed = true;
                }
            }
            if send_failed {
                break;
            }
        }
        let _ = sink.close().await;
    });

    let session_key = format!("{GW_SESSION_PREFIX}{session_id}");

    let _ = send_json(
        &outbound_tx,
        &serde_json::json!({
            "type": "connected",
            "sessionId": session_id,
        }),
    )
    .await;

    {
        let initial_todos = if let Some(svc) = crate::services::try_get_services() {
            crate::tools::todo_write::session_todos(&svc.todo_store, &session_id)
        } else {
            Vec::new()
        };
        let _ = send_json(
            &outbound_tx,
            &serde_json::json!({
                "type": "todo_snapshot",
                "sessionId": session_id,
                "todos": initial_todos,
            }),
        )
        .await;
    }

    let config = {
        let mut cfg = state.config.lock();
        if super::super::desktop_routes::sanitize_active_profile_in_place(&mut cfg) {
            tracing::info!(
                "ws_desktop: sanitized stale default_provider/default_model in persisted config"
            );
        }
        cfg.clone()
    };
    state.live_config.store(config.clone());
    let mut agent = match crate::agent::Agent::from_config(
        &config,
        None,
        Some(state.live_config.clone()),
    )
    .await
    {
        Ok(a) => a,
        Err(e) => {
            let message = format!("agent init failed: {e}");
            let code = if message.contains("no_model_configured")
                || message.contains("未添加模型")
            {
                "NO_MODEL_CONFIGURED"
            } else {
                "AGENT_INIT_FAILED"
            };
            send_error(&outbound_tx, &message, code).await;
            drop(outbound_tx);
            let _ = writer_handle.await;
            return;
        }
    };

    agent.set_hook_runner(Some(std::sync::Arc::clone(&state.hooks)));

    state.hooks.fire_session_start(&session_id, "ws_desktop").await;
    if let Some(backend) = state.session_backend.clone() {
        let session_key_dir = session_key.clone();
        let dir_opt = tokio::task::spawn_blocking(move || {
            backend.get_session_work_dir(&session_key_dir).ok().flatten()
        })
        .await
        .ok()
        .flatten();
        if let Some(dir) = dir_opt {
            let trimmed = dir.trim();
            if !trimmed.is_empty() {
                agent.set_session_workspace_dir(std::path::PathBuf::from(trimmed));
            }
        }
    }
    agent.set_memory_session_id(Some(session_id.clone()));
    if config.nodes.enabled {
        agent.add_node_tools_from_registry(std::sync::Arc::clone(&state.node_registry));
    }
    if config.rbac.enabled
        && let Some(ref engine) = state.rbac
    {
        let identity = crate::security::rbac::CallerIdentity::from_gateway_session(&session_id);
        agent.set_rbac_session(Some(std::sync::Arc::clone(engine)), Some(identity));
    }

    if let Some(ref backend) = state.session_backend {
        let backend_arc = std::sync::Arc::clone(backend);
        let session_key_owned = session_key.clone();
        let messages = tokio::task::spawn_blocking(move || backend_arc.load(&session_key_owned))
            .await
            .unwrap_or_default();
        if !messages.is_empty() {
            agent.seed_history(&messages);
        }
    }

    let (inbound_tx, mut inbound_rx) =
        tokio::sync::mpsc::unbounded_channel::<serde_json::Value>();

    let inbound_tx_lsp = inbound_tx.clone();
    let inbound_tx_replay = inbound_tx.clone();
    let inbound_tx_gateway = inbound_tx.clone();
    let inbound_tx_resource = inbound_tx.clone();

    let cancel_signal_handle = agent.cancel_signal_handle();
    let cancelled_atomic = agent.cancel_token();
    let cancel_signal_for_reader = std::sync::Arc::clone(&cancel_signal_handle);
    let cancelled_atomic_for_reader = std::sync::Arc::clone(&cancelled_atomic);
    let outbound_tx_reader = outbound_tx.clone();
    let session_id_for_reader = session_id.clone();

    let reader_handle = tokio::spawn(async move {
        while let Some(frame) = receiver.next().await {
            match frame {
                Ok(Message::Text(text)) => {

                    let text_str: &str = text.as_str();
                    let parsed: serde_json::Value = match serde_json::from_str(text_str) {
                        Ok(v) => v,
                        Err(_) => {
                            let v = serde_json::json!({
                                "type": "__invalid_json__",
                                "raw": text_str.to_string(),
                            });
                            if inbound_tx.send(v).is_err() {
                                break;
                            }
                            continue;
                        }
                    };
                    let msg_type = parsed
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    if msg_type.as_str() == "ping" {
                        if outbound_tx_reader
                            .send(OutboundFrame::Text(
                                r#"{"type":"pong"}"#.to_string(),
                            ))
                            .await
                            .is_err()
                        {
                            break;
                        }
                        continue;
                    }
                    if msg_type.as_str() == "stop_generation" {
                        cancelled_atomic_for_reader
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                        cancel_signal_for_reader.load_full().cancel();
                        tracing::info!(
                            target: "agent_cancel",
                            "stop_generation received: cancel signal fired (reader-side)"
                        );

                        crate::tools::background_registry::kill_foreground(
                            session_id_for_reader.as_str(),
                        );

                        if let Some(sup) = crate::workers::supervisor::global_supervisor() {
                            let cancelled = sup.cancel_for_parent(&session_id_for_reader);
                            if cancelled > 0 {
                                tracing::info!(
                                    target: "agent_cancel",
                                    parent_session = %session_id_for_reader,
                                    cancelled,
                                    "cascading stop_generation to child workers"
                                );
                            }
                        }
                    }
                    if msg_type.as_str() == "cancel_tool" {
                        let target_session = parsed
                            .get("sessionId")
                            .or_else(|| parsed.get("session_id"))
                            .and_then(|v| v.as_str())
                            .unwrap_or(session_id_for_reader.as_str());
                        let killed = crate::tools::background_registry::kill_foreground(
                            target_session,
                        ) || (target_session != session_id_for_reader.as_str()
                            && crate::tools::background_registry::kill_foreground(
                                session_id_for_reader.as_str(),
                            ));
                        tracing::info!(
                            target: "agent_cancel",
                            session = %target_session,
                            killed,
                            "cancel_tool received: foreground shell kill requested"
                        );
                        continue;
                    }
                    match msg_type.as_str() {

                        "permission_response" | "computer_use_permission_response" => {
                            if let Some(request_id) =
                                parsed.get("requestId").and_then(|v| v.as_str())
                            {
                                let allowed = parsed
                                    .get("allowed")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                let updated_input = parsed
                                    .get("updatedInput")
                                    .or_else(|| parsed.get("updated_input"))
                                    .filter(|v| !v.is_null())
                                    .cloned();
                                let tool_name_hint = parsed
                                    .get("toolName")
                                    .and_then(|v| v.as_str())
                                    .map(str::to_string);
                                let evt = crate::session::SessionEvent::new(
                                    crate::session::SessionEventKind::ApprovalResponded {
                                        id: request_id.to_string(),
                                        decision: if allowed { "yes" } else { "no" }
                                            .to_string(),
                                        responder: Some("desktop".to_string()),
                                        updated_input: updated_input.clone(),
                                    },
                                );
                                let _ = super::approval_sender_for_desktop().send(evt);
                                if allowed
                                    && let Some(input) = updated_input.as_ref()
                                {
                                    let is_ask = matches!(
                                        tool_name_hint.as_deref(),
                                        Some("ask_question") | Some("ask_user")
                                    ) || input
                                        .get("questions")
                                        .map(|v| v.is_array())
                                        .unwrap_or(false)
                                        || input.get("answers").is_some();
                                    if is_ask
                                        && let Some(text) =
                                            super::ask_response_to_user_text(input)
                                    {
                                        let synthetic = serde_json::json!({
                                            "type": "user_message",
                                            "content": text,
                                            "synthetic": true,
                                            "source": "ask_response",
                                            "requestId": request_id,
                                        });
                                        if inbound_tx.send(synthetic).is_err() {
                                            break;
                                        }
                                    }
                                }
                                tracing::debug!(
                                    target: "ws_desktop_gate",
                                    request_id = %request_id,
                                    allowed,
                                    "desktop approval frame fast-pathed to bus"
                                );
                            }

                            continue;
                        }
                        _ => {}
                    }
                    if inbound_tx.send(parsed).is_err() {
                        break;
                    }
                }
                Ok(Message::Ping(payload)) => {
                    if outbound_tx_reader
                        .send(OutboundFrame::Pong(payload.to_vec()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(Message::Close(_)) | Err(_) => break,
                _ => {}
            }
        }
    });

    let lsp_forwarder = {
        let mut rx = state.lsp_events.subscribe();
        let tx = inbound_tx_lsp;
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if let Ok(payload) = serde_json::to_value(&event) {
                            let wrapped = serde_json::json!({
                                "type": "__lsp_forward__",
                                "payload": payload,
                            });
                            if tx.send(wrapped).is_err() {
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        })
    };

    let gateway_event_forwarder = {
        let mut rx = state.event_tx.subscribe();
        let tx = inbound_tx_gateway;
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(payload) => {
                        let is_forwardable = payload
                            .get("type")
                            .and_then(|v| v.as_str())
                            .map(|t| {
                                t == "system_notification" || t == "usage_updated"
                            })
                            .unwrap_or(false);
                        if !is_forwardable {
                            continue;
                        }
                        let wrapped = serde_json::json!({
                            "type": "__gateway_event__",
                            "payload": payload,
                        });
                        if tx.send(wrapped).is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        })
    };

    let resource_event_forwarder = {
        let mut rx = state.workspace_resources.subscribe();
        let tx = inbound_tx_resource.clone();
        let session_id_for_resource = session_id.clone();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        let payload =
                            resource_event_to_system_notification(&event, &session_id_for_resource);
                        let Some(payload) = payload else { continue };
                        let wrapped = serde_json::json!({
                            "type": "__gateway_event__",
                            "payload": payload,
                        });
                        if tx.send(wrapped).is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        })
    };

    {
        let workspace_key_resync = crate::session::workspace_key_from_path(
            agent.current_workspace_dir(),
            &session_id,
        );
        let waiters = state
            .workspace_resources
            .waiters_snapshot_for_session(&workspace_key_resync, &session_id);
        for waiter in waiters {
            let payload = serde_json::json!({
                "type": "system_notification",
                "subtype": "resource_wait_started",
                "sessionId": session_id,
                "data": {
                    "kind": waiter.kind,
                    "target": waiter.target,
                    "holderSessionId": waiter.holder_session_id,
                    "holderTitle": waiter.holder_title,
                },
            });
            let wrapped = serde_json::json!({
                "type": "__gateway_event__",
                "payload": payload,
            });
            let _ = inbound_tx_resource.send(wrapped);
        }
    }

    for warning in crate::gateway::snapshot_startup_warnings() {
        let payload = serde_json::json!({
            "type": "system_notification",
            "subtype": warning.subtype,
            "level": "warning",
            "message": warning.message,
        });
        let wrapped = serde_json::json!({
            "type": "__gateway_event__",
            "payload": payload,
        });
        let _ = inbound_tx_resource.send(wrapped);
    }

    {
        const REPLAY_BATCH: usize = 32;
        let svc = state.lsp.service();
        let cached = svc.get_all_diagnostics().await;
        let mut sent = 0usize;
        for (path, diags) in cached {
            if diags.is_empty() {
                continue;
            }
            let uri = crate::services::lsp::path_to_uri(&path);

            let wire_diags: Vec<serde_json::Value> = diags
                .iter()
                .map(|d| {
                    let severity = match d.severity {
                        crate::services::lsp::DiagnosticSeverity::Error => 1,
                        crate::services::lsp::DiagnosticSeverity::Warning => 2,
                        crate::services::lsp::DiagnosticSeverity::Information => 3,
                        crate::services::lsp::DiagnosticSeverity::Hint => 4,
                    };
                    serde_json::json!({
                        "range": {
                            "start": {
                                "line": d.range.start_line,
                                "character": d.range.start_character,
                            },
                            "end": {
                                "line": d.range.end_line,
                                "character": d.range.end_character,
                            },
                        },
                        "severity": severity,
                        "message": d.message,
                        "source": d.source,
                        "code": d.code,
                    })
                })
                .collect();
            let event = crate::lsp::LspBroadcastEvent::LspDiagnostics {
                server_id: "replay".to_string(),
                uri,
                version: None,
                diagnostics: serde_json::Value::Array(wire_diags),
            };
            if let Ok(payload) = serde_json::to_value(&event) {
                let wrapped = serde_json::json!({
                    "type": "__lsp_forward__",
                    "payload": payload,
                });
                let _ = inbound_tx_replay.send(wrapped);
            }
            sent += 1;
            if sent.is_multiple_of(REPLAY_BATCH) {
                tokio::task::yield_now().await;
            }
        }
    }

    while let Some(parsed) = inbound_rx.recv().await {
        if parsed.get("type").and_then(|v| v.as_str()) == Some("__invalid_json__") {
            let raw = parsed
                .get("raw")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            tracing::debug!(
                target: "ws_desktop_invalid_json",
                raw_full = %raw,
                "received malformed JSON from desktop ws client; preview sent to client",
            );
            send_error(
                &outbound_tx,
                &format!("invalid JSON: {} (...)", raw.chars().take(120).collect::<String>()),
                "INVALID_JSON",
            )
            .await;
            continue;
        }

        if parsed.get("type").and_then(|v| v.as_str()) == Some("__lsp_forward__") {
            if let Some(payload) = parsed.get("payload") {
                let _ = send_json(&outbound_tx, payload).await;
            }
            continue;
        }

        if parsed.get("type").and_then(|v| v.as_str()) == Some("__gateway_event__") {
            if let Some(payload) = parsed.get("payload") {
                let _ = send_json(&outbound_tx, payload).await;
            }
            continue;
        }

        let msg_type = parsed.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match msg_type {
            "prewarm_session" => {

                let _ = send_json(
                    &outbound_tx,
                    &serde_json::json!({
                        "type": "status",
                        "state": "idle",
                        "verb": "ready",
                    }),
                )
                .await;
            }
            "stop_generation" => {

                let _ = send_json(
                    &outbound_tx,
                    &serde_json::json!({
                        "type": "status",
                        "state": "idle",
                        "verb": "stopping",
                    }),
                )
                .await;
            }
            "set_permission_mode" => {
                let mode = parsed
                    .get("mode")
                    .and_then(|v| v.as_str())
                    .unwrap_or("ask");
                desktop_runtime_state().set_permission_mode(mode);
                let _ = send_json(
                    &outbound_tx,
                    &serde_json::json!({
                        "type": "system_notification",
                        "subtype": "permission_mode_updated",
                        "message": format!("Permission mode: {mode}"),
                    }),
                )
                .await;
            }
            "set_coding_mode" => {
                let mode_str = parsed.get("mode").and_then(|v| v.as_str()).unwrap_or("");
                let scope = parsed
                    .get("scope")
                    .and_then(|v| v.as_str())
                    .unwrap_or("session");
                let confirmed = parsed
                    .get("confirmed")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if let Some(parsed_mode) =
                    crate::agent::coding_mode::CodingMode::from_str_loose(mode_str)
                {
                    let svc_opt = crate::services::try_get_services();
                    let previous_mode = svc_opt
                        .map(|svc| svc.resolve_coding_mode_for(Some(&session_key)))
                        .unwrap_or_default();
                    let cfg_for_gate = svc_opt
                        .map(|svc| svc.config())
                        .unwrap_or_else(|| std::sync::Arc::new(state.config.lock().clone()));
                    let whitelist: &[String] =
                        cfg_for_gate.autonomy.auto_approve_mode_transitions.as_slice();
                    let needs_confirm = !confirmed
                        && previous_mode != parsed_mode
                        && !mode_transition_auto_approved(
                            whitelist,
                            previous_mode,
                            parsed_mode,
                        );
                    if needs_confirm {
                        let _ = send_json(
                            &outbound_tx,
                            &serde_json::json!({
                                "type": "system_notification",
                                "subtype": "coding_mode_transition_confirmation_required",
                                "message": format!(
                                    "Switching coding mode {} -> {} requires confirmation",
                                    previous_mode.display_name(),
                                    parsed_mode.display_name(),
                                ),
                                "data": {
                                    "from": previous_mode.display_name(),
                                    "to": parsed_mode.display_name(),
                                    "scope": scope,
                                    "whitelist": whitelist,
                                },
                            }),
                        )
                        .await;
                        continue;
                    }

                    if let Some(svc) = svc_opt {
                        svc.set_session_coding_mode(&session_key, parsed_mode);
                        if scope == "global" {
                            *svc.coding_mode.write() = parsed_mode;
                        }
                    }
                    agent.set_coding_mode(parsed_mode);
                    let derived = super::super::desktop_routes::derive_permission_from_coding(&parsed_mode);
                    desktop_runtime_state().set_permission_mode(derived);
                    let _ = send_json(
                        &outbound_tx,
                        &serde_json::json!({
                            "type": "system_notification",
                            "subtype": "coding_mode_updated",
                            "message": format!("Coding mode: {}", parsed_mode.label()),
                            "data": {
                                "mode": parsed_mode.display_name(),
                                "label": parsed_mode.label(),
                                "permissionMode": derived,
                                "scope": scope,
                                "from": previous_mode.display_name(),
                                "autoApproved": !confirmed
                                    || mode_transition_auto_approved(
                                        whitelist,
                                        previous_mode,
                                        parsed_mode,
                                    ),
                            },
                        }),
                    )
                    .await;
                } else {
                    send_error(
                        &outbound_tx,
                        &format!("unknown coding mode: {mode_str}"),
                        "UNKNOWN_CODING_MODE",
                    )
                    .await;
                }
            }
            "set_pii_config" => {
                let payload = parsed.get("data").cloned().unwrap_or(serde_json::Value::Null);
                let cfg =
                    crate::services::governance::pii_sanitizer::PiiSanitizerConfig::from_settings(&payload);
                let cfg_for_persist = cfg.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    crate::services::governance::pii_sanitizer::update_global_config(cfg_for_persist)
                })
                .await;
                let labels: Vec<String> = cfg
                    .disabled_kinds
                    .iter()
                    .map(|k| k.label().to_string())
                    .collect();
                let _ = send_json(
                    &outbound_tx,
                    &serde_json::json!({
                        "type": "system_notification",
                        "subtype": "pii_config_updated",
                        "data": {
                            "enabled": cfg.enabled,
                            "disabledKinds": labels,
                        },
                    }),
                )
                .await;
            }
            "set_runtime_config" => {

                let provider = parsed
                    .get("providerId")
                    .or_else(|| parsed.get("provider"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                let model = parsed
                    .get("modelId")
                    .or_else(|| parsed.get("model"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());

                let persist = parsed
                    .get("persist")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let snapshot = {
                    let mut cfg = state.config.lock();

                    if let Some(p) = provider.as_deref() {
                        if let Some(profile) = cfg.model_providers.get(p).cloned() {
                            crate::gateway::desktop_routes::apply_active_profile_to_top_level(
                                &mut cfg, p, &profile,
                            );
                        } else {
                            cfg.default_provider = Some(p.to_string());
                        }
                    }
                    if let Some(m) = model.as_ref() {
                        cfg.default_model = Some(m.clone());
                    }
                    cfg.clone()
                };

                if persist {
                    if let Err(e) = snapshot.save().await {
                        tracing::warn!(
                            target: "ws_desktop_runtime_config",
                            error = %e,
                            "set_runtime_config: failed to persist composer-initiated runtime config to disk; \
                             change remains effective in-memory only until restart"
                        );
                        let _ = send_json(
                            &outbound_tx,
                            &serde_json::json!({
                                "type": "system_notification",
                                "subtype": "runtime_config_persist_failed",
                                "message": format!("{e:#}"),
                            }),
                        )
                        .await;
                    }
                }

                state.push_live_config(snapshot);
                state.rebuild_runtime_from_config_async().await;
                if let (Some(p), Some(m)) = (provider.as_ref(), model.as_ref()) {
                    agent.signal_runtime_model_switch(p.clone(), m.clone());
                }
                if let Err(e) = agent.apply_runtime_config_now().await {
                    tracing::warn!(
                        target: "ws_desktop_runtime_config",
                        error = %e,
                        "set_runtime_config: failed to apply runtime config to session agent"
                    );
                }
                let _ = send_json(
                    &outbound_tx,
                    &serde_json::json!({
                        "type": "system_notification",
                        "subtype": "runtime_config_updated",
                        "data": {
                            "persisted": persist,
                        },
                    }),
                )
                .await;
            }
            "permission_response" | "computer_use_permission_response" => {

                if let Some(request_id) = parsed.get("requestId").and_then(|v| v.as_str()) {
                    let allowed = parsed
                        .get("allowed")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let updated_input = parsed
                        .get("updatedInput")
                        .or_else(|| parsed.get("updated_input"))
                        .filter(|v| !v.is_null())
                        .cloned();
                    let evt = crate::session::SessionEvent::new(
                        crate::session::SessionEventKind::ApprovalResponded {
                            id: request_id.to_string(),
                            decision: if allowed { "yes" } else { "no" }.to_string(),
                            responder: Some("desktop".to_string()),
                            updated_input,
                        },
                    );
                    let _ = super::approval_sender_for_desktop().send(evt);
                }
            }
            "debug_bind_tab" => {
                let tab_id = parsed
                    .get("tab_id")
                    .or_else(|| parsed.get("tabId"))
                    .and_then(|v| v.as_u64());
                let Some(tab_id) = tab_id else {
                    send_error(
                        &outbound_tx,
                        "debug_bind_tab.tab_id is required",
                        "EMPTY_TAB_ID",
                    )
                    .await;
                    continue;
                };
                if let Some(ctl) = crate::tools::browser::dock_controller() {
                    if let Err(err) = ctl
                        .bind_tab_to_session(session_id.clone(), tab_id as u32)
                        .await
                    {
                        send_error(
                            &outbound_tx,
                            &format!("debug_bind_tab failed: {err}"),
                            "DOCK_BIND_FAILED",
                        )
                        .await;
                        continue;
                    }
                }
                let _ = send_json(
                    &outbound_tx,
                    &serde_json::json!({
                        "type": "system_notification",
                        "subtype": "debug_tab_bound",
                        "data": { "tab_id": tab_id },
                    }),
                )
                .await;
            }
            "debug_unbind_tab" => {
                let tab_id = parsed
                    .get("tab_id")
                    .or_else(|| parsed.get("tabId"))
                    .and_then(|v| v.as_u64());
                let Some(tab_id) = tab_id else {
                    send_error(
                        &outbound_tx,
                        "debug_unbind_tab.tab_id is required",
                        "EMPTY_TAB_ID",
                    )
                    .await;
                    continue;
                };
                if let Some(ctl) = crate::tools::browser::dock_controller() {
                    if let Err(err) = ctl
                        .unbind_tab_from_session(session_id.clone(), tab_id as u32)
                        .await
                    {
                        send_error(
                            &outbound_tx,
                            &format!("debug_unbind_tab failed: {err}"),
                            "DOCK_UNBIND_FAILED",
                        )
                        .await;
                        continue;
                    }
                }
                let _ = send_json(
                    &outbound_tx,
                    &serde_json::json!({
                        "type": "system_notification",
                        "subtype": "debug_tab_unbound",
                        "data": { "tab_id": tab_id },
                    }),
                )
                .await;
            }
            "debug_bind_prototype_ref" => {
                let tab_id = parsed
                    .get("tab_id")
                    .or_else(|| parsed.get("tabId"))
                    .and_then(|v| v.as_u64());
                let Some(tab_id) = tab_id else {
                    send_error(
                        &outbound_tx,
                        "debug_bind_prototype_ref.tab_id is required",
                        "EMPTY_TAB_ID",
                    )
                    .await;
                    continue;
                };
                crate::tools::browser::set_prototype_ref_tab(&session_key, tab_id as u32);
                let _ = send_json(
                    &outbound_tx,
                    &serde_json::json!({
                        "type": "system_notification",
                        "subtype": "prototype_ref_bound",
                        "data": { "tab_id": tab_id },
                    }),
                )
                .await;
            }
            "debug_unbind_prototype_ref" => {
                crate::tools::browser::clear_prototype_ref_tab(&session_key);
                let _ = send_json(
                    &outbound_tx,
                    &serde_json::json!({
                        "type": "system_notification",
                        "subtype": "prototype_ref_unbound",
                        "data": {},
                    }),
                )
                .await;
            }
            "user_message" => {
                let content = parsed
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if content.is_empty() {
                    send_error(&outbound_tx, "empty user_message.content", "EMPTY_CONTENT").await;
                    continue;
                }

                if let Some(ref backend) = state.session_backend {
                    let backend_arc = std::sync::Arc::clone(backend);
                    let session_key_owned = session_key.clone();
                    let user_msg = crate::providers::ChatMessage::user(&content);
                    let _ = tokio::task::spawn_blocking(move || {
                        backend_arc.append(&session_key_owned, &user_msg)
                    })
                    .await;
                }

                if let Some(svc) = crate::services::try_get_services() {
                    let resolved = svc.resolve_coding_mode_for(Some(&session_key));
                    agent.set_coding_mode(resolved);
                }

                if let Err(e) = agent.apply_runtime_config_now().await {
                    tracing::warn!(
                        target: "ws_desktop_runtime_config",
                        error = %e,
                        "user_message: failed to apply live runtime config before turn"
                    );
                }

                agent.reset_cancel();
                run_turn(&state, &mut agent, &outbound_tx, &session_id, &session_key, &content).await;
            }
            "start_plan_execution" => {

                let plan_path = parsed
                    .get("planPath")
                    .or_else(|| parsed.get("plan_path"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let resume = parsed
                    .get("resume")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let is_curator = parsed
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .map(|k| k.eq_ignore_ascii_case("curator"))
                    .unwrap_or(false);
                if plan_path.is_empty() {
                    send_error(
                        &outbound_tx,
                        "empty start_plan_execution.planPath",
                        "EMPTY_PLAN_PATH",
                    )
                    .await;
                    continue;
                }

                let agent_mode = crate::agent::coding_mode::CodingMode::Agent;
                if let Some(svc) = crate::services::try_get_services() {
                    svc.set_session_coding_mode(&session_key, agent_mode);

                    *svc.coding_mode.write() = agent_mode;
                }
                agent.set_coding_mode(agent_mode);
                let derived =
                    super::super::desktop_routes::derive_permission_from_coding(&agent_mode);
                desktop_runtime_state().set_permission_mode(derived);
                let _ = send_json(
                    &outbound_tx,
                    &serde_json::json!({
                        "type": "system_notification",
                        "subtype": "coding_mode_updated",
                        "message": format!("Coding mode: {}", agent_mode.label()),
                        "data": {
                            "mode": agent_mode.display_name(),
                            "label": agent_mode.label(),
                            "permissionMode": derived,
                        },
                    }),
                )
                .await;

                let workspace_dir = agent.current_workspace_dir().to_path_buf();
                let plans_dir = {
                    let last = workspace_dir
                        .file_name()
                        .and_then(|s| s.to_str())
                        .map(str::to_lowercase);
                    let base = if last.as_deref() == Some("src-tauri")
                        && workspace_dir
                            .parent()
                            .map(|p| p.join("src-tauri").join("tauri.conf.json").exists())
                            .unwrap_or(false)
                    {
                        workspace_dir
                            .parent()
                            .map_or_else(|| workspace_dir.clone(), std::path::Path::to_path_buf)
                    } else {
                        workspace_dir.clone()
                    };
                    base.join(".senweavercoding").join("plans")
                };

                let plan_name = std::path::Path::new(&plan_path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(plan_path.as_str())
                    .trim_end_matches(".plan.md")
                    .to_string();

                let resolved_plan_path: std::path::PathBuf = {
                    let raw = std::path::Path::new(&plan_path);
                    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
                    if raw.is_absolute() {
                        candidates.push(raw.to_path_buf());
                    } else {
                        candidates.push(plans_dir.join(raw));
                        candidates.push(workspace_dir.join(raw));
                    }
                    candidates.push(plans_dir.join(format!("{plan_name}.plan.md")));
                    candidates
                        .iter()
                        .find(|p| p.exists())
                        .cloned()
                        .unwrap_or_else(|| plans_dir.join(format!("{plan_name}.plan.md")))
                };
                let plan_read_display = resolved_plan_path.to_string_lossy().replace('\\', "/");

                let plan_excerpt: String = {
                    let resolved = resolved_plan_path.clone();
                    let truncation_hint = if is_curator {
                        format!(
                            "[... blueprint truncated; call `file_read(\"{plan_read_display}\")` \
                             for the full document ...]"
                        )
                    } else {
                        format!(
                            "[... plan file truncated; call \
                             `update_plan(action=\"load\", plan_name=\"{plan_name}\")` \
                             for the full document ...]"
                        )
                    };
                    match tokio::task::spawn_blocking(move || std::fs::read_to_string(&resolved))
                        .await
                    {
                        Ok(Ok(text)) => {
                            let trimmed = text.trim();
                            if trimmed.is_empty() {
                                String::new()
                            } else if trimmed.chars().count() > 12000 {
                                let head: String = trimmed.chars().take(12000).collect();
                                format!("{head}\n\n{truncation_hint}")
                            } else {
                                trimmed.to_string()
                            }
                        }
                        _ => String::new(),
                    }
                };
                let doc_block_label = if is_curator {
                    "IMPLEMENTATION BLUEPRINT"
                } else {
                    "PLAN DOCUMENT"
                };
                let plan_doc_block = if plan_excerpt.is_empty() {
                    String::new()
                } else {
                    format!(
                        "\n\n--- BEGIN {doc_block_label} (`{plan_read_display}`) ---\n{plan_excerpt}\n\
                         --- END {doc_block_label} ---"
                    )
                };
                let exclusive_task_header = if is_curator {
                    format!(
                        "[Curator build \u{2014} EXCLUSIVE TASK FOR THIS TURN] \
                        This turn exists ONLY to implement ONE specific curator deliverable: the \
                        implementation blueprint at `{plan_read_display}`, whose full content is \
                        included below. Do NOT author a new curator document, do NOT re-enter \
                        curator mode, do NOT search for other plans or blueprints \u{2014} the \
                        correct blueprint is already given to you here. The conversation history \
                        may also contain earlier, unrelated user messages (greetings, \\\"what \
                        model are you\\\", side questions, prior finished tasks). Those are STALE \
                        CONTEXT \u{2014} do NOT answer them. Your one and only job right now is to \
                        turn this blueprint into working code, tracked step by step via \
                        `update_plan`.\n\n"
                    )
                } else {
                    format!(
                        "[Plan execution \u{2014} EXCLUSIVE TASK FOR THIS TURN] \
                        This turn exists ONLY to execute ONE specific saved plan: the plan named \
                        `{plan_name}` (file `{plan_read_display}`), whose full content is included \
                        below. Do NOT search for other plans, do NOT call `update_plan(action=\\\"list\\\")`, \
                        and do NOT open curator / spec / blueprint / `final.md` documents \u{2014} the \
                        correct plan is already given to you here. The conversation history may also \
                        contain earlier, unrelated user messages (greetings, \\\"what model are you\\\", \
                        side questions, prior finished tasks). Those are STALE CONTEXT \u{2014} do NOT \
                        answer them. To (re)load the tracker for THIS plan, the ONLY correct call is \
                        `update_plan(action=\\\"load\\\", plan_name=\\\"{plan_name}\\\")` \u{2014} never guess \
                        a different plan name or path. Your one and only job right now is to carry out \
                        this plan.\n\n"
                    )
                };

                let trigger_content = if is_curator {
                    if resume {
                        format!(
                            "{exclusive_task_header}\
                             [Curator build resume  - Agent mode]\n\
                             The user clicked **Continue** because the previous build turn ended \
                             with unfinished steps. The blueprint is `{plan_read_display}`.\n\n\
                             Your job for this turn:\n\
                             1. **Inspect current progress**  - call `update_plan(action=\"get\")`. \
                                If it returns an EMPTY tracker (for example after an app restart), \
                                re-derive the concrete, ordered implementation steps from the \
                                embedded blueprint and register them with ONE \
                                `update_plan(action=\"set\")` call, then continue. If `get` already \
                                shows steps, do NOT call `set` again  - that would wipe completion \
                                status. Look at which steps are `completed` / `skipped` / \
                                `in_progress` / `pending`.\n\
                             2. **Resume each remaining step ONE AT A TIME, in order**. For every \
                                step that is not yet `completed` or `skipped`:\n\
                                a. Call `update_plan(action=\"update\", step_id=<id>, \
                                   status=\"in_progress\")`.\n\
                                b. Perform the actual edits / shell commands for THIS step only.\n\
                                c. Call `update_plan(action=\"update\", step_id=<id>, \
                                   status=\"completed\")` (or `\"skipped\"` with a `notes` reason).\n\
                                Only THEN start the next step's `in_progress` mark.\n\
                             3. **Run the verification commands** named in the blueprint before \
                                declaring done.\n\n\
                             **CRITICAL - never batch status flips at the end of the turn.** The \
                             user is watching a live progress bar fed by every `update_plan` call. \
                             Each step's `completed` flip must visibly precede the next step's \
                             `in_progress` flip.\n\n\
                             Do NOT stop, do NOT ask for confirmation, do NOT summarise what's \
                             already done. Work straight through every remaining step.{plan_doc_block}"
                        )
                    } else {
                        format!(
                            "{exclusive_task_header}\
                             [Curator build trigger  - Agent mode]\n\
                             The user clicked **Build** on the curator card to implement the \
                             blueprint `{plan_read_display}`.\n\n\
                             Your job for this turn:\n\
                             1. **Create the live step tracker FIRST**  - read the embedded \
                                blueprint below, break its implementation work into concrete, \
                                ordered, verifiable steps, and register them with EXACTLY ONE \
                                `update_plan(action=\"set\")` call. This single call creates the \
                                progress bar the user is watching. Do NOT call \
                                `update_plan(action=\"load\")` (there is no `.plan.md`); after the \
                                one `set`, use `update` only.\n\
                             2. **Execute each step ONE AT A TIME, in order**. For every step:\n\
                                a. Call `update_plan(action=\"update\", step_id=<id>, \
                                   status=\"in_progress\")`.\n\
                                b. Perform the actual edits / shell commands for THIS step only, \
                                   following the blueprint faithfully.\n\
                                c. Call `update_plan(action=\"update\", step_id=<id>, \
                                   status=\"completed\")` (or `\"skipped\"` with a `notes` reason if \
                                   a step turns out unnecessary).\n\
                                Only THEN start the next step's `in_progress` mark.\n\
                             3. **Run the verification commands** named in the blueprint before \
                                declaring done.\n\n\
                             **CRITICAL - never batch status flips at the end of the turn.** The \
                             user is watching a live progress bar fed by every `update_plan` call. \
                             Each step's `completed` flip must visibly precede the next step's \
                             `in_progress` flip.\n\n\
                             Do NOT narrate the mode switch, do NOT re-ask questions already \
                             answered. Start by setting up the tracker, then execute step 1 \
                             immediately.{plan_doc_block}"
                        )
                    }
                } else if resume {
                    format!(
                        "{exclusive_task_header}\
                         [Plan execution resume  - Agent mode]\n\
                         The user clicked **Continue** on the plan card because the \
                         previous execution turn ended with unfinished todos. The plan is \
                         `{plan_name}` (file `{plan_read_display}`).\n\n\
                         Your job for this turn:\n\
                         1. **Inspect current progress**  - call \
                            `update_plan(action=\"get\")`. If it returns an EMPTY plan (for \
                            example after an app restart the in-memory tracker is gone), \
                            call `update_plan(action=\"load\", plan_name=\"{plan_name}\")` \
                            ONCE to reload THIS exact plan from disk, then continue. If \
                            `get` already shows the steps, do NOT call `load` or `set`  - \
                            that would wipe completion status. Either way, look at which \
                            steps are `completed` / `skipped` / `in_progress` / `pending`.\n\
                         2. **Resume each remaining step ONE AT A TIME, in order**. For \
                            every step that is not yet `completed` or `skipped`, do these \
                            three things back-to-back BEFORE moving on to the next step:\n\
                            a. Call `update_plan(action=\"update\", step_id=<id>, \
                               status=\"in_progress\")`.\n\
                            b. Perform the actual edits / shell commands for THIS step \
                               only.\n\
                            c. Call `update_plan(action=\"update\", step_id=<id>, \
                               status=\"completed\")` (or `\"skipped\"` with a `notes` \
                               reason if it's no longer needed).\n\
                            Only THEN start the next step's `in_progress` mark.\n\
                         3. **Run the verification commands** in the plan's `## Verification` / \
                            Verification section before declaring done.\n\n\
                         **CRITICAL - never batch status flips at the end of the turn.** \
                         The user is watching a live progress bar fed by every \
                         `update_plan` call. If you do all the real work first and then \
                         fire a flurry of `update_plan(action=\"update\", \
                         status=\"completed\")` calls in a row at the end, the bar stays \
                         stuck and then jumps to 100% in one frame - that is exactly the \
                         failure mode this prompt forbids. Each step's `completed` flip \
                         must visibly precede the next step's `in_progress` flip.\n\n\
                         Do NOT stop, do NOT ask for confirmation, do NOT summarise \
                         what's already done. Work straight through every remaining \
                         step.{plan_doc_block}"
                    )
                } else {
                    format!(
                        "{exclusive_task_header}\
                         [Plan execution trigger  - Agent mode]\n\
                         The user clicked **Build** on the plan card. The finalised plan is \
                         `{plan_name}` (file `{plan_read_display}`).\n\n\
                         Your job for this turn:\n\
                         1. **Load + hydrate the plan in ONE step**  - call \
                            `update_plan(action=\"load\", plan_name=\"{plan_name}\")` exactly \
                            once. This reads THIS exact plan file from disk AND populates the \
                            in-memory tracker verbatim (same ids / content as the user's \
                            progress bar), so you do NOT need `file_read` and you do NOT need \
                            `update_plan(action=\"set\")`. Do NOT search for or load any other \
                            plan name. After this single `load`, never call `load`/`set` \
                            again this turn  - use `update` only.\n\
                         2. **Execute each todo ONE AT A TIME, in order**. For every step, \
                            do these three things back-to-back BEFORE moving on to the next \
                            step:\n\
                            a. Call `update_plan(action=\"update\", step_id=<id>, \
                               status=\"in_progress\")`.\n\
                            b. Perform the actual edits / shell commands for THIS step \
                               only.\n\
                            c. Call `update_plan(action=\"update\", step_id=<id>, \
                               status=\"completed\")` (or `\"skipped\"` with a `notes` \
                               reason if the step turns out unnecessary).\n\
                            Only THEN start the next step's `in_progress` mark.\n\
                         3. **Run the verification commands** in the `## Verification` section \
                            before declaring done.\n\n\
                         **CRITICAL - never batch status flips at the end of the turn.** \
                         The user is watching a live progress bar fed by every \
                         `update_plan` call. If you do all the real work first and then \
                         fire a flurry of `update_plan(action=\"update\", \
                         status=\"completed\")` calls in a row at the end, the bar stays \
                         stuck and then jumps to 100% in one frame - that is exactly the \
                         failure mode this prompt forbids. Each step's `completed` flip \
                         must visibly precede the next step's `in_progress` flip.\n\n\
                         Do NOT narrate the mode switch, do NOT re-ask questions the user \
                         already answered in Plan mode. Start with step 1 immediately.\
                         {plan_doc_block}"
                    )
                };

                if let Some(ref backend) = state.session_backend {
                    let backend_arc = std::sync::Arc::clone(backend);
                    let session_key_owned = session_key.clone();
                    let trigger_msg =
                        crate::providers::ChatMessage::user(&trigger_content);
                    let _ = tokio::task::spawn_blocking(move || {
                        backend_arc.append_hidden(&session_key_owned, &trigger_msg)
                    })
                    .await;
                }

                if let Some(ref backend) = state.session_backend {
                    let backend_arc = std::sync::Arc::clone(backend);
                    let session_key_owned = session_key.clone();
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    let marker_payload = serde_json::json!([
                        {
                            "type": "mode_switch",
                            "plan_path": plan_path.clone(),
                            "target_mode": "agent",
                            "handoff_kind": if is_curator { "curator" } else { "plan" },
                            "status": "switched",
                            "resume": resume,
                            "timestamp_ms": now_ms,
                        }
                    ])
                    .to_string();
                    let marker_msg =
                        crate::providers::ChatMessage::assistant(marker_payload);
                    let _ = tokio::task::spawn_blocking(move || {
                        backend_arc.append(&session_key_owned, &marker_msg)
                    })
                    .await;
                }

                {
                    let snap = state.config.lock().clone();
                    state.push_live_config(snap);
                }

                agent.arm_plan_execution(plan_path.clone());

                agent.reset_cancel();
                run_turn(
                    &state,
                    &mut agent,
                    &outbound_tx,
                    &session_id,
                    &session_key,
                    &trigger_content,
                )
                .await;
            }
            other => {
                send_error(
                    &outbound_tx,
                    &format!("unsupported message type: {other}"),
                    "UNKNOWN_MESSAGE_TYPE",
                )
                .await;
            }
        }
    }

    reader_handle.abort();
    lsp_forwarder.abort();
    gateway_event_forwarder.abort();
    resource_event_forwarder.abort();
    drop(outbound_tx);
    let _ = writer_handle.await;

    if let Some(svc) = crate::services::try_get_services() {
        svc.clear_session_coding_mode(&session_key);
    }

    let _ = crate::services::governance::credential_vault::purge_session_ephemeral(&session_key);
    if let Some(ctl) = crate::tools::browser::dock_controller() {
        let session_key_for_release = session_key.clone();
        tokio::spawn(async move {
            if let Err(err) = ctl
                .release_agent_tabs_for_session(session_key_for_release)
                .await
            {
                tracing::warn!(
                    "[ws_desktop] release_agent_tabs_for_session failed: {err}"
                );
            }
        });
    }

    state.hooks.fire_session_end(&session_id, "ws_desktop").await;
    if let Some(engine) = crate::evolution::try_global() {
        let snapshot = engine.config_snapshot();
        if snapshot.reflection.enabled
            && matches!(
                snapshot.reflection.trigger_mode,
                crate::evolution::ReflectionTriggerMode::Auto
            )
        {
            engine.schedule_session_reflection(
                &session_id,
                crate::evolution::ReflectionTriggerCause::SessionEnd,
            );
        }
        if snapshot.auto_distill_on_session_end {
            let _ = snapshot;
        }
    }
}

pub struct DesktopRuntimeState {
    permission_mode: parking_lot::RwLock<String>,
    settings_path: parking_lot::RwLock<Option<std::path::PathBuf>>,
    hydrated: std::sync::atomic::AtomicBool,
}

impl DesktopRuntimeState {
    fn new() -> Self {
        Self {

            permission_mode: parking_lot::RwLock::new("default".to_string()),
            settings_path: parking_lot::RwLock::new(None),
            hydrated: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn set_settings_path(&self, path: std::path::PathBuf) {
        *self.settings_path.write() = Some(path);
    }

    fn hydrate_if_needed(&self) {
        use std::sync::atomic::Ordering;
        if self.hydrated.load(Ordering::Acquire) {
            return;
        }
        let path = self.settings_path.read().clone();
        if let Some(p) = path {
            if let Ok(contents) = std::fs::read_to_string(&p) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) {
                    if let Some(s) = json.get("permissionMode").and_then(|v| v.as_str()) {
                        if !s.is_empty() {
                            *self.permission_mode.write() = s.to_string();
                        }
                    }
                }
            }
        }
        self.hydrated.store(true, Ordering::Release);
    }

    pub fn permission_mode(&self) -> String {
        self.hydrate_if_needed();
        self.permission_mode.read().clone()
    }

    pub fn set_permission_mode(&self, mode: &str) {

        self.hydrated
            .store(true, std::sync::atomic::Ordering::Release);
        *self.permission_mode.write() = mode.to_string();
        let path = self.settings_path.read().clone();
        if let Some(p) = path {
            let mode_owned = mode.to_string();
            let path_for_log = p.clone();
            tokio::task::spawn(async move {
                let res = tokio::task::spawn_blocking(move || {
                    persist_permission_mode_to_disk(&p, &mode_owned)
                })
                .await;
                if let Ok(Err(err)) = res {
                    tracing::warn!(
                        error = %err,
                        path = %path_for_log.display(),
                        "[desktop] failed to persist permission_mode to desktop_user.json"
                    );
                }
            });
        }
    }
}

fn persist_permission_mode_to_disk(
    path: &std::path::Path,
    mode: &str,
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut json: serde_json::Value = match std::fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| serde_json::json!({})),
        Err(_) => serde_json::json!({}),
    };
    if !json.is_object() {
        json = serde_json::json!({});
    }
    if let Some(obj) = json.as_object_mut() {
        obj.insert(
            "permissionMode".to_string(),
            serde_json::Value::String(mode.to_string()),
        );
    }
    let serialized =
        serde_json::to_string_pretty(&json).unwrap_or_else(|_| json.to_string());
    std::fs::write(path, serialized)
}

pub fn desktop_runtime_state() -> &'static DesktopRuntimeState {
    static STATE: std::sync::OnceLock<DesktopRuntimeState> = std::sync::OnceLock::new();
    STATE.get_or_init(DesktopRuntimeState::new)
}

async fn send_json(outbound: &OutboundSender, value: &serde_json::Value) {
    let _ = outbound.send(OutboundFrame::Text(value.to_string())).await;
}

async fn send_error(outbound: &OutboundSender, message: &str, code: &str) {
    send_json(
        outbound,
        &crate::agent::error_classify::user_facing_error_json(message, code),
    )
    .await;
}

struct PersistJob {
    session_key: String,
    rows: Vec<crate::providers::ChatMessage>,
}

static PERSIST_PENDING: AtomicUsize = AtomicUsize::new(0);

const MAX_PERSIST_RETRIES: u32 = 10;

pub(crate) async fn wait_persist_drained(deadline: std::time::Duration) -> bool {
    let start = std::time::Instant::now();
    loop {
        if PERSIST_PENDING.load(Ordering::SeqCst) == 0 {
            return true;
        }
        if start.elapsed() >= deadline {
            tracing::warn!(
                target: "ws_desktop_persist",
                pending = PERSIST_PENDING.load(Ordering::SeqCst),
                "session persist queue did not drain within shutdown deadline"
            );
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

fn persist_sender(
    backend: &std::sync::Arc<dyn crate::channels::session::backend::SessionBackend>,
) -> &'static mpsc::UnboundedSender<PersistJob> {
    static SENDER: std::sync::OnceLock<mpsc::UnboundedSender<PersistJob>> =
        std::sync::OnceLock::new();
    SENDER.get_or_init(|| {
        let (tx, mut rx) = mpsc::unbounded_channel::<PersistJob>();
        let backend = std::sync::Arc::clone(backend);
        tokio::spawn(async move {
            let mut retry_queue: std::collections::VecDeque<(PersistJob, u32)> =
                std::collections::VecDeque::new();
            loop {
                let (job, attempts) = if let Some(item) = retry_queue.pop_front() {
                    item
                } else {
                    match rx.recv().await {
                        Some(job) => (job, 0u32),
                        None => break,
                    }
                };

                let backend_for_write = std::sync::Arc::clone(&backend);
                let outcome = tokio::task::spawn_blocking(move || {
                    let PersistJob { session_key, rows } = job;
                    for (idx, msg) in rows.iter().enumerate() {
                        if let Err(e) = backend_for_write.append(&session_key, msg) {
                            let leftover = rows[idx..].to_vec();
                            return Some((
                                PersistJob {
                                    session_key,
                                    rows: leftover,
                                },
                                e.to_string(),
                            ));
                        }
                    }
                    None
                })
                .await;

                match outcome {
                    Ok(None) => {
                        PERSIST_PENDING.fetch_sub(1, Ordering::SeqCst);
                    }
                    Ok(Some((leftover_job, err))) => {
                        let next_attempts = attempts + 1;
                        if next_attempts >= MAX_PERSIST_RETRIES {
                            tracing::error!(
                                target: "ws_desktop_persist",
                                error = %err,
                                session_key = %leftover_job.session_key,
                                dropped = leftover_job.rows.len(),
                                attempts = next_attempts,
                                "session persist append failed permanently; dropping batch after max retries"
                            );
                            PERSIST_PENDING.fetch_sub(1, Ordering::SeqCst);
                        } else {
                            tracing::warn!(
                                target: "ws_desktop_persist",
                                error = %err,
                                session_key = %leftover_job.session_key,
                                pending = leftover_job.rows.len(),
                                attempt = next_attempts,
                                "session persist append failed; retrying in background"
                            );
                            retry_queue.push_back((leftover_job, next_attempts));
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        }
                    }
                    Err(join_err) => {
                        tracing::warn!(
                            target: "ws_desktop_persist",
                            error = %join_err,
                            "session persist worker join error; dropping batch"
                        );
                        PERSIST_PENDING.fetch_sub(1, Ordering::SeqCst);
                    }
                }
            }
        });
        tx
    })
}

fn describe_panic(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

fn enqueue_persist(
    state: &AppState,
    session_key: &str,
    rows: Vec<crate::providers::ChatMessage>,
) {
    if rows.is_empty() {
        return;
    }
    let Some(backend) = state.session_backend.as_ref() else {
        return;
    };
    PERSIST_PENDING.fetch_add(1, Ordering::SeqCst);
    if persist_sender(backend)
        .send(PersistJob {
            session_key: session_key.to_string(),
            rows,
        })
        .is_err()
    {
        PERSIST_PENDING.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct DesktopSqlitePersist {
    thinking_buf: String,
    thinking_segment_started_ms: Option<u64>,
    text_buf: String,
    assistant_segment: Vec<serde_json::Value>,
    out: Vec<crate::providers::ChatMessage>,
}

impl DesktopSqlitePersist {
    fn wallclock_ms_unix() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    fn absorb_pending_text_into_segment(&mut self) {
        self.absorb_pending_thinking();
        self.absorb_pending_text();
    }

    fn finalize_assistant_segment(&mut self) {
        if self.assistant_segment.is_empty() {
            return;
        }
        if let Ok(s) = serde_json::to_string(&self.assistant_segment) {
            self.out
                .push(crate::providers::ChatMessage::assistant(s));
        }
        self.assistant_segment.clear();
    }

    fn absorb_pending_text(&mut self) {
        let pending = self.text_buf.trim_end();
        if pending.is_empty() {
            self.text_buf.clear();
            return;
        }
        self.assistant_segment
            .push(json!({ "type": "text", "text": pending }));
        self.text_buf.clear();
    }

    const MAX_TEXT_BUF_BYTES: usize = 1024 * 1024;

    fn on_chunk(&mut self, delta: &str) {
        if !self.thinking_buf.is_empty() {
            self.absorb_pending_text();
            self.absorb_pending_thinking();
        }
        self.text_buf.push_str(delta);
        if self.text_buf.len() > Self::MAX_TEXT_BUF_BYTES {
            self.absorb_pending_text();
        }
    }

    fn reset_stream(&mut self) {
        self.text_buf.clear();
    }

    fn discard_pending_thinking(&mut self) {
        self.thinking_buf.clear();
        self.thinking_segment_started_ms = None;
    }

    fn absorb_pending_thinking(&mut self) {
        let pending = self.thinking_buf.trim_end();
        if pending.is_empty() {
            self.thinking_buf.clear();
            self.thinking_segment_started_ms = None;
            return;
        }
        let completed_ms = Self::wallclock_ms_unix();
        let mut obj = serde_json::Map::new();
        obj.insert("type".to_string(), serde_json::json!("thinking"));
        obj.insert(
            "thinking".to_string(),
            serde_json::Value::String(pending.to_string()),
        );
        obj.insert(
            "completed_at_ms".to_string(),
            serde_json::Value::from(completed_ms),
        );
        if let Some(started_ms) = self.thinking_segment_started_ms.take() {
            obj.insert(
                "started_at_ms".to_string(),
                serde_json::Value::from(started_ms),
            );
        }
        self.assistant_segment.push(serde_json::Value::Object(obj));
        self.thinking_buf.clear();
        self.thinking_segment_started_ms = None;
    }

    fn on_thinking(&mut self, delta: &str) {
        if self.thinking_buf.is_empty() && !delta.is_empty() {
            self.thinking_segment_started_ms = Some(Self::wallclock_ms_unix());
        }
        self.thinking_buf.push_str(delta);
    }

    fn on_tool_use(&mut self, name: &str, tool_use_id: &str, input: serde_json::Value) {
        self.absorb_pending_text_into_segment();
        let safe_input = crate::services::governance::credential_vault::redact_args_optional(&input);
        self.assistant_segment.push(json!({
            "type": "tool_use",
            "name": name,
            "id": tool_use_id,
            "input": safe_input,
        }));
    }

    fn on_file_edit(
        &mut self,
        path: &str,
        additions: i32,
        deletions: i32,
        diff: Option<&str>,
        edit_batch_id: Option<&str>,
    ) {
        self.absorb_pending_text_into_segment();
        let mut obj = serde_json::Map::new();
        obj.insert("type".to_string(), serde_json::json!("file_edit"));
        obj.insert("path".to_string(), serde_json::Value::String(path.to_string()));
        obj.insert("additions".to_string(), serde_json::Value::from(additions));
        obj.insert("deletions".to_string(), serde_json::Value::from(deletions));
        if let Some(d) = diff {
            obj.insert("diff".to_string(), serde_json::Value::String(d.to_string()));
        }
        if let Some(b) = edit_batch_id {
            obj.insert("edit_batch_id".to_string(), serde_json::Value::String(b.to_string()));
        }
        obj.insert(
            "timestamp_ms".to_string(),
            serde_json::Value::from(Self::wallclock_ms_unix()),
        );
        self.assistant_segment.push(serde_json::Value::Object(obj));
    }

    fn on_command_preview(&mut self, tool_name: &str, args: &serde_json::Value) {
        self.absorb_pending_text_into_segment();
        let safe = crate::services::governance::credential_vault::redact_args_optional(args);
        self.assistant_segment.push(json!({
            "type": "command_preview",
            "tool_name": tool_name,
            "input": safe,
            "timestamp_ms": Self::wallclock_ms_unix(),
        }));
    }

    fn on_subagent_chunk(
        &mut self,
        task_id: Option<&str>,
        agent_id: &str,
        kind: &str,
        delta: &str,
        parent_tool_use_id: Option<&str>,
    ) {
        self.absorb_pending_text_into_segment();
        let mut obj = serde_json::Map::new();
        obj.insert("type".to_string(), serde_json::json!("subagent_chunk"));
        obj.insert("agent_id".to_string(), serde_json::Value::String(agent_id.to_string()));
        obj.insert("kind".to_string(), serde_json::Value::String(kind.to_string()));
        obj.insert("delta".to_string(), serde_json::Value::String(delta.to_string()));
        if let Some(t) = task_id {
            obj.insert("task_id".to_string(), serde_json::Value::String(t.to_string()));
        }
        if let Some(p) = parent_tool_use_id {
            obj.insert("parent_tool_use_id".to_string(), serde_json::Value::String(p.to_string()));
        }
        obj.insert(
            "timestamp_ms".to_string(),
            serde_json::Value::from(Self::wallclock_ms_unix()),
        );
        self.assistant_segment.push(serde_json::Value::Object(obj));
    }

    fn on_worker_event(
        &mut self,
        kind: &str,
        worker_id: &str,
        parent_tool_use_id: Option<&str>,
        payload: serde_json::Value,
    ) {
        self.absorb_pending_text_into_segment();
        let mut obj = serde_json::Map::new();
        obj.insert("type".to_string(), serde_json::json!("worker_event"));
        obj.insert("kind".to_string(), serde_json::Value::String(kind.to_string()));
        obj.insert("worker_id".to_string(), serde_json::Value::String(worker_id.to_string()));
        if let Some(p) = parent_tool_use_id {
            obj.insert("parent_tool_use_id".to_string(), serde_json::Value::String(p.to_string()));
        }
        obj.insert("payload".to_string(), payload);
        obj.insert(
            "timestamp_ms".to_string(),
            serde_json::Value::from(Self::wallclock_ms_unix()),
        );
        self.assistant_segment.push(serde_json::Value::Object(obj));
    }

    fn on_plan_progress(
        &mut self,
        plan_path: &str,
        title: &str,
        todos: serde_json::Value,
        timestamp_ms: u64,
    ) {
        self.absorb_pending_text_into_segment();
        self.finalize_assistant_segment();
        let payload = vec![json!({
            "type": "plan_progress",
            "plan_path": plan_path,
            "title": title,
            "todos": todos,
            "timestamp_ms": timestamp_ms,
        })];
        if let Ok(s) = serde_json::to_string(&payload) {
            self.out.push(crate::providers::ChatMessage::assistant(s));
        }
    }

    fn on_tool_result(&mut self, tool_use_id: &str, output: String, is_error: bool) {
        self.absorb_pending_text_into_segment();
        self.finalize_assistant_segment();
        let safe_output = crate::services::governance::credential_vault::redact_for_audit_optional(&output);
        let payload = vec![json!({
            "type": "tool_result",
            "tool_use_id": tool_use_id,
            "content": safe_output,
            "is_error": is_error,
        })];
        if let Ok(s) = serde_json::to_string(&payload) {
            self.out.push(crate::providers::ChatMessage::tool(s));
        }
    }

    fn take_unflushed(&mut self) -> Vec<crate::providers::ChatMessage> {
        std::mem::take(&mut self.out)
    }

    fn finish(mut self) -> Vec<crate::providers::ChatMessage> {
        self.absorb_pending_text_into_segment();
        self.finalize_assistant_segment();
        self.out
    }
}

static TOOL_USE_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_tool_use_id() -> String {
    let n = TOOL_USE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("toolu_{n:x}")
}

async fn run_turn(
    state: &AppState,
    agent: &mut crate::agent::Agent,
    outbound: &OutboundSender,
    session_id: &str,
    session_key: &str,
    content: &str,
) {
    use crate::agent::TurnEvent;

    let workspace_key = crate::session::workspace_key_from_path(
        agent.current_workspace_dir(),
        session_id,
    );

    if state.session_run_state.is_running(session_id) {
        tracing::warn!(
            session_id,
            workspace_key = %workspace_key,
            "run_turn rejected: same session already running",
        );
        let _ = send_json(
            outbound,
            &serde_json::json!({
                "type": "workspace_busy",
                "workspaceKey": workspace_key,
                "currentSessionId": session_id,
            }),
        )
        .await;
        return;
    }

    let _run_guard = state.session_run_state.guard(session_id.to_string());

    let _ = send_json(
        outbound,
        &serde_json::json!({
            "type": "status",
            "state": "thinking",
        }),
    )
    .await;

    let (event_tx, mut event_rx) = mpsc::channel::<TurnEvent>(256);
    let content_owned = content.to_string();
    let mut text_block_open = false;
    let mut current_tool_use_id: Option<String> = None;
    let mut tool_use_id_for_name: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut accumulated_text = String::new();
    let started = std::time::Instant::now();

    let user_message_index: i64 = if let Some(ref backend) = state.session_backend {
        let backend_arc = std::sync::Arc::clone(backend);
        let session_key_owned = session_key.to_string();
        tokio::task::spawn_blocking(move || {
            let rows = backend_arc.load_with_tombstones(&session_key_owned);
            let total = rows.iter().filter(|m| m.message.role == "user").count();
            #[allow(clippy::cast_possible_wrap)]
            {
                (total.saturating_sub(1)) as i64
            }
        })
        .await
        .unwrap_or(0)
    } else {
        0
    };
    let mut recorded_batches: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    let sqlite_persist = std::sync::Arc::new(std::sync::Mutex::new(DesktopSqlitePersist::default()));
    let sqlite_persist_forward = std::sync::Arc::clone(&sqlite_persist);

    let coding_mode_label = agent
        .current_coding_mode()
        .map(|m| m.display_name().to_string())
        .or_else(|| {
            crate::services::try_get_services()
                .map(|svc| svc.coding_mode.read().display_name().to_string())
        });
    if let (Some(svc), Some(label)) = (
        crate::services::try_get_services(),
        coding_mode_label.as_ref(),
    ) {
        if let Some(parsed) = crate::agent::coding_mode::CodingMode::from_str_loose(label) {
            *svc.coding_mode.write() = parsed;
        }
    }

    let cost_tracking_ctx = state.cost_tracker.as_ref().map(|tracker| {
        let prices = {
            let cfg = state.config.lock();
            std::sync::Arc::new(cfg.cost.prices.clone())
        };
        let mut ctx =
            crate::agent::ToolLoopCostTrackingContext::new(std::sync::Arc::clone(tracker), prices)
                .with_chat_session_id(session_id.to_string());
        if let Some(ref mode) = coding_mode_label {
            ctx = ctx.with_coding_mode(mode.clone());
        }
        ctx
    });

    if let Some(engine) = crate::evolution::try_global() {
        if engine.enabled() {
            engine.flush_next_state(session_id, "user", &content_owned);
        }
    }

    let evolution_ctx = crate::evolution::try_global().and_then(|engine| {
        if !engine.enabled() {
            return None;
        }
        let mut ctx =
            crate::evolution::EvolutionCtx::new(engine, session_id.to_string())
                .with_turn_class(crate::evolution::TurnClass::Main);
        if let Some(ref mode) = coding_mode_label {
            ctx = ctx.with_coding_mode(mode.clone());
        }
        Some(ctx)
    });

    let session_title = if let Some(backend) = state.session_backend.clone() {
        let session_key_for_title = session_key.to_string();
        tokio::task::spawn_blocking(move || {
            backend
                .get_session_name(&session_key_for_title)
                .ok()
                .flatten()
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
        })
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| session_id.to_string())
    } else {
        session_id.to_string()
    };
    let session_ctx = crate::session::SessionContext {
        session_id: session_id.to_string(),
        workspace_key: workspace_key.clone(),
        title: session_title,
        workspace_dir: agent.current_workspace_dir().to_string_lossy().into_owned(),
    };
    let turn_fut = async {
        let inner = async {
            crate::agent::scope_tool_loop_cost_tracking(
                cost_tracking_ctx,
                agent.turn_streamed(&content_owned, event_tx),
            )
            .await
        };
        let scoped =
            crate::evolution::scope_evolution_ctx(evolution_ctx.clone(), inner);
        let result =
            crate::session::scope_session_context(session_ctx, scoped).await;
        if let Some(ref ctx) = evolution_ctx {
            let aborted = match &result {
                Ok(_) => None,
                Err(error) => Some(format!("{error}")),
            };
            let final_text = result.as_ref().ok().cloned();
            let _ = ctx.finalize_turn(final_text, aborted);
        }
        result
    };

    let forward_fut = async {
        while let Some(event) = event_rx.recv().await {
            match event {
                TurnEvent::Chunk { delta } => {
                    if let Ok(mut pg) = sqlite_persist_forward.lock() {
                        pg.on_chunk(&delta);
                    }
                    if !text_block_open {
                        let _ = send_json(
                            outbound,
                            &serde_json::json!({
                                "type": "content_start",
                                "blockType": "text",
                            }),
                        )
                        .await;
                        text_block_open = true;
                    }
                    const MAX_ACCUMULATED_TEXT_BYTES: usize = 2 * 1024 * 1024;
                    if accumulated_text.len() + delta.len() <= MAX_ACCUMULATED_TEXT_BYTES {
                        accumulated_text.push_str(&delta);
                    } else if accumulated_text.len() < MAX_ACCUMULATED_TEXT_BYTES {
                        let remaining = MAX_ACCUMULATED_TEXT_BYTES - accumulated_text.len();
                        let take_bytes = delta
                            .char_indices()
                            .take_while(|(idx, _)| *idx <= remaining)
                            .last()
                            .map(|(idx, ch)| idx + ch.len_utf8())
                            .unwrap_or(0);
                        if take_bytes > 0 {
                            accumulated_text.push_str(&delta[..take_bytes]);
                        }
                    }
                    let _ = send_json(
                        outbound,
                        &serde_json::json!({
                            "type": "content_delta",
                            "text": delta,
                        }),
                    )
                    .await;
                }
                TurnEvent::StreamReset => {
                    accumulated_text.clear();
                    if let Ok(mut pg) = sqlite_persist_forward.lock() {
                        pg.reset_stream();
                    }
                    let _ = send_json(
                        outbound,
                        &serde_json::json!({
                            "type": "content_reset",
                        }),
                    )
                    .await;
                }
                TurnEvent::Thinking { delta } => {
                    if let Ok(mut pg) = sqlite_persist_forward.lock() {
                        pg.on_thinking(&delta);
                    }
                    let _ = send_json(
                        outbound,
                        &serde_json::json!({
                            "type": "thinking",
                            "text": delta,
                        }),
                    )
                    .await;
                }
                TurnEvent::ToolCall {
                    name,
                    args,
                    tool_call_id,
                } => {
                    text_block_open = false;
                    let id = tool_call_id
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(next_tool_use_id);
                    current_tool_use_id = Some(id.clone());
                    tool_use_id_for_name.insert(name.clone(), id.clone());
                    let safe_args = crate::services::governance::credential_vault::redact_args_optional(&args);
                    if let Ok(mut pg) = sqlite_persist_forward.lock() {
                        pg.on_tool_use(&name, &id, safe_args.clone());
                    }
                    let _ = send_json(
                        outbound,
                        &serde_json::json!({
                            "type": "content_start",
                            "blockType": "tool_use",
                            "toolName": name,
                            "toolUseId": id,
                        }),
                    )
                    .await;
                    let _ = send_json(
                        outbound,
                        &serde_json::json!({
                            "type": "tool_use_complete",
                            "toolName": name,
                            "toolUseId": id,
                            "input": safe_args,
                            "sessionId": session_id,
                        }),
                    )
                    .await;
                }
                TurnEvent::ToolResult {
                    name,
                    output,
                    success,
                    tool_call_id,
                } => {
                    let id = tool_call_id
                        .filter(|s| !s.is_empty())
                        .or_else(|| tool_use_id_for_name.remove(&name))
                        .or_else(|| current_tool_use_id.clone())
                        .unwrap_or_else(next_tool_use_id);
                    current_tool_use_id = None;
                    let is_error = crate::agent::tool_handler::event_status::tool_result_is_error(
                        &name,
                        success,
                        &output,
                    );
                    let safe_output =
                        crate::services::governance::credential_vault::redact_for_audit_optional(&output);
                    let flush_rows = {
                        if let Ok(mut pg) = sqlite_persist_forward.lock() {
                            pg.on_tool_result(&id, safe_output.clone(), is_error);
                            pg.take_unflushed()
                        } else {
                            Vec::new()
                        }
                    };
                    enqueue_persist(state, session_key, flush_rows);
                    let _ = send_json(
                        outbound,
                        &serde_json::json!({
                            "type": "tool_result",
                            "toolUseId": id,
                            "content": safe_output,
                            "isError": is_error,
                        }),
                    )
                    .await;
                    if !is_error
                        && matches!(
                            name.as_str(),
                            "todo_write"
                                | "TodoWrite"
                                | "todowrite"
                                | "tasks_write"
                                | "TasksWrite"
                        )
                    {
                        let snapshot = if let Some(svc) = crate::services::try_get_services() {
                            crate::tools::todo_write::session_todos(&svc.todo_store, session_id)
                        } else {
                            Vec::new()
                        };
                        let _ = send_json(
                            outbound,
                            &serde_json::json!({
                                "type": "todo_snapshot",
                                "sessionId": session_id,
                                "todos": snapshot,
                            }),
                        )
                        .await;
                    }
                }
                TurnEvent::PlanProgressCommitted {
                    plan_path,
                    title,
                    todos_json,
                } => {
                    let todos = serde_json::from_str::<serde_json::Value>(&todos_json)
                        .unwrap_or(serde_json::Value::Null);
                    let timestamp_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    let flush_rows = {
                        if let Ok(mut pg) = sqlite_persist_forward.lock() {
                            pg.on_plan_progress(&plan_path, &title, todos.clone(), timestamp_ms);
                            pg.take_unflushed()
                        } else {
                            Vec::new()
                        }
                    };
                    enqueue_persist(state, session_key, flush_rows);
                    let _ = send_json(
                        outbound,
                        &serde_json::json!({
                            "type": "plan_progress",
                            "planPath": plan_path,
                            "title": title,
                            "todos": todos,
                            "timestampMs": timestamp_ms,
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

                    if let Some(ref batch) = edit_batch_id {
                        if !batch.is_empty() && recorded_batches.insert(batch.clone()) {
                            if let Some(ref backend) = state.session_backend {
                                let backend_arc = std::sync::Arc::clone(backend);
                                let session_key_owned = session_key.to_string();
                                let batch_owned = batch.clone();
                                let umi = user_message_index;
                                let outcome = tokio::task::spawn_blocking(move || {
                                    backend_arc.record_edit_batch(
                                        &session_key_owned,
                                        umi,
                                        &batch_owned,
                                    )
                                })
                                .await;
                                if let Ok(Err(e)) = outcome {
                                    tracing::warn!(
                                        target: "rewind",
                                        "record_edit_batch failed: session={} idx={} batch={} err={}",
                                        session_key, user_message_index, batch, e
                                    );
                                }
                            }
                        }
                    }

                    if let Ok(mut pg) = sqlite_persist_forward.lock() {
                        pg.on_file_edit(
                            &path,
                            additions,
                            deletions,
                            diff.as_deref(),
                            edit_batch_id.as_deref(),
                        );
                    }

                    let _ = send_json(
                        outbound,
                        &serde_json::json!({
                            "type": "system_notification",
                            "subtype": "file_edit",
                            "data": {
                                "path": path,
                                "additions": additions,
                                "deletions": deletions,
                                "diff": diff,
                                "editBatchId": edit_batch_id,
                            }
                        }),
                    )
                    .await;
                }
                TurnEvent::StatusUpdate { action, detail } => {
                    let _ = send_json(
                        outbound,
                        &serde_json::json!({
                            "type": "status",
                            "state": "tool_executing",
                            "verb": action,
                            "tokens": null,
                        }),
                    )
                    .await;
                    if !detail.is_empty() {
                        let _ = send_json(
                            outbound,
                            &serde_json::json!({
                                "type": "system_notification",
                                "subtype": "status_detail",
                                "message": detail,
                            }),
                        )
                        .await;
                    }
                }
                TurnEvent::ProgressTick {
                    iteration,
                    max_iterations: _,
                    tokens_used,
                } => {
                    let _ = send_json(
                        outbound,
                        &serde_json::json!({
                            "type": "status",
                            "state": "thinking",
                            "verb": format!("iter {iteration}"),
                            "tokens": tokens_used,
                            "elapsed": started.elapsed().as_secs(),
                        }),
                    )
                    .await;
                }
                TurnEvent::CommandPreview { tool_name, args, estimated_duration_ms: _ } => {
                    if let Ok(mut pg) = sqlite_persist_forward.lock() {
                        pg.on_command_preview(&tool_name, &args);
                    }
                    let _ = send_json(
                        outbound,
                        &serde_json::json!({
                            "type": "system_notification",
                            "subtype": "command_preview",
                            "data": {
                                "toolName": tool_name,
                                "input": args,
                            }
                        }),
                    )
                    .await;
                }
                TurnEvent::Cancelling { reason } => {
                    let _ = send_json(
                        outbound,
                        &serde_json::json!({
                            "type": "status",
                            "state": "idle",
                            "verb": "cancelling",
                        }),
                    )
                    .await;
                    let _ = send_json(
                        outbound,
                        &serde_json::json!({
                            "type": "system_notification",
                            "subtype": "cancelling",
                            "message": reason,
                        }),
                    )
                    .await;
                }
                TurnEvent::ContextCompressed {
                    tokens_before: _,
                    tokens_after: _,
                } => {

                }
                TurnEvent::PermissionRequest {
                    request_id,
                    tool_name,
                    input,
                    description,
                } => {

                    let tool_use_id = tool_use_id_for_name
                        .get(&tool_name)
                        .cloned()
                        .or_else(|| current_tool_use_id.clone());
                    let mut frame = serde_json::json!({
                        "type": "permission_request",
                        "requestId": request_id,
                        "toolName": tool_name,
                        "input": input,
                    });
                    if let Some(id) = tool_use_id {
                        frame["toolUseId"] = serde_json::Value::String(id);
                    }
                    if let Some(desc) = description {
                        frame["description"] = serde_json::Value::String(desc);
                    }
                    let _ = send_json(outbound, &frame).await;
                }
                TurnEvent::SubagentChunk {
                    task_id,
                    agent_id,
                    kind,
                    delta,
                } => {
                    let kind_str = format!("{kind:?}");
                    let parent_id = current_tool_use_id.clone();
                    if let Ok(mut pg) = sqlite_persist_forward.lock() {
                        let task_opt = if task_id.is_empty() { None } else { Some(task_id.as_str()) };
                        pg.on_subagent_chunk(
                            task_opt,
                            &agent_id,
                            &kind_str,
                            &delta,
                            parent_id.as_deref(),
                        );
                    }
                    let mut data = serde_json::json!({
                        "taskId": task_id,
                        "agentId": agent_id,
                        "kind": kind_str,
                        "delta": delta,
                    });
                    if let Some(ref pid) = parent_id
                        && let serde_json::Value::Object(obj) = &mut data
                    {
                        obj.insert(
                            "parentToolUseId".to_string(),
                            serde_json::Value::String(pid.clone()),
                        );
                    }
                    let _ = send_json(
                        outbound,
                        &serde_json::json!({
                            "type": "system_notification",
                            "subtype": "subagent_chunk",
                            "data": data,
                        }),
                    )
                    .await;
                }
                TurnEvent::Error { message } => {
                    send_error(outbound, &message, "TURN_ERROR").await;
                }
                TurnEvent::PiiSanitized { report } => {
                    let mut counts = serde_json::Map::new();
                    let mut total: u64 = 0;
                    for (kind, count) in report.counts.iter() {
                        counts.insert(
                            kind.label().to_string(),
                            serde_json::Value::from(*count as u64),
                        );
                        total += *count as u64;
                    }
                    if total == 0 {
                        continue;
                    }
                    let _ = send_json(
                        outbound,
                        &serde_json::json!({
                            "type": "system_notification",
                            "subtype": "debug_pii_stats",
                            "data": {
                                "total": total,
                                "counts": serde_json::Value::Object(counts),
                            }
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
                    if let Ok(mut pg) = sqlite_persist_forward.lock() {
                        pg.reset_stream();
                        pg.discard_pending_thinking();
                    }
                    let _ = send_json(
                        outbound,
                        &serde_json::json!({
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
                    let parent_id = if parent_tool_use_id.is_empty() {
                        current_tool_use_id.clone().unwrap_or_default()
                    } else {
                        parent_tool_use_id
                    };
                    if let Ok(mut pg) = sqlite_persist_forward.lock() {
                        let parent_opt = if parent_id.is_empty() { None } else { Some(parent_id.as_str()) };
                        pg.on_worker_event(
                            "spawned",
                            &worker_id,
                            parent_opt,
                            serde_json::json!({ "title": title, "model": model }),
                        );
                    }
                    let _ = send_json(
                        outbound,
                        &serde_json::json!({
                            "type": "worker_spawned",
                            "sessionId": session_id,
                            "parentToolUseId": parent_id,
                            "workerId": worker_id,
                            "title": title,
                            "model": model,
                        }),
                    )
                    .await;
                }
                TurnEvent::WorkerStatus { worker_id, status, detail } => {
                    if let Ok(mut pg) = sqlite_persist_forward.lock() {
                        pg.on_worker_event(
                            "status",
                            &worker_id,
                            current_tool_use_id.as_deref(),
                            serde_json::json!({ "status": status, "detail": detail }),
                        );
                    }
                    let _ = send_json(
                        outbound,
                        &serde_json::json!({
                            "type": "worker_status",
                            "sessionId": session_id,
                            "workerId": worker_id,
                            "status": status,
                            "detail": detail,
                        }),
                    )
                    .await;
                }
                TurnEvent::WorkerProgress { worker_id, action, detail } => {
                    if let Ok(mut pg) = sqlite_persist_forward.lock() {
                        pg.on_worker_event(
                            "progress",
                            &worker_id,
                            current_tool_use_id.as_deref(),
                            serde_json::json!({ "action": action, "detail": detail }),
                        );
                    }
                    let _ = send_json(
                        outbound,
                        &serde_json::json!({
                            "type": "worker_progress",
                            "sessionId": session_id,
                            "workerId": worker_id,
                            "action": action,
                            "detail": detail,
                        }),
                    )
                    .await;
                }
                TurnEvent::WorkerCompleted { worker_id, success, summary } => {
                    if let Ok(mut pg) = sqlite_persist_forward.lock() {
                        pg.on_worker_event(
                            "completed",
                            &worker_id,
                            current_tool_use_id.as_deref(),
                            serde_json::json!({ "success": success, "summary": summary }),
                        );
                    }
                    let _ = send_json(
                        outbound,
                        &serde_json::json!({
                            "type": "worker_completed",
                            "sessionId": session_id,
                            "workerId": worker_id,
                            "success": success,
                            "summary": summary,
                        }),
                    )
                    .await;
                }
                TurnEvent::WorkerStopped { worker_id, reason } => {
                    if let Ok(mut pg) = sqlite_persist_forward.lock() {
                        pg.on_worker_event(
                            "stopped",
                            &worker_id,
                            current_tool_use_id.as_deref(),
                            serde_json::json!({ "reason": reason }),
                        );
                    }
                    let _ = send_json(
                        outbound,
                        &serde_json::json!({
                            "type": "worker_stopped",
                            "sessionId": session_id,
                            "workerId": worker_id,
                            "reason": reason,
                        }),
                    )
                    .await;
                }
                TurnEvent::ParentResumed { reason } => {
                    let _ = send_json(
                        outbound,
                        &serde_json::json!({
                            "type": "parent_resumed",
                            "sessionId": session_id,
                            "reason": reason,
                        }),
                    )
                    .await;
                }
            }
        }
    };

    let (turn_caught, forward_caught) = tokio::join!(
        std::panic::AssertUnwindSafe(turn_fut).catch_unwind(),
        std::panic::AssertUnwindSafe(forward_fut).catch_unwind(),
    );

    let forward_panicked = forward_caught.is_err();
    if let Err(panic) = &forward_caught {
        tracing::error!(
            target: "ws_desktop_turn",
            session_id,
            "event forwarding panicked (recovered): {}",
            describe_panic(&**panic),
        );
    }

    let turn_result: Result<String, String> = match turn_caught {
        Ok(Ok(final_text)) if !forward_panicked => Ok(final_text),
        Ok(Ok(_)) => Err(
            "turn completed but event forwarding was interrupted by an internal error".to_string(),
        ),
        Ok(Err(err)) => Err(format!("{err}")),
        Err(panic) => {
            let detail = describe_panic(&*panic);
            tracing::error!(
                target: "ws_desktop_turn",
                session_id,
                "turn execution panicked (recovered): {detail}",
            );
            Err(format!("internal error recovered: {detail}"))
        }
    };

    match turn_result {
        Ok(final_text) => {
            if state.session_backend.is_some() {
                let recorder = sqlite_persist
                    .lock()
                    .ok()
                    .map(|mut g| std::mem::take(&mut *g))
                    .unwrap_or_default();
                let mut rows = recorder.finish();
                if rows.is_empty() && !final_text.trim().is_empty() {
                    rows.push(crate::providers::ChatMessage::assistant(final_text.clone()));
                }
                enqueue_persist(state, session_key, rows);
            }
            let final_todos = if let Some(svc) = crate::services::try_get_services() {
                crate::tools::todo_write::session_todos(&svc.todo_store, session_id)
            } else {
                Vec::new()
            };
            let _ = send_json(
                outbound,
                &serde_json::json!({
                    "type": "todo_snapshot",
                    "sessionId": session_id,
                    "todos": final_todos,
                }),
            )
            .await;
            let usage = agent.last_usage();
            let _ = send_json(
                outbound,
                &serde_json::json!({
                    "type": "message_complete",
                    "usage": {
                        "input_tokens": usage.and_then(|u| u.input_tokens).unwrap_or(0),
                        "output_tokens": usage.and_then(|u| u.output_tokens).unwrap_or(0),
                        "cache_read_tokens": usage.and_then(|u| u.cached_input_tokens).unwrap_or(0),
                        "cache_creation_tokens": usage.and_then(|u| u.cache_creation_input_tokens).unwrap_or(0),
                    },
                }),
            )
            .await;
        }
        Err(err) => {
            if state.session_backend.is_some() {
                let recorder = sqlite_persist
                    .lock()
                    .ok()
                    .map(|mut g| std::mem::take(&mut *g))
                    .unwrap_or_default();
                let rows = recorder.finish();
                enqueue_persist(state, session_key, rows);
            }
            let msg = format!("{err}");
            let code = crate::agent::error_classify::classify_turn_error_code(&msg);
            send_error(outbound, &msg, code).await;
            let final_todos = if let Some(svc) = crate::services::try_get_services() {
                crate::tools::todo_write::session_todos(&svc.todo_store, session_id)
            } else {
                Vec::new()
            };
            let _ = send_json(
                outbound,
                &serde_json::json!({
                    "type": "todo_snapshot",
                    "sessionId": session_id,
                    "todos": final_todos,
                }),
            )
            .await;
        }
    }

    let _ = send_json(
        outbound,
        &serde_json::json!({
            "type": "status",
            "state": "idle",
        }),
    )
    .await;

    if let Some(backend) = state.session_backend.clone() {
        let session_key_get = session_key.to_string();
        let backend_get = backend.clone();
        let existing = tokio::task::spawn_blocking(move || {
            backend_get.get_session_name(&session_key_get).ok().flatten()
        })
        .await
        .ok()
        .flatten();
        let needs_auto_title = existing
            .as_deref()
            .map(|name| name.trim().is_empty() || is_legacy_default_title(name))
            .unwrap_or(true);
        if needs_auto_title {
            let summary = first_line(content).chars().take(60).collect::<String>();
            if !summary.is_empty() {
                let session_key_set = session_key.to_string();
                let backend_set = backend.clone();
                let summary_for_set = summary.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    backend_set.set_session_name(&session_key_set, &summary_for_set)
                })
                .await;
                let _ = send_json(
                    outbound,
                    &serde_json::json!({
                        "type": "session_title_updated",
                        "sessionId": session_id,
                        "title": summary,
                    }),
                )
                .await;
            }
        }
    }
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("").trim()
}

fn resource_event_to_system_notification(
    event: &crate::session::ResourceEvent,
    current_session_id: &str,
) -> Option<serde_json::Value> {
    use crate::session::ResourceEvent;
    match event {
        ResourceEvent::WaitStarted {
            session_id,
            kind,
            target,
            holder_session_id,
            holder_title,
        } => {
            if session_id != current_session_id {
                return None;
            }
            Some(serde_json::json!({
                "type": "system_notification",
                "subtype": "resource_wait_started",
                "sessionId": session_id,
                "data": {
                    "kind": kind,
                    "target": target,
                    "holderSessionId": holder_session_id,
                    "holderTitle": holder_title,
                },
            }))
        }
        ResourceEvent::WaitResolved {
            session_id,
            kind,
            target,
        } => {
            if session_id != current_session_id {
                return None;
            }
            Some(serde_json::json!({
                "type": "system_notification",
                "subtype": "resource_wait_resolved",
                "sessionId": session_id,
                "data": {
                    "kind": kind,
                    "target": target,
                },
            }))
        }
    }
}

fn mode_transition_auto_approved(
    whitelist: &[String],
    from: crate::agent::coding_mode::CodingMode,
    to: crate::agent::coding_mode::CodingMode,
) -> bool {
    crate::agent::mode::transition::is_auto_approved(whitelist, from, to)
}

fn is_legacy_default_title(name: &str) -> bool {
    let trimmed = name.trim();
    if matches!(
        trimmed,
        "Untitled session" | "New Session" | "新对话" | "New conversation"
    ) {
        return true;
    }

    if let Some(rest) = trimmed.strip_prefix("Session ") {
        let bytes = rest.as_bytes();
        if bytes.len() == 5
            && bytes[0].is_ascii_digit()
            && bytes[1].is_ascii_digit()
            && bytes[2] == b':'
            && bytes[3].is_ascii_digit()
            && bytes[4].is_ascii_digit()
        {
            return true;
        }
    }
    false
}
