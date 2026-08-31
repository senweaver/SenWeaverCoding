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
const DESKTOP_INBOUND_CAPACITY: usize = 4096;

const DEFAULT_MAX_DESKTOP_CONNECTIONS: usize = 64;

static DESKTOP_CONNECTION_COUNT: AtomicUsize = AtomicUsize::new(0);

fn max_desktop_connections() -> usize {
    crate::util::get_runtime_var("SEN_MAX_DESKTOP_CONNECTIONS")
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_MAX_DESKTOP_CONNECTIONS)
}

struct DesktopConnectionGuard;

impl DesktopConnectionGuard {
    fn try_acquire() -> Option<Self> {
        let cap = max_desktop_connections();
        let prev = DESKTOP_CONNECTION_COUNT.fetch_add(1, Ordering::AcqRel);
        if prev >= cap {
            DESKTOP_CONNECTION_COUNT.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        Some(Self)
    }
}

impl Drop for DesktopConnectionGuard {
    fn drop(&mut self) {
        DESKTOP_CONNECTION_COUNT.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Debug)]
enum OutboundFrame {
    Text(String),
    Pong(Vec<u8>),

    ContentDelta(String, u64),

    Thinking(String, u64),
}

fn with_frame_seq(frame: &str, out_seq: &mut u64) -> String {
    let trimmed = frame.trim_start();
    let Some(rest) = trimmed.strip_prefix('{') else {
        return frame.to_string();
    };
    *out_seq += 1;
    let seq = *out_seq;
    if rest.trim_start().starts_with('}') {
        return format!("{{\"seq\":{seq}}}");
    }
    format!("{{\"seq\":{seq},{rest}")
}

type OutboundSender = tokio::sync::mpsc::Sender<OutboundFrame>;

pub async fn handle_ws_desktop(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if let Err(reject) = super::authorize_ws_request(
        &state,
        &headers,
        Some(peer),
        None,
        "/ws/{session_id}",
    ) {
        return reject;
    }

    if !super::is_valid_session_id(&session_id) {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "Bad Request  - malformed session_id",
        )
            .into_response();
    }

    let Some(conn_guard) = DesktopConnectionGuard::try_acquire() else {
        tracing::warn!(
            target: "ws_desktop",
            cap = max_desktop_connections(),
            "desktop connection cap reached; rejecting new /ws connection"
        );
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "Too many active sessions  - close a session tab or raise SEN_MAX_DESKTOP_CONNECTIONS",
        )
            .into_response();
    };

    let ws = super::with_websocket_auth_protocol(ws, &headers);
    ws.on_upgrade(move |socket| handle_socket(socket, state, session_id, conn_guard))
        .into_response()
}

async fn handle_socket(
    socket: WebSocket,
    state: AppState,
    session_id: String,
    _conn_guard: DesktopConnectionGuard,
) {
    abort_disconnect_grace(&session_id);
    register_session_connection(&session_id);
    let (mut sink, mut receiver) = socket.split();

    let (outbound_tx, mut outbound_rx) =
        tokio::sync::mpsc::channel::<OutboundFrame>(1024);
    let sender_seq = register_session_sender(&session_id, &outbound_tx);

    let (control_tx, mut control_rx) =
        tokio::sync::mpsc::channel::<OutboundFrame>(64);

    let conn_token = tokio_util::sync::CancellationToken::new();
    let turn_abort_token = tokio_util::sync::CancellationToken::new();
    let writer_conn_token = conn_token.clone();

    let writer_handle = crate::runtime::spawn_supervised("ws_desktop.writer", async move {
        const COALESCE_WINDOW_MS: u64 = 24;
        const COALESCE_MAX_FRAMES: usize = 64;
        const WRITER_SEND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
        macro_rules! send_frame {
            ($msg:expr) => {{
                match tokio::time::timeout(WRITER_SEND_TIMEOUT, sink.send($msg)).await {
                    Ok(Ok(())) => false,
                    _ => {
                        writer_conn_token.cancel();
                        true
                    }
                }
            }};
        }
        let mut delta_buf = String::new();
        let mut thinking_buf = String::new();
        let mut delta_turn_seq: u64 = 0;
        let mut thinking_turn_seq: u64 = 0;
        let mut out_seq: u64 = 0;
        loop {
            let frame = tokio::select! {
                biased;
                Some(ctrl) = control_rx.recv() => {
                    let control_msg = match ctrl {
                        OutboundFrame::Text(s) => {
                            Message::Text(with_frame_seq(&s, &mut out_seq).into())
                        }
                        OutboundFrame::Pong(p) => Message::Pong(p.into()),
                        OutboundFrame::ContentDelta(t, _) | OutboundFrame::Thinking(t, _) => {
                            Message::Text(with_frame_seq(&t, &mut out_seq).into())
                        }
                    };
                    if send_frame!(control_msg) {
                        break;
                    }
                    continue;
                }
                main = outbound_rx.recv() => match main {
                    Some(f) => f,
                    None => break,
                },
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
            thinking_buf.clear();
            let mut send_failed = false;

            macro_rules! flush_buf {
                ($kind:expr, $buf:expr, $turn_seq:expr) => {{
                    if !$buf.is_empty() {
                        out_seq += 1;
                        let coalesced = if $turn_seq > 0 {
                            serde_json::json!({
                                "seq": out_seq,
                                "type": $kind,
                                "text": $buf.clone(),
                                "turnSeq": $turn_seq,
                            })
                        } else {
                            serde_json::json!({
                                "seq": out_seq,
                                "type": $kind,
                                "text": $buf.clone(),
                            })
                        }
                        .to_string();
                        $buf.clear();
                        if send_frame!(Message::Text(coalesced.into())) {
                            send_failed = true;
                        }
                    }
                }};
            }

            for f in frames.drain(..) {
                match f {
                    OutboundFrame::ContentDelta(text, tseq) => {
                        flush_buf!("thinking", thinking_buf, thinking_turn_seq);
                        if send_failed {
                            break;
                        }
                        if !delta_buf.is_empty() && delta_turn_seq != tseq {
                            flush_buf!("content_delta", delta_buf, delta_turn_seq);
                            if send_failed {
                                break;
                            }
                        }
                        delta_turn_seq = tseq;
                        delta_buf.push_str(&text);
                    }
                    OutboundFrame::Thinking(text, tseq) => {
                        flush_buf!("content_delta", delta_buf, delta_turn_seq);
                        if send_failed {
                            break;
                        }
                        if !thinking_buf.is_empty() && thinking_turn_seq != tseq {
                            flush_buf!("thinking", thinking_buf, thinking_turn_seq);
                            if send_failed {
                                break;
                            }
                        }
                        thinking_turn_seq = tseq;
                        thinking_buf.push_str(&text);
                    }
                    OutboundFrame::Text(s) => {
                        flush_buf!("content_delta", delta_buf, delta_turn_seq);
                        if send_failed {
                            break;
                        }
                        flush_buf!("thinking", thinking_buf, thinking_turn_seq);
                        if send_failed {
                            break;
                        }
                        if send_frame!(Message::Text(with_frame_seq(&s, &mut out_seq).into())) {
                            send_failed = true;
                            break;
                        }
                    }
                    OutboundFrame::Pong(p) => {
                        flush_buf!("content_delta", delta_buf, delta_turn_seq);
                        if send_failed {
                            break;
                        }
                        flush_buf!("thinking", thinking_buf, thinking_turn_seq);
                        if send_failed {
                            break;
                        }
                        if send_frame!(Message::Pong(p.into())) {
                            send_failed = true;
                            break;
                        }
                    }
                }
            }
            if !send_failed {
                flush_buf!("content_delta", delta_buf, delta_turn_seq);
            }
            if !send_failed {
                flush_buf!("thinking", thinking_buf, thinking_turn_seq);
            }
            if send_failed {
                break;
            }
        }
        let _ = sink.close().await;
    })
    .into_inner();

    let session_key = format!("{GW_SESSION_PREFIX}{session_id}");
    let connection_id = uuid::Uuid::new_v4().to_string();

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

    let (config, config_sanitized) = {
        let mut cfg = state.config.lock();
        let changed = super::super::desktop::routes::sanitize_active_profile_in_place(&mut cfg);
        if changed {
            tracing::info!(
                "ws_desktop: sanitized stale default_provider/default_model in persisted config"
            );
        }
        (cfg.clone(), changed)
    };
    if config_sanitized {
        if let Err(e) = crate::gateway::persist_config(&config).await {
            tracing::warn!(
                target: "ws_desktop_persist",
                error = %e,
                "failed to persist sanitized config on ws connect"
            );
        }
    }
    state.push_live_config(config.clone());
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
            unregister_session_sender(&session_id, sender_seq);
            drop(outbound_tx);
            let _ = writer_handle.await;
            let _ = unregister_session_connection(&session_id);
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
                crate::security::sandbox::register_workspace_root_for_session(
                    &session_id,
                    std::path::Path::new(trimmed),
                );
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
        const SEED_HISTORY_WINDOW: usize = 400;
        let backend_arc = std::sync::Arc::clone(backend);
        let session_key_owned = session_key.clone();
        let (messages, persisted_user_msgs) = match tokio::task::spawn_blocking(move || {
            let messages = backend_arc.load_tail(&session_key_owned, SEED_HISTORY_WINDOW);
            let count = backend_arc.count_user_messages(&session_key_owned) as u64;
            (messages, count)
        })
        .await
        {
            Ok(loaded) => loaded,
            Err(e) => {
                tracing::warn!(
                    target: "ws_desktop_persist",
                    error = %e,
                    "session history load task panicked; starting with empty history"
                );
                (Vec::new(), 0)
            }
        };
        if !messages.is_empty() {
            agent.seed_history(&messages);
        }
        agent.set_gateway_sync_marker(persisted_user_msgs);
    }

    if let Some(svc) = crate::services::try_get_services() {
        if svc.is_global_auto_coding_mode()
            && svc.session_coding_mode(&session_key).is_none()
            && !svc.is_session_auto_coding_mode(&session_key)
        {
            svc.set_session_auto_coding_mode(&session_key, true);
        }
        let resolved_mode = svc.resolve_coding_mode_for(Some(&session_key));
        set_coding_mode_scoped(&mut agent, &session_id, &connection_id, resolved_mode).await;
        let derived =
            super::super::desktop::routes::derive_permission_from_coding(&resolved_mode);
        let is_auto = svc.is_session_auto_coding_mode(&session_key);
        let candidate = if is_auto { "default" } else { derived };
        let permission_mode = desktop_runtime_state()
            .ensure_session_permission_mode(&session_key, candidate);
        let _ = send_json(
            &outbound_tx,
            &serde_json::json!({
                "type": "system_notification",
                "subtype": "coding_mode_updated",
                "message": if is_auto {
                    "Coding mode: Auto (intent-routed)".to_string()
                } else {
                    format!("Coding mode: {}", resolved_mode.label())
                },
                "sessionId": session_id,
                "data": {
                    "mode": if is_auto { "auto" } else { resolved_mode.display_name() },
                    "label": if is_auto { "Auto" } else { resolved_mode.label() },
                    "permissionMode": permission_mode,
                    "sessionId": session_id,
                    "scope": "session",
                    "auto": is_auto,
                },
            }),
        )
        .await;
    }

    let (inbound_tx, mut inbound_rx) =
        tokio::sync::mpsc::channel::<serde_json::Value>(DESKTOP_INBOUND_CAPACITY);

    let inbound_tx_lsp = inbound_tx.clone();
    let inbound_tx_replay = inbound_tx.clone();
    let inbound_tx_gateway = inbound_tx.clone();
    let inbound_tx_resource = inbound_tx.clone();

    let last_activity =
        std::sync::Arc::new(std::sync::atomic::AtomicU64::new(desktop_now_unix_secs()));

    if state.session_run_state.is_running(&session_id) {
        if let Some(feed) = crate::session::get_turn_feed(&session_id) {
            let (history, mut rx) = feed.subscribe_with_history();
            let attach_outbound = outbound_tx.clone();
            let attach_conn_token = conn_token.clone();
            let _ = send_json(
                &outbound_tx,
                &serde_json::json!({ "type": "status", "state": "thinking" }),
            )
            .await;
            for frame in crate::approval::pending_replays_for_session(&session_id) {
                let _ = send_json(&outbound_tx, &frame).await;
            }
            let attach_feed = std::sync::Arc::clone(&feed);
            crate::runtime::spawn_supervised("ws_desktop.turn_attach", async move {
                let mut last_replayed: Option<u64> = None;
                for (index, frame) in history {
                    if attach_outbound
                        .send(OutboundFrame::Text(frame.to_string()))
                        .await
                        .is_err()
                    {
                        return;
                    }
                    last_replayed = Some(index);
                }
                loop {
                    tokio::select! {
                        biased;
                        _ = attach_conn_token.cancelled() => break,
                        recv = rx.recv() => match recv {
                            Ok((index, frame)) => {
                                if last_replayed.is_some_and(|last| index <= last) {
                                    continue;
                                }
                                if attach_outbound
                                    .send(OutboundFrame::Text(frame.to_string()))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                                last_replayed = Some(index);
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                                let recovered = attach_feed.frames_after(last_replayed);
                                let lost = match (last_replayed, recovered.first()) {
                                    (Some(last), Some((first_idx, _))) => *first_idx > last + 1,
                                    (None, Some((first_idx, _))) => *first_idx > 0,
                                    (_, None) => skipped > 0,
                                };
                                let mut send_failed = false;
                                for (index, frame) in recovered {
                                    if attach_outbound
                                        .send(OutboundFrame::Text(frame.to_string()))
                                        .await
                                        .is_err()
                                    {
                                        send_failed = true;
                                        break;
                                    }
                                    last_replayed = Some(index);
                                }
                                if send_failed {
                                    break;
                                }
                                if lost {
                                    let notice = serde_json::json!({
                                        "type": "system_notification",
                                        "subtype": "stream_lagged",
                                        "level": "warning",
                                        "message": format!(
                                            "Live stream skipped {skipped} frame(s); the full text will be in the saved history when this turn finishes."
                                        ),
                                        "data": { "skippedFrames": skipped },
                                    })
                                    .to_string();
                                    if attach_outbound
                                        .send(OutboundFrame::Text(notice))
                                        .await
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                                continue;
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        },
                    }
                }
            });
        }
    }

    let cancel_signal_handle = agent.cancel_signal_handle();
    let cancelled_atomic = agent.cancel_token();

    {
        let abort_token = turn_abort_token.clone();
        let atom = std::sync::Arc::clone(&cancelled_atomic);
        let sig = std::sync::Arc::clone(&cancel_signal_handle);
        crate::runtime::spawn_supervised("ws_desktop.turn_abort_bridge", async move {
            abort_token.cancelled().await;
            atom.store(true, std::sync::atomic::Ordering::SeqCst);
            sig.load_full().cancel();
        });
    }
    let cancel_signal_for_reader = std::sync::Arc::clone(&cancel_signal_handle);
    let cancelled_atomic_for_reader = std::sync::Arc::clone(&cancelled_atomic);
    let control_tx_reader = control_tx.clone();
    let session_id_for_reader = session_id.clone();
    let connection_id_for_reader = connection_id.clone();
    let live_config_for_reader = state.live_config.clone();
    let conn_token_reader = conn_token.clone();
    let last_activity_reader = std::sync::Arc::clone(&last_activity);

    let reader_handle = crate::runtime::spawn_supervised("ws_desktop.reader", async move {
        while let Some(frame) = receiver.next().await {
            last_activity_reader.store(
                desktop_now_unix_secs(),
                std::sync::atomic::Ordering::Relaxed,
            );
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
                            match inbound_tx.try_send(v) {
                                Ok(()) => {}
                                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                                    tracing::warn!(
                                        target: "ws_desktop",
                                        "inbound channel full; dropping invalid-json frame to keep reader responsive"
                                    );
                                }
                                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => break,
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
                        let _ = control_tx_reader
                            .try_send(OutboundFrame::Text(r#"{"type":"pong"}"#.to_string()));
                        let buddy_cfg = live_config_for_reader.load().buddy.clone();
                        if let Some((event, greeting)) =
                            crate::buddy::idle_transition_event(&buddy_cfg)
                        {
                            let frame = serde_json::json!({
                                "type": "buddy_event",
                                "sessionId": session_id_for_reader,
                                "event": event,
                                "greeting": greeting,
                                "showNotifications": buddy_cfg.show_notifications,
                            });
                            let _ = control_tx_reader
                                .try_send(OutboundFrame::Text(frame.to_string()));
                        }
                        continue;
                    }
                    if msg_type.as_str() == "stop_generation" {
                        cancelled_atomic_for_reader
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                        cancel_signal_for_reader.load_full().cancel();
                        if let Some(feed) =
                            crate::session::get_turn_feed(&session_id_for_reader)
                        {
                            feed.request_cancel();
                        }
                        tracing::info!(
                            target: "agent_cancel",
                            "stop_generation received: cancel signal fired (reader-side)"
                        );

                        crate::tools::background::registry::kill_foreground(
                            session_id_for_reader.as_str(),
                            Some(connection_id_for_reader.as_str()),
                        );

                        let cascade_to_workers = parsed
                            .get("cascade")
                            .or_else(|| parsed.get("stopWorkers"))
                            .or_else(|| parsed.get("stop_workers"))
                            .and_then(|v| v.as_bool())
                            .unwrap_or(true);
                        if cascade_to_workers {
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
                    }
                    if msg_type.as_str() == "set_permission_mode" {
                        let raw_mode = parsed
                            .get("mode")
                            .and_then(|v| v.as_str())
                            .unwrap_or("default");
                        let Some(mode) =
                            crate::config::normalize_desktop_permission_mode(raw_mode)
                        else {
                            let err = serde_json::json!({
                                "type": "error",
                                "code": "INVALID_PERMISSION_MODE",
                                "message": format!(
                                    "unknown permission mode: {raw_mode}; expected one of: default, acceptEdits, plan, bypassPermissions, dontAsk, askEveryTime"
                                ),
                            });
                            let _ = control_tx_reader
                                .try_send(OutboundFrame::Text(err.to_string()));
                            continue;
                        };
                        let session_key = format!("{GW_SESSION_PREFIX}{session_id_for_reader}");
                        desktop_runtime_state().set_session_permission_mode(&session_key, mode);
                        let note = serde_json::json!({
                            "type": "system_notification",
                            "subtype": "permission_mode_updated",
                            "message": format!("Permission mode: {mode}"),
                            "data": {
                                "mode": mode,
                                "scope": "session",
                                "sessionId": session_id_for_reader,
                            },
                        });
                        let _ = control_tx_reader
                            .try_send(OutboundFrame::Text(note.to_string()));
                        continue;
                    }
                    if msg_type.as_str() == "set_debug_submode" {
                        let submode_id = parsed
                            .get("submode")
                            .or_else(|| parsed.get("subMode"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("auto")
                            .to_string();
                        let Some(submode) =
                            crate::agent::debug::DebugSubMode::from_id(&submode_id)
                        else {
                            let err = serde_json::json!({
                                "type": "error",
                                "code": "UNKNOWN_DEBUG_SUBMODE",
                                "message": format!("unknown debug submode: {submode_id}"),
                            });
                            let _ = control_tx_reader
                                .try_send(OutboundFrame::Text(err.to_string()));
                            continue;
                        };
                        let params = parsed
                            .get("params")
                            .cloned()
                            .unwrap_or_else(|| serde_json::json!({}));
                        let session_key = format!("{GW_SESSION_PREFIX}{session_id_for_reader}");
                        if let Some(svc) = crate::services::try_get_services() {
                            svc.set_session_debug(
                                &session_key,
                                submode.id().to_string(),
                                params.clone(),
                            );
                        }
                        let ack = serde_json::json!({
                            "type": "debug_submode_set",
                            "submode": submode.id(),
                            "params": params,
                        });
                        let _ = control_tx_reader
                            .try_send(OutboundFrame::Text(ack.to_string()));
                        continue;
                    }
                    if msg_type.as_str() == "cancel_tool" {
                        let requested_session = parsed
                            .get("sessionId")
                            .or_else(|| parsed.get("session_id"))
                            .and_then(|v| v.as_str());
                        if let Some(req) = requested_session {
                            if req != session_id_for_reader.as_str() {
                                tracing::warn!(
                                    target: "agent_cancel",
                                    requested = %req,
                                    connection_session = %session_id_for_reader,
                                    "cancel_tool rejected: sessionId does not belong to this connection"
                                );
                                continue;
                            }
                        }
                        let killed = crate::tools::background::registry::kill_foreground(
                            session_id_for_reader.as_str(),
                            Some(connection_id_for_reader.as_str()),
                        );
                        tracing::info!(
                            target: "agent_cancel",
                            session = %session_id_for_reader,
                            killed,
                            "cancel_tool received: foreground shell kill requested"
                        );
                        continue;
                    }
                    match msg_type.as_str() {

                        "permission_response" => {
                            if let Some(request_id) =
                                parsed.get("requestId").and_then(|v| v.as_str())
                            {
                                if !crate::approval::claim_pending_gateway_approval_for_session(
                                    request_id,
                                    Some(session_id_for_reader.as_str()),
                                ) {
                                    tracing::debug!(
                                        target: "ws_desktop_gate",
                                        request_id = %request_id,
                                        "desktop approval ignored: already claimed by another responder"
                                    );
                                    continue;
                                }
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
                                crate::approval::record_session_decision_delivery(
                                    request_id,
                                    if allowed { "yes" } else { "no" },
                                );
                                let _ = crate::approval::drop_pending_gateway_approval(
                                    request_id,
                                );
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
                                        match tokio::time::timeout(
                                            std::time::Duration::from_secs(30),
                                            inbound_tx.send(synthetic),
                                        )
                                        .await
                                        {
                                            Ok(Ok(())) => {}
                                            Ok(Err(_)) => break,
                                            Err(_) => {
                                                tracing::error!(
                                                    target: "ws_desktop",
                                                    request_id = %request_id,
                                                    "inbound channel saturated for 30s; synthetic ask-response could not be delivered and the waiting turn may stall"
                                                );
                                            }
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
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        inbound_tx.send(parsed),
                    )
                    .await
                    {
                        Ok(Ok(())) => {}
                        Ok(Err(_)) => break,
                        Err(_) => {
                            tracing::warn!(
                                target: "ws_desktop",
                                "inbound channel saturated for >5s; dropping frame to keep reader responsive for ping/approval"
                            );
                        }
                    }
                }
                Ok(Message::Ping(payload)) => {
                    let _ = control_tx_reader.try_send(OutboundFrame::Pong(payload.to_vec()));
                }
                Ok(Message::Close(_)) | Err(_) => break,
                _ => {}
            }
        }

        conn_token_reader.cancel();

        let grace_token = register_disconnect_grace(&session_id_for_reader);
        let grace_secs = DESKTOP_RECONNECT_GRACE_SECS;
        tracing::info!(
            target: "agent_cancel",
            session = %session_id_for_reader,
            grace_secs,
            "websocket disconnected: arming reconnect grace window before cancelling any in-flight turn"
        );
        crate::runtime::spawn_supervised("ws_desktop.disconnect_grace", async move {
            tokio::select! {
                _ = grace_token.cancelled() => {
                    tracing::info!(
                        target: "agent_cancel",
                        session = %session_id_for_reader,
                        "websocket reconnected within grace window: in-flight turn preserved"
                    );
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(grace_secs)) => {
                    let live_connections = session_live_connections()
                        .lock()
                        .get(&session_id_for_reader)
                        .copied()
                        .unwrap_or(0);
                    if live_connections > 0 {
                        tracing::info!(
                            target: "agent_cancel",
                            session = %session_id_for_reader,
                            live_connections,
                            "reconnect grace window elapsed but the session has live connections (client already reconnected); leaving the in-flight turn untouched"
                        );
                    } else {
                        cancelled_atomic_for_reader
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                        cancel_signal_for_reader.load_full().cancel();
                        tracing::info!(
                            target: "agent_cancel",
                            session = %session_id_for_reader,
                            "reconnect grace window elapsed: firing cancel to stop any orphaned in-flight turn"
                        );
                    }
                    clear_disconnect_grace_slot(&session_id_for_reader, &grace_token);
                }
            }
        });
    });

    let lsp_forwarder = {
        let mut rx = state.lsp_events.subscribe();
        let tx = inbound_tx_lsp;
        crate::runtime::spawn_supervised("ws_desktop.lsp_forwarder", async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if let Ok(payload) = serde_json::to_value(&event) {
                            let wrapped = serde_json::json!({
                                "type": "__lsp_forward__",
                                "payload": payload,
                            });
                            match tx.try_send(wrapped) {
                                Ok(()) => {}
                                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                                    tracing::warn!(
                                        target: "ws_desktop",
                                        "inbound channel full; dropping lsp forward frame"
                                    );
                                }
                                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => break,
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
        let session_id_for_gateway = session_id.clone();
        crate::runtime::spawn_supervised("ws_desktop.gateway_forwarder", async move {
            loop {
                match rx.recv().await {
                    Ok(payload) => {
                        let payload_type = payload
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let is_forwardable = matches!(
                            payload_type,
                            "system_notification" | "usage_updated" | "task_update"
                        );
                        if !is_forwardable {
                            continue;
                        }
                        {
                            let target = payload
                                .get("sessionId")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            if !target.is_empty() && target != session_id_for_gateway {
                                continue;
                            }
                        }
                        let wrapped = serde_json::json!({
                            "type": "__gateway_event__",
                            "payload": payload,
                        });
                        match tx.try_send(wrapped) {
                            Ok(()) => {}
                            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                                tracing::warn!(
                                    target: "ws_desktop",
                                    "inbound channel full; dropping gateway event frame"
                                );
                            }
                            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => break,
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(dropped)) => {
                        let notice = serde_json::json!({
                            "type": "system_notification",
                            "subtype": "stream_lagged",
                            "level": "warning",
                            "data": { "dropped": dropped, "channel": "gateway_events" },
                        });
                        let _ = tx.try_send(notice);
                        continue;
                    }
                    Err(_) => break,
                }
            }
        })
    };

    let resource_event_forwarder = {
        let mut rx = state.workspace_resources.subscribe();
        let tx = inbound_tx_resource.clone();
        let session_id_for_resource = session_id.clone();
        crate::runtime::spawn_supervised("ws_desktop.resource_forwarder", async move {
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
                        match tx.try_send(wrapped) {
                            Ok(()) => {}
                            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                                tracing::warn!(
                                    target: "ws_desktop",
                                    "inbound channel full; dropping resource event frame"
                                );
                            }
                            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => break,
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        })
    };

    let heartbeat_handle = {
        let conn_token_ping = conn_token.clone();
        let last_activity_ping = std::sync::Arc::clone(&last_activity);
        crate::runtime::spawn_supervised("ws_desktop.heartbeat", async move {
            let interval = std::time::Duration::from_secs(DESKTOP_WS_PING_INTERVAL_SECS);
            let mut prev_tick = desktop_now_unix_secs();
            loop {
                tokio::select! {
                    _ = conn_token_ping.cancelled() => break,
                    _ = tokio::time::sleep(interval) => {}
                }
                let now = desktop_now_unix_secs();
                let since_prev_tick = now.saturating_sub(prev_tick);
                prev_tick = now;
                if since_prev_tick > DESKTOP_WS_PING_INTERVAL_SECS.saturating_mul(2) {
                    last_activity_ping.store(now, std::sync::atomic::Ordering::Relaxed);
                    continue;
                }
                let idle = now.saturating_sub(
                    last_activity_ping.load(std::sync::atomic::Ordering::Relaxed),
                );
                if idle >= DESKTOP_WS_IDLE_TIMEOUT_SECS {
                    tracing::info!(
                        target: "ws_desktop",
                        idle_secs = idle,
                        "desktop websocket idle beyond timeout (half-open?); cancelling connection so the session is not left stuck busy"
                    );
                    conn_token_ping.cancel();
                    break;
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
            let _ = inbound_tx_resource.try_send(wrapped);
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
        let _ = inbound_tx_resource.try_send(wrapped);
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
                let _ = inbound_tx_replay.try_send(wrapped);
            }
            sent += 1;
            if sent.is_multiple_of(REPLAY_BATCH) {
                tokio::task::yield_now().await;
            }
        }
    }

    let mut pending_ask_request_id: Option<String> = None;
    loop {
        let parsed = tokio::select! {
            biased;
            _ = conn_token.cancelled() => break,
            frame = inbound_rx.recv() => match frame {
                Some(p) => p,
                None => break,
            },
        };
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
                if mode_str.eq_ignore_ascii_case("auto") {
                    if let Some(svc) = crate::services::try_get_services() {
                        svc.set_session_auto_coding_mode(&session_key, true);
                        if scope == "global" {
                            svc.set_global_auto_coding_mode(true);
                        }
                    }
                    let derived = "default";
                    desktop_runtime_state().set_session_permission_mode(&session_key, derived);
                    let _ = send_json(
                        &outbound_tx,
                        &serde_json::json!({
                            "type": "system_notification",
                            "subtype": "coding_mode_updated",
                            "message": "Coding mode: Auto (intent-routed)",
                            "data": {
                                "mode": "auto",
                                "label": "Auto",
                                "permissionMode": derived,
                                "scope": scope,
                                "auto": true,
                            },
                        }),
                    )
                    .await;
                    continue;
                }
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

                    let auto_approved = crate::agent::mode::transition::is_auto_approved(
                        whitelist,
                        previous_mode,
                        parsed_mode,
                    );
                    let needs_confirm =
                        !confirmed && previous_mode != parsed_mode && !auto_approved;
                    if needs_confirm {
                        let _ = send_json(
                            &outbound_tx,
                            &serde_json::json!({
                                "type": "system_notification",
                                "subtype": "coding_mode_confirm_required",
                                "message": format!(
                                    "Switching coding mode {} -> {} requires confirmation",
                                    previous_mode.display_name(),
                                    parsed_mode.display_name(),
                                ),
                                "data": {
                                    "mode": parsed_mode.display_name(),
                                    "label": parsed_mode.label(),
                                    "scope": scope,
                                    "from": previous_mode.display_name(),
                                    "sessionId": session_id.as_str(),
                                },
                            }),
                        )
                        .await;
                        continue;
                    }

                    if let Some(svc) = svc_opt {
                        svc.set_session_auto_coding_mode(&session_key, false);
                        svc.set_session_coding_mode(&session_key, parsed_mode);
                        if scope == "global" {
                            svc.set_global_auto_coding_mode(false);
                            *svc.coding_mode.write() = parsed_mode;
                        }
                    }
                    set_coding_mode_scoped(&mut agent, &session_id, &connection_id, parsed_mode)
                        .await;
                    let derived = super::super::desktop::routes::derive_permission_from_coding(&parsed_mode);
                    desktop_runtime_state().set_session_permission_mode(&session_key, derived);
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
                let request_id = parsed
                    .get("requestId")
                    .and_then(|value| value.as_str())
                    .map(str::to_string);

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
                let mut succeeded = provider.is_some() && model.is_some();

                let mut candidate = { state.config.lock().clone() };

                if let Some(p) = provider.as_deref() {
                    if let Some(profile) = candidate.model_providers.get(p).cloned() {
                        crate::gateway::desktop::routes::apply_active_profile_to_top_level(
                            &mut candidate, p, &profile,
                        );
                    } else {
                        candidate.default_provider = Some(p.to_string());
                    }
                }
                if let Some(m) = model.as_ref() {
                    candidate.default_model = Some(m.clone());
                }

                if let Err(e) = candidate.validate() {
                    succeeded = false;
                    tracing::warn!(
                        target: "ws_desktop_runtime_config",
                        error = %e,
                        "set_runtime_config: merged runtime config failed full validation; \
                         keeping previous config active"
                    );
                    let _ = send_json(
                        &outbound_tx,
                        &serde_json::json!({
                            "type": "system_notification",
                            "subtype": "runtime_config_validation_failed",
                            "level": "error",
                            "message": format!("{e:#}"),
                            "data": {
                                "requestId": request_id.as_deref(),
                            },
                        }),
                    )
                    .await;
                } else {
                    if let (Some(p), Some(m)) = (provider.as_ref(), model.as_ref()) {
                        agent.signal_runtime_model_switch(p.clone(), m.clone());
                    }

                    if persist {
                        *state.config.lock() = candidate.clone();

                        if let Err(e) = crate::gateway::persist_config(&candidate).await {
                            succeeded = false;
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
                                    "level": "warning",
                                    "message": format!("{e:#}"),
                                    "data": {
                                        "requestId": request_id.as_deref(),
                                    },
                                }),
                            )
                            .await;
                        }

                        state.push_live_config(candidate);
                        state.rebuild_runtime_from_config_async().await;
                    }

                    if let Err(e) = agent.apply_runtime_config_now().await {
                        succeeded = false;
                        tracing::warn!(
                            target: "ws_desktop_runtime_config",
                            error = %e,
                            "set_runtime_config: failed to apply runtime config to session agent"
                        );
                    }
                }
                let _ = send_json(
                    &outbound_tx,
                    &serde_json::json!({
                        "type": "system_notification",
                        "subtype": if succeeded {
                            "runtime_config_updated"
                        } else {
                            "runtime_config_apply_failed"
                        },
                        "data": {
                            "persisted": persist,
                            "requestId": request_id.as_deref(),
                        },
                    }),
                )
                .await;
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
                crate::tools::browser::set_test_target_tab(&session_id, tab_id as u32);
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
                if crate::tools::browser::current_test_target_tab(&session_id)
                    == Some(tab_id as u32)
                {
                    crate::tools::browser::clear_test_target_tab(&session_id);
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
                let figma_url = parsed
                    .get("figma_url")
                    .or_else(|| parsed.get("figmaUrl"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                if let Some(url) = figma_url {
                    if !url.contains("figma.com/") {
                        send_error(
                            &outbound_tx,
                            "debug_bind_prototype_ref.figma_url must be a figma.com URL",
                            "INVALID_FIGMA_URL",
                        )
                        .await;
                        continue;
                    }
                    crate::tools::browser::set_prototype_ref_figma(&session_id, url);
                    let _ = send_json(
                        &outbound_tx,
                        &serde_json::json!({
                            "type": "system_notification",
                            "subtype": "prototype_ref_bound",
                            "data": { "figma_url": url },
                        }),
                    )
                    .await;
                    continue;
                }
                let Some(tab_id) = tab_id else {
                    send_error(
                        &outbound_tx,
                        "debug_bind_prototype_ref requires tab_id or figma_url",
                        "EMPTY_TAB_ID",
                    )
                    .await;
                    continue;
                };
                crate::tools::browser::set_prototype_ref_tab(&session_id, tab_id as u32);
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
                crate::tools::browser::clear_prototype_ref_tab(&session_id);
                crate::tools::browser::clear_prototype_ref_figma(&session_id);
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
                let mut raw_content = parsed
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let attachments: Vec<serde_json::Value> = parsed
                    .get("attachments")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let mut display_content = parsed
                    .get("displayContent")
                    .or_else(|| parsed.get("display_content"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());
                if raw_content.is_empty() && attachments.is_empty() {
                    send_error(&outbound_tx, "empty user_message.content", "EMPTY_CONTENT").await;
                    continue;
                }

                let client_msg_id = parsed
                    .get("clientMsgId")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());
                if let Some(ref cid) = client_msg_id {
                    if client_msg_id_is_duplicate(&session_key, cid) {
                        send_user_message_ack(&outbound_tx, &session_id, cid).await;
                        continue;
                    }
                }

                let is_ask_response = parsed
                    .get("source")
                    .and_then(|v| v.as_str())
                    .map(|s| s == "ask_response")
                    .unwrap_or(false);

                if attachments.is_empty() && !is_ask_response {
                    let slash_probe = raw_content.trim().to_string();
                    if is_probable_slash_command(&slash_probe) {
                        use crate::commands::dispatch::SlashOutcome;
                        let outcome = crate::commands::dispatch::dispatch_slash_input_scoped(
                            &slash_probe,
                            session_id.clone(),
                            agent.current_workspace_dir().to_path_buf(),
                            false,
                            false,
                        )
                        .await;
                        match outcome {
                            SlashOutcome::NotCommand => {}
                            SlashOutcome::Quit => {
                                send_slash_command_result(
                                    &outbound_tx,
                                    &slash_probe,
                                    true,
                                    "This command exits the CLI REPL; in the desktop app, close the session tab instead.",
                                )
                                .await;
                                continue;
                            }
                            SlashOutcome::Clear => {
                                agent.clear_history();
                                send_slash_command_result(
                                    &outbound_tx,
                                    &slash_probe,
                                    true,
                                    "Conversation context cleared for this session; the saved transcript is kept.",
                                )
                                .await;
                                continue;
                            }
                            SlashOutcome::Handled { success, message } => {
                                let message = if message.trim().is_empty() {
                                    if success {
                                        "Command executed.".to_string()
                                    } else {
                                        "Command failed.".to_string()
                                    }
                                } else {
                                    message
                                };
                                send_slash_command_result(
                                    &outbound_tx,
                                    &slash_probe,
                                    success,
                                    &message,
                                )
                                .await;
                                continue;
                            }
                            SlashOutcome::Followup { message, prompt } => {
                                if let Some(note) =
                                    message.filter(|m| !m.trim().is_empty())
                                {
                                    send_slash_command_result(
                                        &outbound_tx,
                                        &slash_probe,
                                        true,
                                        &note,
                                    )
                                    .await;
                                }
                                if display_content.is_none() {
                                    display_content = Some(slash_probe);
                                }
                                raw_content = prompt;
                            }
                        }
                    }
                }

                if !is_ask_response && state.session_run_state.is_running(&session_id) {
                    let workspace_key = crate::session::workspace_key_from_path(
                        agent.current_workspace_dir(),
                        &session_id,
                    );
                    tracing::warn!(
                        session_id = %session_id,
                        workspace_key = %workspace_key,
                        "user_message rejected before persist: session already running",
                    );
                    let _ = send_json(
                        &outbound_tx,
                        &serde_json::json!({
                            "type": "workspace_busy",
                            "workspaceKey": workspace_key,
                            "currentSessionId": session_id,
                        }),
                    )
                    .await;
                    continue;
                }

                let content = if attachments.is_empty() {
                    raw_content
                } else {
                    let workspace = agent.current_workspace_dir().to_path_buf();
                    let raw_for_enrich = raw_content.clone();
                    match tokio::task::spawn_blocking(move || {
                        enrich_content_with_attachments(&raw_for_enrich, &attachments, &workspace)
                    })
                    .await
                    {
                        Ok(c) => c,
                        Err(e) => {
                            tracing::warn!(
                                target: "ws_desktop_attachments",
                                error = %e,
                                "attachment enrichment task panicked; sending text content only"
                            );
                            raw_content
                        }
                    }
                };

                if state.session_backend.is_some() {
                    let mut user_msg = crate::providers::ChatMessage::user(&content);
                    if let Some(ref display) = display_content {
                        user_msg.metadata.insert(
                            "display_content".to_string(),
                            serde_json::Value::String(display.clone()),
                        );
                    }
                    if let Some(ref cid) = client_msg_id {
                        user_msg.metadata.insert(
                            "client_msg_id".to_string(),
                            serde_json::Value::String(cid.clone()),
                        );
                    }
                    enqueue_persist(&state, &session_key, vec![user_msg]).await;
                }
                if let Some(ref cid) = client_msg_id {
                    note_client_msg_id(&session_key, cid);
                    send_user_message_ack(&outbound_tx, &session_id, cid).await;
                }

                if let Some(svc) = crate::services::try_get_services() {
                    let resolved = svc.resolve_coding_mode_for(Some(&session_key));
                    set_coding_mode_scoped(&mut agent, &session_id, &connection_id, resolved)
                        .await;
                }

                if let Err(e) = agent.apply_runtime_config_now().await {
                    tracing::warn!(
                        target: "ws_desktop_runtime_config",
                        error = %e,
                        "user_message: failed to apply live runtime config before turn"
                    );
                }

                if is_ask_response {
                    agent.mark_resuming_from_ask();
                } else if let Some(stale) = pending_ask_request_id.take() {
                    let _ = crate::approval::drop_pending_gateway_approval(&stale);
                }

                agent.reset_cancel();
                pending_ask_request_id = run_turn(&state, &mut agent, &outbound_tx, &session_id, &session_key, &connection_id, &content).await;
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
                }
                set_coding_mode_scoped(&mut agent, &session_id, &connection_id, agent_mode).await;
                let derived =
                    super::super::desktop::routes::derive_permission_from_coding(&agent_mode);
                desktop_runtime_state().set_session_permission_mode(&session_key, derived);
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

                if state.session_backend.is_some() {
                    let trigger_msg =
                        crate::providers::ChatMessage::user(&trigger_content);
                    enqueue_persist_rows(
                        &state,
                        &session_key,
                        vec![PersistRow {
                            msg: trigger_msg,
                            hidden: true,
                        }],
                    )
                    .await;
                }

                if state.session_backend.is_some() {
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
                    enqueue_persist(&state, &session_key, vec![marker_msg]).await;
                }

                {
                    let snap = state.config.lock().clone();
                    state.push_live_config(snap);
                }

                if state.session_run_state.is_running(&session_id) {
                    let workspace_key = crate::session::workspace_key_from_path(
                        agent.current_workspace_dir(),
                        &session_id,
                    );
                    tracing::warn!(
                        session_id = %session_id,
                        workspace_key = %workspace_key,
                        "start_plan_execution rejected: session already running",
                    );
                    let _ = send_json(
                        &outbound_tx,
                        &serde_json::json!({
                            "type": "workspace_busy",
                            "workspaceKey": workspace_key,
                            "currentSessionId": session_id,
                        }),
                    )
                    .await;
                    continue;
                }

                agent.arm_plan_execution(plan_path.clone());

                agent.reset_cancel();
                pending_ask_request_id = run_turn(
                    &state,
                    &mut agent,
                    &outbound_tx,
                    &session_id,
                    &session_key,
                    &connection_id,
                    &trigger_content,
                )
                .await;
            }
            "start_design_generation" => {
                let submode_id = parsed
                    .get("submode")
                    .or_else(|| parsed.get("subMode"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let Some(submode) =
                    crate::agent::designer::DesignerSubMode::from_id(&submode_id)
                else {
                    send_error(
                        &outbound_tx,
                        &format!("unknown designer submode: {submode_id}"),
                        "UNKNOWN_DESIGN_SUBMODE",
                    )
                    .await;
                    continue;
                };
                let params = parsed
                    .get("params")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));
                let brief = parsed
                    .get("brief")
                    .or_else(|| parsed.get("content"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let ref_artifact = parsed
                    .get("refArtifact")
                    .or_else(|| parsed.get("ref_artifact"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());
                let ref_artifact_name = parsed
                    .get("refArtifactName")
                    .or_else(|| parsed.get("ref_artifact_name"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());
                let ref_element = parsed
                    .get("refElement")
                    .or_else(|| parsed.get("ref_element"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());
                let ref_element_label = parsed
                    .get("refElementLabel")
                    .or_else(|| parsed.get("ref_element_label"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());
                let attachments: Vec<serde_json::Value> = parsed
                    .get("attachments")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();

                let designer_mode = crate::agent::coding_mode::CodingMode::Designer;
                if let Some(svc) = crate::services::try_get_services() {
                    svc.set_session_coding_mode(&session_key, designer_mode);
                    svc.set_session_designer(
                        &session_key,
                        submode.id().to_string(),
                        params.clone(),
                        ref_artifact.clone(),
                    );
                }
                set_coding_mode_scoped(&mut agent, &session_id, &connection_id, designer_mode)
                    .await;
                let derived =
                    super::super::desktop::routes::derive_permission_from_coding(&designer_mode);
                desktop_runtime_state().set_session_permission_mode(&session_key, derived);
                let _ = send_json(
                    &outbound_tx,
                    &serde_json::json!({
                        "type": "system_notification",
                        "subtype": "coding_mode_updated",
                        "message": format!("Coding mode: {}", designer_mode.label()),
                        "data": {
                            "mode": designer_mode.display_name(),
                            "label": designer_mode.label(),
                            "permissionMode": derived,
                            "designSubmode": submode.id(),
                        },
                    }),
                )
                .await;

                let existing_decks = if matches!(
                    submode,
                    crate::agent::designer::DesignerSubMode::Deck
                ) {
                    crate::agent::designer::pipeline::list_existing_decks(
                        agent.current_workspace_dir(),
                        &session_id,
                    )
                } else {
                    Vec::new()
                };
                let brief_is_resume = ref_artifact
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .is_none()
                    && crate::agent::designer::pipeline::is_continuation_brief(&brief);
                let (attachment_suffix, attachment_paths) = if attachments.is_empty() {
                    (String::new(), Vec::new())
                } else {
                    let workspace = agent.current_workspace_dir().to_path_buf();
                    match tokio::task::spawn_blocking(move || {
                        enrich_content_with_attachments("", &attachments, &workspace)
                    })
                    .await
                    {
                        Ok(suffix) => {
                            let (cleaned, image_paths) =
                                crate::multimodal::parse_image_markers(&suffix);
                            let mut paths = image_paths;
                            for line in cleaned.lines() {
                                if let Some(rest) =
                                    line.trim().strip_prefix("[Attached file:")
                                    && let Some(p) = rest.strip_suffix(']')
                                {
                                    let p = p.trim();
                                    if !p.is_empty() {
                                        paths.push(p.to_string());
                                    }
                                }
                            }
                            (suffix, paths)
                        }
                        Err(e) => {
                            tracing::warn!(
                                target: "ws_desktop_attachments",
                                error = %e,
                                "design attachment enrichment task panicked; continuing without attachments"
                            );
                            (String::new(), Vec::new())
                        }
                    }
                };
                let mut trigger_content = if brief_is_resume {
                    crate::agent::designer::pipeline::build_design_resume_message(
                        &session_id,
                        &brief,
                    )
                } else {
                    crate::agent::designer::pipeline::build_design_task_message(
                        submode,
                        &params,
                        &brief,
                        ref_artifact.as_deref(),
                        ref_element.as_deref(),
                        ref_element_label.as_deref(),
                        &session_id,
                        &existing_decks,
                    )
                };
                if !attachment_suffix.is_empty() {
                    if !attachment_paths.is_empty() {
                        trigger_content.push_str(
                            "\n\nUser-attached reference files (already saved to disk; \
                             usable as tool inputs such as `media_generate source_image=...` \
                             or `view_image`):",
                        );
                        for p in &attachment_paths {
                            trigger_content.push_str(&format!("\n- {p}"));
                        }
                    }
                    trigger_content.push_str(&attachment_suffix);
                }

                let persisted_user_text = if brief.trim().is_empty() {
                    trigger_content.clone()
                } else {
                    format!("{brief}{attachment_suffix}")
                };
                if state.session_backend.is_some() {
                    let mut user_msg = crate::providers::ChatMessage::user(&persisted_user_text);
                    if let Some(ref target) = ref_artifact {
                        user_msg.metadata.insert(
                            "design_ref".to_string(),
                            serde_json::Value::String(target.clone()),
                        );
                        if let Some(ref name) = ref_artifact_name {
                            user_msg.metadata.insert(
                                "design_ref_name".to_string(),
                                serde_json::Value::String(name.clone()),
                            );
                        }
                        if let Some(ref element) = ref_element {
                            user_msg.metadata.insert(
                                "design_ref_element".to_string(),
                                serde_json::Value::String(element.clone()),
                            );
                            if let Some(ref label) = ref_element_label {
                                user_msg.metadata.insert(
                                    "design_ref_element_label".to_string(),
                                    serde_json::Value::String(label.clone()),
                                );
                            }
                        }
                    }
                    enqueue_persist(&state, &session_key, vec![user_msg]).await;
                }

                if let Err(e) = agent.apply_runtime_config_now().await {
                    tracing::warn!(
                        target: "ws_desktop_runtime_config",
                        error = %e,
                        "start_design_generation: failed to apply live runtime config before turn"
                    );
                }

                agent.reset_cancel();
                pending_ask_request_id = run_turn(
                    &state,
                    &mut agent,
                    &outbound_tx,
                    &session_id,
                    &session_key,
                    &connection_id,
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

    conn_token.cancel();
    turn_abort_token.cancel();
    reader_handle.abort();
    heartbeat_handle.abort();
    lsp_forwarder.abort();
    gateway_event_forwarder.abort();
    resource_event_forwarder.abort();
    unregister_session_sender(&session_id, sender_seq);
    drop(outbound_tx);
    let _ = writer_handle.await;

    let last_connection_for_session = unregister_session_connection(&session_id);
    if !last_connection_for_session {
        tracing::debug!(
            target: "ws_desktop",
            session_id = %session_id,
            "skipping session teardown: a newer live connection owns this session"
        );
        return;
    }

    if let Some(svc) = crate::services::try_get_services() {
        svc.clear_session_designer(&session_key);
        svc.clear_session_debug(&session_key);
    }
    desktop_runtime_state().clear_session_permission_mode(&session_key);
    crate::security::sandbox::unregister_session_workspace_root(&session_id);
    if let Some(mgr) = crate::session::global_workspace_resources() {
        mgr.clear_session_snapshots(&session_id);
    }

    let _ = crate::services::governance::credential_vault::purge_session_ephemeral(&session_key);
    if let Some(ctl) = crate::tools::browser::dock_controller() {
        let session_key_for_release = session_key.clone();
        crate::runtime::task_manager::spawn_supervised(
            "ws_desktop.release_agent_tabs",
            async move {
                if let Err(err) = ctl
                    .release_agent_tabs_for_session(session_key_for_release)
                    .await
                {
                    tracing::warn!(
                        "[ws_desktop] release_agent_tabs_for_session failed: {err}"
                    );
                }
            },
        );
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
        if snapshot.auto_distill_on_session_end && snapshot.distill_enabled {
            let engine_for_distill = std::sync::Arc::clone(&engine);
            let session_for_distill = session_id.to_string();
            tokio::task::spawn_blocking(move || {
                let store = std::sync::Arc::clone(engine_for_distill.store());
                let turn_ids = match store.top_session_turn_ids_by_reward(
                    &session_for_distill,
                    0.5,
                    3,
                ) {
                    Ok(ids) => ids,
                    Err(error) => {
                        tracing::debug!(
                            error = %error,
                            "evolution: session-end distill query failed"
                        );
                        return;
                    }
                };
                for turn_id in turn_ids {
                    if store.has_distill_audit(&turn_id) {
                        continue;
                    }
                    if let Ok(Some(turn)) = store.find_turn_record(&turn_id) {
                        let _ = engine_for_distill
                            .enqueue_distill_forced(crate::evolution::DistillRequest { turn });
                    }
                }
            });
        }
    }
}

pub struct DesktopRuntimeState {
    permission_mode: parking_lot::RwLock<String>,
    session_permission_modes:
        parking_lot::RwLock<std::collections::HashMap<String, String>>,
    settings_path: parking_lot::RwLock<Option<std::path::PathBuf>>,
    hydrated: std::sync::atomic::AtomicBool,
}

impl DesktopRuntimeState {
    fn new() -> Self {
        Self {

            permission_mode: parking_lot::RwLock::new("default".to_string()),
            session_permission_modes: parking_lot::RwLock::new(
                std::collections::HashMap::new(),
            ),
            settings_path: parking_lot::RwLock::new(None),
            hydrated: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn set_settings_path(&self, path: std::path::PathBuf) {
        *self.settings_path.write() = Some(path.clone());
        self.prewarm(path);
    }

    fn prewarm(&self, path: std::path::PathBuf) {
        let hydrate = move || {
            let state = desktop_runtime_state();
            state.ensure_hydrated_from(Some(&path));
        };
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::task::block_in_place(hydrate);
        } else {
            hydrate();
        }
    }

    fn ensure_hydrated(&self) {
        let path = self.settings_path.read().clone();
        self.ensure_hydrated_from(path.as_deref());
    }

    fn ensure_hydrated_from(&self, path: Option<&std::path::Path>) {
        use std::sync::atomic::Ordering;
        if self.hydrated.load(Ordering::Acquire) {
            return;
        }
        let disk_mode = path.and_then(read_permission_mode_from_disk);
        let mut guard = self.permission_mode.write();
        if self.hydrated.load(Ordering::Acquire) {
            return;
        }
        if let Some(mode) = disk_mode {
            *guard = mode;
        }
        drop(guard);
        self.hydrated.store(true, Ordering::Release);
    }

    pub fn permission_mode(&self) -> String {
        self.ensure_hydrated();
        self.permission_mode.read().clone()
    }

    pub fn permission_mode_for(&self, session_key: &str) -> String {
        if let Some(mode) = self.session_permission_modes.read().get(session_key) {
            return mode.clone();
        }
        self.permission_mode()
    }

    pub fn session_permission_mode_opt(&self, session_key: &str) -> Option<String> {
        self.session_permission_modes
            .read()
            .get(session_key)
            .cloned()
    }

    pub fn ensure_session_permission_mode(&self, session_key: &str, mode: &str) -> String {
        let mut map = self.session_permission_modes.write();
        if let Some(existing) = map.get(session_key) {
            return existing.clone();
        }
        map.insert(session_key.to_string(), mode.to_string());
        mode.to_string()
    }

    pub fn set_session_permission_mode(&self, session_key: &str, mode: &str) {
        self.session_permission_modes
            .write()
            .insert(session_key.to_string(), mode.to_string());
    }

    pub fn clear_session_permission_mode(&self, session_key: &str) {
        self.session_permission_modes.write().remove(session_key);
    }

    pub fn set_permission_mode(&self, mode: &str) {

        self.hydrated
            .store(true, std::sync::atomic::Ordering::Release);
        let canonical = crate::config::normalize_desktop_permission_mode(mode)
            .unwrap_or("default")
            .to_string();
        *self.permission_mode.write() = canonical.clone();
        let path = self.settings_path.read().clone();
        if let Some(p) = path {
            let mode_owned = canonical;
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

fn read_permission_mode_from_disk(path: &std::path::Path) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    let json = serde_json::from_str::<serde_json::Value>(&contents).ok()?;
    let mode = json.get("permissionMode").and_then(|v| v.as_str())?;
    crate::config::normalize_desktop_permission_mode(mode).map(|s| s.to_string())
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
    crate::util::atomic_write(path, serialized.as_bytes())
}

pub fn desktop_runtime_state() -> &'static DesktopRuntimeState {
    static STATE: std::sync::OnceLock<DesktopRuntimeState> = std::sync::OnceLock::new();
    STATE.get_or_init(DesktopRuntimeState::new)
}

const DESKTOP_RECONNECT_GRACE_SECS: u64 = 60;

fn desktop_now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

const DESKTOP_WS_PING_INTERVAL_SECS: u64 = 30;
const DESKTOP_WS_IDLE_TIMEOUT_SECS: u64 = 300;

type DisconnectGraceToken = std::sync::Arc<tokio_util::sync::CancellationToken>;

fn disconnect_grace_registry(
) -> &'static parking_lot::Mutex<std::collections::HashMap<String, DisconnectGraceToken>> {
    static REG: std::sync::OnceLock<
        parking_lot::Mutex<std::collections::HashMap<String, DisconnectGraceToken>>,
    > = std::sync::OnceLock::new();
    REG.get_or_init(|| parking_lot::Mutex::new(std::collections::HashMap::new()))
}

fn register_disconnect_grace(session_id: &str) -> DisconnectGraceToken {
    let token: DisconnectGraceToken =
        std::sync::Arc::new(tokio_util::sync::CancellationToken::new());
    if let Some(prev) = disconnect_grace_registry()
        .lock()
        .insert(session_id.to_string(), token.clone())
    {
        prev.cancel();
    }
    token
}

fn abort_disconnect_grace(session_id: &str) {
    if let Some(token) = disconnect_grace_registry().lock().remove(session_id) {
        token.cancel();
    }
}

fn session_live_connections(
) -> &'static parking_lot::Mutex<std::collections::HashMap<String, usize>> {
    static REG: std::sync::OnceLock<
        parking_lot::Mutex<std::collections::HashMap<String, usize>>,
    > = std::sync::OnceLock::new();
    REG.get_or_init(|| parking_lot::Mutex::new(std::collections::HashMap::new()))
}

fn register_session_connection(session_id: &str) {
    *session_live_connections()
        .lock()
        .entry(session_id.to_string())
        .or_insert(0) += 1;
}

fn unregister_session_connection(session_id: &str) -> bool {
    let mut guard = session_live_connections().lock();
    match guard.get_mut(session_id) {
        Some(n) if *n > 1 => {
            *n -= 1;
            false
        }
        Some(_) => {
            guard.remove(session_id);
            true
        }
        None => true,
    }
}

fn session_out_senders(
) -> &'static parking_lot::Mutex<std::collections::HashMap<String, Vec<(u64, OutboundSender)>>>
{
    static REG: std::sync::OnceLock<
        parking_lot::Mutex<std::collections::HashMap<String, Vec<(u64, OutboundSender)>>>,
    > = std::sync::OnceLock::new();
    REG.get_or_init(|| parking_lot::Mutex::new(std::collections::HashMap::new()))
}

fn register_session_sender(session_id: &str, tx: &OutboundSender) -> u64 {
    static NEXT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let seq = NEXT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    session_out_senders()
        .lock()
        .entry(session_id.to_string())
        .or_default()
        .push((seq, tx.clone()));
    seq
}

fn unregister_session_sender(session_id: &str, seq: u64) {
    {
        let mut guard = session_out_senders().lock();
        if let Some(senders) = guard.get_mut(session_id) {
            senders.retain(|(s, _)| *s != seq);
            if senders.is_empty() {
                guard.remove(session_id);
            }
        }
    }
    clear_critical_overflow(seq);
}

fn is_critical_broadcast(payload: &serde_json::Value) -> bool {
    matches!(
        payload.get("type").and_then(|v| v.as_str()),
        Some(
            "session_history_changed"
                | "workspace_busy"
                | "permission_request"
                | "message_complete"
                | "tool_use_complete"
                | "tool_result"
                | "status"
        )
    )
}

const MAX_CRITICAL_OVERFLOW_FRAMES: usize = 1024;

struct CriticalOverflow {
    frames: std::collections::VecDeque<String>,
    draining: bool,
}

fn push_critical_bounded(state: &mut CriticalOverflow, text: String, sender_seq: u64) {
    if state.frames.len() >= MAX_CRITICAL_OVERFLOW_FRAMES {
        state.frames.pop_front();
        tracing::warn!(
            target: "ws_desktop",
            sender_seq,
            cap = MAX_CRITICAL_OVERFLOW_FRAMES,
            "critical overflow queue full; dropping oldest queued frame"
        );
    }
    state.frames.push_back(text);
}

fn critical_overflow_registry(
) -> &'static parking_lot::Mutex<std::collections::HashMap<u64, std::sync::Arc<parking_lot::Mutex<CriticalOverflow>>>>
{
    static REG: std::sync::OnceLock<
        parking_lot::Mutex<
            std::collections::HashMap<u64, std::sync::Arc<parking_lot::Mutex<CriticalOverflow>>>,
        >,
    > = std::sync::OnceLock::new();
    REG.get_or_init(|| parking_lot::Mutex::new(std::collections::HashMap::new()))
}

fn critical_overflow_slot(
    sender_seq: u64,
) -> std::sync::Arc<parking_lot::Mutex<CriticalOverflow>> {
    let mut registry = critical_overflow_registry().lock();
    std::sync::Arc::clone(registry.entry(sender_seq).or_insert_with(|| {
        std::sync::Arc::new(parking_lot::Mutex::new(CriticalOverflow {
            frames: std::collections::VecDeque::new(),
            draining: false,
        }))
    }))
}

fn spawn_critical_drainer(
    slot: std::sync::Arc<parking_lot::Mutex<CriticalOverflow>>,
    tx: OutboundSender,
) {
    crate::runtime::spawn_supervised(
        "ws_desktop.critical_broadcast_backpressure",
        async move {
            loop {
                let next = {
                    let mut state = slot.lock();
                    match state.frames.pop_front() {
                        Some(frame) => frame,
                        None => {
                            state.draining = false;
                            break;
                        }
                    }
                };
                if tx.send(OutboundFrame::Text(next)).await.is_err() {
                    let mut state = slot.lock();
                    state.frames.clear();
                    state.draining = false;
                    break;
                }
            }
        },
    );
}

fn send_critical_frame(sender_seq: u64, tx: &OutboundSender, text: String) -> bool {
    let slot = critical_overflow_slot(sender_seq);
    let mut state = slot.lock();
    if state.draining {
        push_critical_bounded(&mut state, text, sender_seq);
        return true;
    }
    match tx.try_send(OutboundFrame::Text(text)) {
        Ok(()) => true,
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => false,
        Err(tokio::sync::mpsc::error::TrySendError::Full(frame)) => {
            let OutboundFrame::Text(text_frame) = frame else {
                return true;
            };
            push_critical_bounded(&mut state, text_frame, sender_seq);
            state.draining = true;
            drop(state);
            spawn_critical_drainer(slot, tx.clone());
            true
        }
    }
}

fn clear_critical_overflow(sender_seq: u64) {
    critical_overflow_registry().lock().remove(&sender_seq);
}

pub fn broadcast_session_event(session_id: &str, payload: &serde_json::Value) {
    let senders: Vec<(u64, OutboundSender)> = {
        let guard = session_out_senders().lock();
        guard
            .get(session_id)
            .map(|list| list.clone())
            .unwrap_or_default()
    };
    if senders.is_empty() {
        return;
    }
    let text = payload.to_string();
    let critical = is_critical_broadcast(payload);
    let mut dead: Vec<u64> = Vec::new();
    for (seq, tx) in senders {
        if critical {
            if !send_critical_frame(seq, &tx, text.clone()) {
                dead.push(seq);
            }
            continue;
        }
        let draining = {
            let registry = critical_overflow_registry().lock();
            registry
                .get(&seq)
                .is_some_and(|slot| slot.lock().draining)
        };
        if draining {
            tracing::warn!(
                target: "ws_desktop",
                session_id,
                frame_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("?"),
                "dropping non-critical session frame: critical frames are queued ahead"
            );
            continue;
        }
        match tx.try_send(OutboundFrame::Text(text.clone())) {
            Ok(()) => {}
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => dead.push(seq),
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                tracing::warn!(
                    target: "ws_desktop",
                    session_id,
                    frame_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("?"),
                    "dropping non-critical session frame: outbound queue full"
                );
            }
        }
    }
    for seq in dead {
        unregister_session_sender(session_id, seq);
    }
}

fn clear_disconnect_grace_slot(session_id: &str, token: &DisconnectGraceToken) {
    let mut guard = disconnect_grace_registry().lock();
    if guard
        .get(session_id)
        .map(|existing| std::sync::Arc::ptr_eq(existing, token))
        .unwrap_or(false)
    {
        guard.remove(session_id);
    }
}

tokio::task_local! {
    static SCOPED_PERMISSION_MODE: String;
}

pub async fn scope_permission_mode<F>(mode: String, fut: F) -> F::Output
where
    F: std::future::Future,
{
    SCOPED_PERMISSION_MODE.scope(mode, fut).await
}

pub fn active_permission_mode() -> String {
    SCOPED_PERMISSION_MODE.try_with(|m| m.clone()).unwrap_or_else(|_| {
        let fallback = desktop_runtime_state().permission_mode();
        tracing::warn!(
            target: "isolation",
            fallback = %fallback,
            "active_permission_mode() called without session scope; falling back to global default",
        );
        fallback
    })
}

pub fn scoped_permission_mode_opt() -> Option<String> {
    SCOPED_PERMISSION_MODE.try_with(|m| m.clone()).ok()
}

pub fn global_permission_mode() -> String {
    desktop_runtime_state().permission_mode()
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

static SESSION_SEEN_CLIENT_MSG_IDS: once_cell::sync::Lazy<
    parking_lot::Mutex<std::collections::HashMap<String, std::collections::VecDeque<String>>>,
> = once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));

fn client_msg_id_is_duplicate(session_key: &str, client_msg_id: &str) -> bool {
    let map = SESSION_SEEN_CLIENT_MSG_IDS.lock();
    map.get(session_key)
        .is_some_and(|seen| seen.iter().any(|id| id == client_msg_id))
}

fn note_client_msg_id(session_key: &str, client_msg_id: &str) {
    const MAX_TRACKED_CLIENT_MSG_IDS: usize = 64;
    let mut map = SESSION_SEEN_CLIENT_MSG_IDS.lock();
    let seen = map.entry(session_key.to_string()).or_default();
    if seen.iter().any(|id| id == client_msg_id) {
        return;
    }
    seen.push_back(client_msg_id.to_string());
    while seen.len() > MAX_TRACKED_CLIENT_MSG_IDS {
        seen.pop_front();
    }
}

async fn send_user_message_ack(
    outbound: &OutboundSender,
    session_id: &str,
    client_msg_id: &str,
) {
    send_json(
        outbound,
        &serde_json::json!({
            "type": "user_message_ack",
            "sessionId": session_id,
            "clientMsgId": client_msg_id,
        }),
    )
    .await;
}

fn is_probable_slash_command(input: &str) -> bool {
    let Some(rest) = input.strip_prefix('/') else {
        return false;
    };
    let token = rest.split_whitespace().next().unwrap_or("");
    !token.is_empty()
        && token
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

async fn send_slash_command_result(
    outbound: &OutboundSender,
    command: &str,
    success: bool,
    message: &str,
) {
    send_json(
        outbound,
        &serde_json::json!({
            "type": "system_notification",
            "subtype": "slash_command_result",
            "level": if success { "info" } else { "error" },
            "message": message,
            "data": { "command": command, "success": success },
        }),
    )
    .await;
}

#[derive(Clone)]
struct PersistRow {
    msg: crate::providers::ChatMessage,
    hidden: bool,
}

struct PersistJob {
    session_key: String,
    rows: Vec<PersistRow>,
    attempts: u32,
}

const MAX_PERSIST_RETRIES: u32 = 20;

static PERSIST_PENDING: AtomicUsize = AtomicUsize::new(0);

static SESSION_PERSIST_PENDING: once_cell::sync::Lazy<
    parking_lot::Mutex<std::collections::HashMap<String, usize>>,
> = once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));

fn session_pending_inc(session_key: &str) {
    let mut map = SESSION_PERSIST_PENDING.lock();
    *map.entry(session_key.to_string()).or_insert(0) += 1;
}

fn session_pending_dec(session_key: &str) {
    let mut map = SESSION_PERSIST_PENDING.lock();
    if let Some(n) = map.get_mut(session_key) {
        *n = n.saturating_sub(1);
        if *n == 0 {
            map.remove(session_key);
        }
    }
}

fn session_pending_count(session_key: &str) -> usize {
    SESSION_PERSIST_PENDING
        .lock()
        .get(session_key)
        .copied()
        .unwrap_or(0)
}

pub(crate) async fn wait_session_persist_drained(
    session_key: &str,
    deadline: std::time::Duration,
) -> bool {
    let start = std::time::Instant::now();
    loop {
        if session_pending_count(session_key) == 0 {
            return true;
        }
        if start.elapsed() >= deadline {
            tracing::warn!(
                target: "ws_desktop_persist",
                session_key,
                pending = session_pending_count(session_key),
                "session persist queue did not drain within deadline; falling back to current db state"
            );
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

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

const PERSIST_QUEUE_CAPACITY: usize = 16384;

fn pop_blocked_job(
    blocked: &mut std::collections::HashMap<String, std::collections::VecDeque<PersistJob>>,
    session_key: &str,
) -> Option<PersistJob> {
    let queue = blocked.get_mut(session_key)?;
    let next = queue.pop_front();
    if queue.is_empty() {
        blocked.remove(session_key);
    }
    next
}

fn persist_sender(
    backend: &std::sync::Arc<dyn crate::channels::session::backend::SessionBackend>,
) -> &'static mpsc::Sender<PersistJob> {
    static SENDER: std::sync::OnceLock<mpsc::Sender<PersistJob>> =
        std::sync::OnceLock::new();
    SENDER.get_or_init(|| {
        let (tx, mut rx) = mpsc::channel::<PersistJob>(PERSIST_QUEUE_CAPACITY);
        let backend = std::sync::Arc::clone(backend);
        crate::runtime::spawn_supervised("ws_desktop.persist_worker", async move {
            let mut retry_queue: std::collections::VecDeque<(PersistJob, tokio::time::Instant)> =
                std::collections::VecDeque::new();
            let mut blocked: std::collections::HashMap<
                String,
                std::collections::VecDeque<PersistJob>,
            > = std::collections::HashMap::new();
            let mut channel_open = true;
            loop {
                let now = tokio::time::Instant::now();
                let due_now = retry_queue
                    .front()
                    .map(|(_, due)| *due <= now)
                    .unwrap_or(false);

                let mut job = if due_now {
                    match retry_queue.pop_front() {
                        Some((job, _)) => job,
                        None => continue,
                    }
                } else if let Some(due) = retry_queue.front().map(|(_, due)| *due) {
                    tokio::select! {
                        biased;
                        incoming = rx.recv(), if channel_open => match incoming {
                            Some(job) => job,
                            None => {
                                channel_open = false;
                                continue;
                            }
                        },
                        _ = tokio::time::sleep_until(due) => {
                            match retry_queue.pop_front() {
                                Some((job, _)) => job,
                                None => continue,
                            }
                        }
                    }
                } else if channel_open {
                    match rx.recv().await {
                        Some(job) => job,
                        None => break,
                    }
                } else if let Some((job, due)) = retry_queue.pop_front() {
                    tokio::time::sleep_until(due).await;
                    job
                } else {
                    break;
                };

                if retry_queue
                    .iter()
                    .any(|(parked, _)| parked.session_key == job.session_key)
                {
                    blocked
                        .entry(job.session_key.clone())
                        .or_default()
                        .push_back(job);
                    continue;
                }

                loop {
                    let session_key = job.session_key.clone();
                    let backend_for_write = std::sync::Arc::clone(&backend);
                    let outcome = tokio::task::spawn_blocking(move || {
                        let PersistJob {
                            session_key,
                            rows,
                            attempts,
                        } = job;
                        for (idx, row) in rows.iter().enumerate() {
                            let result = if row.hidden {
                                backend_for_write.append_hidden(&session_key, &row.msg)
                            } else {
                                backend_for_write.append(&session_key, &row.msg)
                            };
                            if let Err(e) = result {
                                let leftover = rows[idx..].to_vec();
                                return Some((
                                    PersistJob {
                                        session_key,
                                        rows: leftover,
                                        attempts: attempts.saturating_add(1),
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
                            session_pending_dec(&session_key);
                            match pop_blocked_job(&mut blocked, &session_key) {
                                Some(next) => {
                                    job = next;
                                    continue;
                                }
                                None => break,
                            }
                        }
                        Ok(Some((leftover_job, err))) => {
                            if leftover_job.attempts >= MAX_PERSIST_RETRIES {
                                tracing::error!(
                                    target: "ws_desktop_persist",
                                    error = %err,
                                    session_key = %leftover_job.session_key,
                                    dropped_rows = leftover_job.rows.len(),
                                    attempts = leftover_job.attempts,
                                    "session persist append failed repeatedly; giving up on batch"
                                );
                                PERSIST_PENDING.fetch_sub(1, Ordering::SeqCst);
                                session_pending_dec(&session_key);
                                match pop_blocked_job(&mut blocked, &session_key) {
                                    Some(next) => {
                                        job = next;
                                        continue;
                                    }
                                    None => break,
                                }
                            } else {
                                let backoff_ms =
                                    (500u64 << leftover_job.attempts.min(6)).min(30_000);
                                tracing::warn!(
                                    target: "ws_desktop_persist",
                                    error = %err,
                                    session_key = %leftover_job.session_key,
                                    pending = leftover_job.rows.len(),
                                    attempts = leftover_job.attempts,
                                    backoff_ms,
                                    "session persist append failed; retrying in background without blocking other sessions"
                                );
                                let due = tokio::time::Instant::now()
                                    + std::time::Duration::from_millis(backoff_ms);
                                retry_queue.push_back((leftover_job, due));
                                break;
                            }
                        }
                        Err(join_err) => {
                            tracing::warn!(
                                target: "ws_desktop_persist",
                                error = %join_err,
                                "session persist worker join error; dropping batch"
                            );
                            PERSIST_PENDING.fetch_sub(1, Ordering::SeqCst);
                            session_pending_dec(&session_key);
                            match pop_blocked_job(&mut blocked, &session_key) {
                                Some(next) => {
                                    job = next;
                                    continue;
                                }
                                None => break,
                            }
                        }
                    }
                }
            }
        });
        tx
    })
}

use crate::util::describe_panic;

async fn enqueue_persist(
    state: &AppState,
    session_key: &str,
    rows: Vec<crate::providers::ChatMessage>,
) -> bool {
    enqueue_persist_rows(
        state,
        session_key,
        rows.into_iter()
            .map(|msg| PersistRow { msg, hidden: false })
            .collect(),
    )
    .await
}

async fn enqueue_persist_rows(
    state: &AppState,
    session_key: &str,
    rows: Vec<PersistRow>,
) -> bool {
    if rows.is_empty() {
        return false;
    }
    let Some(backend) = state.session_backend.as_ref() else {
        return false;
    };
    let job = PersistJob {
        session_key: session_key.to_string(),
        rows,
        attempts: 0,
    };
    let sender = persist_sender(backend);
    match sender.try_send(job) {
        Ok(()) => {
            PERSIST_PENDING.fetch_add(1, Ordering::SeqCst);
            session_pending_inc(session_key);
            false
        }
        Err(mpsc::error::TrySendError::Full(job)) => {
            tracing::warn!(
                target: "ws_desktop_persist",
                pending = PERSIST_PENDING.load(Ordering::SeqCst),
                capacity = PERSIST_QUEUE_CAPACITY,
                session_key,
                "session persist backlog full; applying backpressure instead of dropping"
            );
            let _ = state.event_tx.send(serde_json::json!({
                "type": "persist_lag",
                "sessionKey": session_key,
                "pending": PERSIST_PENDING.load(Ordering::SeqCst),
            }));
            if sender.send(job).await.is_ok() {
                PERSIST_PENDING.fetch_add(1, Ordering::SeqCst);
                session_pending_inc(session_key);
            } else {
                tracing::error!(
                    target: "ws_desktop_persist",
                    session_key,
                    "session persist worker channel closed during backpressure; batch lost"
                );
            }
            true
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            tracing::error!(
                target: "ws_desktop_persist",
                session_key,
                "session persist worker channel closed; batch lost"
            );
            true
        }
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
            self.absorb_pending_text();
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

    fn finish_for_interrupt(mut self) -> Vec<crate::providers::ChatMessage> {
        self.absorb_pending_text_into_segment();
        let dangling: Vec<String> = self
            .assistant_segment
            .iter()
            .filter(|v| v.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
            .filter_map(|v| v.get("id").and_then(|i| i.as_str()).map(String::from))
            .collect();
        self.finalize_assistant_segment();
        for tool_use_id in &dangling {
            self.on_tool_result(tool_use_id, "[interrupted by user]".to_string(), true);
        }
        self.out
    }
}

fn assistant_object_has_visible_text(obj: &serde_json::Map<String, serde_json::Value>) -> bool {
    match obj.get("content") {
        Some(serde_json::Value::String(s)) => !s.trim().is_empty(),
        Some(serde_json::Value::Array(parts)) => parts.iter().any(|p| match p {
            serde_json::Value::String(s) => !s.trim().is_empty(),
            serde_json::Value::Object(o) => o
                .get("text")
                .and_then(|t| t.as_str())
                .is_some_and(|t| !t.trim().is_empty()),
            _ => false,
        }),
        _ => false,
    }
}

fn assistant_content_has_visible_text(content: &str) -> bool {
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(serde_json::Value::Array(blocks)) => blocks.iter().any(|b| {
            b.get("type").and_then(|t| t.as_str()) == Some("text")
                && b.get("text")
                    .and_then(|t| t.as_str())
                    .is_some_and(|t| !t.trim().is_empty())
        }),
        Ok(serde_json::Value::Object(obj)) => assistant_object_has_visible_text(&obj),
        _ => !content.trim().is_empty(),
    }
}

fn sanitize_attachment_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('_');
    if trimmed.is_empty() {
        "attachment".to_string()
    } else {
        trimmed.chars().take(80).collect()
    }
}

fn attachment_ext_from_mime(mime: &str) -> Option<&'static str> {
    match mime {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/webp" => Some("webp"),
        "image/gif" => Some("gif"),
        "image/bmp" => Some("bmp"),
        "text/plain" => Some("txt"),
        "application/pdf" => Some("pdf"),
        _ => None,
    }
}

fn enrich_content_with_attachments(
    content: &str,
    attachments: &[serde_json::Value],
    workspace: &std::path::Path,
) -> String {
    use base64::Engine;

    let mut out = content.to_string();
    let dir = workspace.join(".sen").join("attachments");
    let mut dir_ready = false;

    for att in attachments {
        let ty = att.get("type").and_then(|v| v.as_str()).unwrap_or("file");
        let name = att.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let path = att
            .get("path")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let data = att
            .get("data")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());

        if let Some(p) = path {
            if ty == "image" {
                out.push_str(&format!("\n\n[IMAGE:{p}]"));
            } else {
                out.push_str(&format!("\n\n[Attached file: {p}]"));
            }
            continue;
        }

        let Some(data_url) = data else { continue };
        let (mime, b64) = match data_url.strip_prefix("data:") {
            Some(rest) => match rest.find(',') {
                Some(i) => (
                    rest[..i].split(';').next().unwrap_or("").to_string(),
                    &rest[i + 1..],
                ),
                None => continue,
            },
            None => (String::new(), data_url),
        };
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) else {
            tracing::warn!(
                target: "ws_desktop_attachments",
                name,
                "failed to decode attachment base64 payload; skipping"
            );
            continue;
        };
        if !dir_ready {
            if let Err(e) = std::fs::create_dir_all(&dir) {
                tracing::warn!(
                    target: "ws_desktop_attachments",
                    error = %e,
                    "failed to create attachments directory; skipping attachment persistence"
                );
                return out;
            }
            dir_ready = true;
        }
        let mut file_name = sanitize_attachment_name(name);
        if !file_name.contains('.') {
            if let Some(ext) = attachment_ext_from_mime(&mime) {
                file_name = format!("{file_name}.{ext}");
            }
        }
        let unique = format!(
            "{}-{}",
            &uuid::Uuid::new_v4().to_string()[..8],
            file_name
        );
        let file_path = dir.join(unique);
        if let Err(e) = std::fs::write(&file_path, &bytes) {
            tracing::warn!(
                target: "ws_desktop_attachments",
                error = %e,
                "failed to write attachment to disk; skipping"
            );
            continue;
        }
        let display = file_path.display();
        if ty == "image" {
            out.push_str(&format!("\n\n[IMAGE:{display}]"));
        } else {
            out.push_str(&format!("\n\n[Attached file: {display}]"));
        }
    }

    out
}

fn make_error_history_row(message: &str, code: &str) -> Option<crate::providers::ChatMessage> {
    let safe_message = crate::providers::sanitize_api_error(message);
    serde_json::to_string(&serde_json::json!([{
        "type": "error",
        "message": safe_message,
        "code": code,
        "timestamp_ms": DesktopSqlitePersist::wallclock_ms_unix(),
    }]))
    .ok()
    .map(crate::providers::ChatMessage::assistant)
}

static TOOL_USE_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_tool_use_id() -> String {
    let n = TOOL_USE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("toolu_{n:x}")
}

async fn set_coding_mode_scoped(
    agent: &mut crate::agent::Agent,
    session_id: &str,
    connection_id: &str,
    mode: crate::agent::coding_mode::CodingMode,
) {
    let ctx = crate::session::SessionContext {
        session_id: session_id.to_string(),
        workspace_key: crate::session::workspace_key_from_path(
            agent.current_workspace_dir(),
            session_id,
        ),
        title: session_id.to_string(),
        workspace_dir: agent
            .current_workspace_dir()
            .to_string_lossy()
            .into_owned(),
        connection_id: Some(connection_id.to_string()),
    };
    crate::session::scope_session_context(ctx, async {
        agent.set_coding_mode(mode);
    })
    .await;
}

async fn run_turn(
    state: &AppState,
    agent: &mut crate::agent::Agent,
    outbound: &OutboundSender,
    session_id: &str,
    session_key: &str,
    connection_id: &str,
    content: &str,
) -> Option<String> {
    use crate::agent::TurnEvent;
    let mut pending_ask_request_id: Option<String> = None;

    let workspace_key = crate::session::workspace_key_from_path(
        agent.current_workspace_dir(),
        session_id,
    );

    let _run_guard = state.session_run_state.guard(session_id.to_string());
    if !_run_guard.was_inserted() {
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
        return None;
    }

    let _rewind_guard =
        crate::gateway::api::core::acquire_rewind_lock(session_id).await;

    let feed_for_pump = crate::session::register_turn_feed(
        session_id,
        agent.cancel_token(),
        agent.cancel_signal_handle(),
    );
    let _turn_feed_guard = crate::session::TurnFeedGuard::new(
        session_id.to_string(),
        std::sync::Arc::clone(&feed_for_pump),
    );

    static NEXT_TURN_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let turn_seq =
        NEXT_TURN_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let (tee_tx, mut tee_rx) = mpsc::channel::<OutboundFrame>(1024);
    let real_outbound = outbound.clone();
    let _tee_pump = crate::runtime::spawn_supervised("ws_desktop.turn_tee", async move {
        while let Some(frame) = tee_rx.recv().await {
            let frame = match frame {
                OutboundFrame::Text(text) => {
                    let stamped = match serde_json::from_str::<serde_json::Value>(&text) {
                        Ok(mut value) => match value.as_object_mut() {
                            Some(obj) => {
                                obj.insert(
                                    "turnSeq".to_string(),
                                    serde_json::json!(turn_seq),
                                );
                                value.to_string()
                            }
                            None => text,
                        },
                        Err(_) => text,
                    };
                    feed_for_pump.publish(&stamped);
                    OutboundFrame::Text(stamped)
                }
                OutboundFrame::ContentDelta(t, tseq) => {
                    feed_for_pump.publish(
                        &serde_json::json!({ "type": "content_delta", "text": t, "turnSeq": tseq })
                            .to_string(),
                    );
                    OutboundFrame::ContentDelta(t, tseq)
                }
                OutboundFrame::Thinking(t, tseq) => {
                    feed_for_pump.publish(
                        &serde_json::json!({ "type": "thinking", "text": t, "turnSeq": tseq })
                            .to_string(),
                    );
                    OutboundFrame::Thinking(t, tseq)
                }
                other => other,
            };
            let _ = real_outbound.send(frame).await;
        }
    });
    let outbound: &OutboundSender = &tee_tx;

    let _ = send_json(
        outbound,
        &serde_json::json!({
            "type": "status",
            "state": "thinking",
        }),
    )
    .await;

    let (event_tx, mut event_rx) = mpsc::channel::<TurnEvent>(1024);
    let content_owned = content.to_string();
    let mut text_block_open = false;
    let mut current_tool_use_id: Option<String> = None;
    let mut tool_id_pairer = crate::session::FallbackToolIdPairer::default();
    let mut last_tool_use_id_for_name: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut streaming_tool_args: std::collections::HashMap<u32, (String, usize, bool)> =
        std::collections::HashMap::new();
    let mut accumulated_text = String::new();
    let mut streamed_turn_error: Option<String> = None;
    let mut thinking_forwarded = false;
    let started = std::time::Instant::now();

    let user_message_index: i64 = if let Some(ref backend) = state.session_backend {
        let drained = wait_session_persist_drained(
            session_key,
            std::time::Duration::from_secs(30),
        )
        .await;
        if !drained {
            tracing::warn!(
                target: "ws_desktop_persist",
                session_key,
                "session persist queue did not drain before turn start; \
                 disabling edit-batch rewind attribution for this turn to avoid \
                 reverting the wrong user message's files"
            );
            -1
        } else {
            let backend_arc = std::sync::Arc::clone(backend);
            let session_key_owned = session_key.to_string();
            let persisted_total: Option<u64> = match tokio::task::spawn_blocking(move || {
                backend_arc.count_user_messages(&session_key_owned) as u64
            })
            .await
            {
                Ok(total) => Some(total),
                Err(e) => {
                    tracing::warn!(
                        target: "ws_desktop_persist",
                        error = %e,
                        "failed to compute user_message_index; disabling rewind attribution for this turn"
                    );
                    None
                }
            };
            match persisted_total {
                Some(total) => {
                    let persisted_before = total.saturating_sub(1);
                    let marker = agent.gateway_sync_marker();
                    if persisted_before > marker {
                        tracing::info!(
                            target: "ws_desktop_persist",
                            session_key,
                            persisted_before,
                            marker,
                            "session transcript advanced by another connection; \
                             re-seeding this connection's agent history from the persisted tail"
                        );
                        const SEED_HISTORY_WINDOW: usize = 400;
                        let backend_reseed = std::sync::Arc::clone(backend);
                        let key_reseed = session_key.to_string();
                        match tokio::task::spawn_blocking(move || {
                            backend_reseed.load_tail(&key_reseed, SEED_HISTORY_WINDOW)
                        })
                        .await
                        {
                            Ok(mut tail) => {
                                if let Some(last) = tail.last() {
                                    if last.role == "user" && last.content == content {
                                        tail.pop();
                                    }
                                }
                                agent.clear_history();
                                if !tail.is_empty() {
                                    agent.seed_history(&tail);
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    target: "ws_desktop_persist",
                                    error = %e,
                                    "failed to re-seed agent history after cross-connection drift"
                                );
                            }
                        }
                    }
                    agent.set_gateway_sync_marker(total);
                    #[allow(clippy::cast_possible_wrap)]
                    {
                        (total.saturating_sub(1)) as i64
                    }
                }
                None => -1,
            }
        }
    } else {
        -1
    };
    let mut recorded_batches: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    let sqlite_persist = std::sync::Arc::new(std::sync::Mutex::new(DesktopSqlitePersist::default()));
    let sqlite_persist_forward = std::sync::Arc::clone(&sqlite_persist);

    let session_is_auto = crate::services::try_get_services()
        .map(|svc| svc.is_session_auto_coding_mode(&session_key))
        .unwrap_or(false);
    let turn_coding_mode = if session_is_auto {
        let resolved = agent.resolve_auto_coding_mode(content).await;
        agent.set_coding_mode(resolved);
        let derived =
            crate::gateway::desktop::routes::derive_permission_from_coding(&resolved);
        desktop_runtime_state().set_session_permission_mode(&session_key, derived);
        let _ = send_json(
            outbound,
            &serde_json::json!({
                "type": "system_notification",
                "subtype": "coding_mode_auto_resolved",
                "message": format!("Auto mode \u{2192} {}", resolved.label()),
                "data": {
                    "mode": resolved.display_name(),
                    "label": resolved.label(),
                    "permissionMode": derived,
                    "auto": true,
                },
            }),
        )
        .await;
        resolved
    } else {
        agent.current_coding_mode().unwrap_or_else(|| {
            crate::services::try_get_services()
                .map(|svc| svc.resolve_coding_mode_for(Some(&session_key)))
                .unwrap_or_default()
        })
    };
    let coding_mode_label = Some(turn_coding_mode.display_name().to_string());

    let cost_tracking_ctx = state.cost_tracker.as_ref().map(|tracker| {
        let prices = {
            let cfg = state.live_config.load();
            std::sync::Arc::new(crate::cost::pricing::effective_model_prices(&cfg))
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
                .with_turn_class(crate::evolution::TurnClass::Main)
                .with_user_message(content_owned.clone());
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
        connection_id: Some(connection_id.to_string()),
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
        let mode_scoped =
            crate::agent::coding_mode::scope_coding_mode(turn_coding_mode, scoped);
        let perm_scoped = crate::gateway::ws::desktop::scope_permission_mode(
            desktop_runtime_state().permission_mode_for(&session_key),
            mode_scoped,
        );
        let result =
            crate::session::scope_session_context(session_ctx, perm_scoped).await;
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
        let mut approval_bus_rx = crate::gateway::ws::gateway_approval_bus().subscribe();
        loop {
            let event = tokio::select! {
                biased;
                turn_event = event_rx.recv() => match turn_event {
                    Some(ev) => ev,
                    None => break,
                },
                bus_event = approval_bus_rx.recv() => {
                    match bus_event {
                        Ok(evt) => {
                            if let crate::session::SessionEventKind::ApprovalRequested {
                                id: request_id,
                                tool_name,
                                arguments,
                                ..
                            } = &evt.kind
                            {
                                let belongs_here = crate::approval::pending_gateway_approval_session(
                                    request_id,
                                )
                                .is_some_and(|owner| owner.as_deref() == Some(session_id));
                                if belongs_here {
                                    let _ = send_json(
                                        outbound,
                                        &serde_json::json!({
                                            "type": "permission_request",
                                            "requestId": request_id,
                                            "toolName": tool_name,
                                            "input": arguments,
                                            "description": arguments
                                                .get("reason")
                                                .and_then(|v| v.as_str()),
                                        }),
                                    )
                                    .await;
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                            tracing::warn!(
                                target: "gateway.approval",
                                skipped,
                                session_id = %session_id,
                                "approval bus lagged; replaying pending approval requests for this session"
                            );
                            for replay in crate::approval::pending_replays_for_session(session_id) {
                                let _ = send_json(outbound, &replay).await;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {}
                    }
                    continue;
                }
            };
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
                    let _ = outbound.send(OutboundFrame::ContentDelta(delta, turn_seq)).await;
                }
                TurnEvent::StreamReset => {
                    accumulated_text.clear();
                    streaming_tool_args.clear();
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
                    if thinking_forwarded || !delta.trim().is_empty() {
                        thinking_forwarded = true;
                        if let Ok(mut pg) = sqlite_persist_forward.lock() {
                            pg.on_thinking(&delta);
                        }
                        let _ = outbound.send(OutboundFrame::Thinking(delta, turn_seq)).await;
                    }
                }
                TurnEvent::ToolCall {
                    name,
                    args,
                    tool_call_id,
                } => {
                    text_block_open = false;
                    streaming_tool_args.clear();
                    let id = tool_call_id
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(next_tool_use_id);
                    current_tool_use_id = Some(id.clone());
                    tool_id_pairer.push_call_id(&name, id.clone());
                    last_tool_use_id_for_name.insert(name.clone(), id.clone());
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
                TurnEvent::ToolArgsDelta {
                    call_index,
                    name,
                    args_delta,
                    args_total_len,
                } => {
                    const MAX_STREAMING_ARGS_BUFFER_BYTES: usize = 512 * 1024;
                    const STREAMING_ARGS_SEND_STEP_BYTES: usize = 1024;
                    const STREAMING_ARGS_TAIL_HOLDBACK_BYTES: usize = 1024;
                    let (buffer, last_sent_len, poisoned) =
                        streaming_tool_args.entry(call_index).or_default();
                    if *poisoned {
                        continue;
                    }
                    if buffer.len() + args_delta.len() != args_total_len as usize {
                        *poisoned = true;
                        buffer.clear();
                        *last_sent_len = 0;
                        tracing::debug!(
                            target: "gateway.ws",
                            call_index,
                            tool = %name,
                            "tool args delta gap detected (dropped event under backpressure); live preview disabled for this call"
                        );
                        continue;
                    }
                    if buffer.len() + args_delta.len() <= MAX_STREAMING_ARGS_BUFFER_BYTES {
                        buffer.push_str(&args_delta);
                        let first_send = *last_sent_len == 0;
                        if first_send
                            || buffer.len().saturating_sub(*last_sent_len)
                                >= STREAMING_ARGS_SEND_STEP_BYTES
                        {
                            *last_sent_len = buffer.len();
                            let safe_snapshot =
                                crate::services::governance::credential_vault::redact_for_audit_optional(
                                    buffer,
                                );
                            let mut visible_end = safe_snapshot
                                .len()
                                .saturating_sub(STREAMING_ARGS_TAIL_HOLDBACK_BYTES);
                            while visible_end > 0 && !safe_snapshot.is_char_boundary(visible_end)
                            {
                                visible_end -= 1;
                            }
                            if visible_end > 0 {
                                let _ = send_json(
                                    outbound,
                                    &serde_json::json!({
                                        "type": "tool_use_args_delta",
                                        "toolName": name,
                                        "callIndex": call_index,
                                        "argsSnapshot": &safe_snapshot[..visible_end],
                                        "sessionId": session_id,
                                    }),
                                )
                                .await;
                            }
                        }
                    }
                }
                TurnEvent::ToolResult {
                    name,
                    output,
                    success,
                    tool_call_id,
                } => {
                    let explicit_id = tool_call_id.filter(|s| !s.is_empty());
                    let id = match explicit_id {
                        Some(id) => {
                            tool_id_pairer.remove_id(&name, &id);
                            id
                        }
                        None => tool_id_pairer
                            .pop_result_id(&name)
                            .or_else(|| current_tool_use_id.clone())
                            .unwrap_or_else(next_tool_use_id),
                    };
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
                    if enqueue_persist(state, session_key, flush_rows).await {
                        let _ = send_json(
                            outbound,
                            &serde_json::json!({
                                "type": "persist_lag",
                                "sessionId": session_id,
                            }),
                        )
                        .await;
                    }
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
                    if enqueue_persist(state, session_key, flush_rows).await {
                        let _ = send_json(
                            outbound,
                            &serde_json::json!({
                                "type": "persist_lag",
                                "sessionId": session_id,
                            }),
                        )
                        .await;
                    }
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
                        if user_message_index < 0 {
                            tracing::debug!(
                                target: "rewind",
                                session_key,
                                batch = %batch,
                                "skipping edit-batch attribution: user_message_index unresolved for this turn"
                            );
                        } else if !batch.is_empty() && recorded_batches.insert(batch.clone()) {
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
                    let state = match action.as_str() {
                        "thinking"
                        | "compressing"
                        | "preparing"
                        | "waiting_model"
                        | "model_override" => "thinking",
                        _ => "tool_executing",
                    };
                    let _ = send_json(
                        outbound,
                        &serde_json::json!({
                            "type": "status",
                            "state": state,
                            "verb": action,
                            "detail": detail,
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
                    tokens_before,
                    tokens_after,
                } => {
                    let _ = send_json(
                        outbound,
                        &serde_json::json!({
                            "type": "context_compressed",
                            "tokens_before": tokens_before,
                            "tokens_after": tokens_after,
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

                    if matches!(
                        tool_name.as_str(),
                        "ask_question" | "ask_user" | "AskQuestion" | "AskUserQuestion"
                    ) {
                        pending_ask_request_id = Some(request_id.clone());
                    }

                    let tool_use_id = tool_id_pairer
                        .peek_last(&tool_name)
                        .or_else(|| current_tool_use_id.clone())
                        .or_else(|| last_tool_use_id_for_name.get(&tool_name).cloned());
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
                    let hooks_for_notify = std::sync::Arc::clone(&state.hooks);
                    let message_for_notify = message.clone();
                    crate::runtime::spawn_supervised("hooks.notification", async move {
                        hooks_for_notify
                            .fire_notification("error", &message_for_notify)
                            .await;
                    });
                    let code =
                        crate::agent::error_classify::classify_turn_error_code(&message);
                    send_error(outbound, &message, code).await;
                    streamed_turn_error = Some(message);
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

    let buddy_cfg = state.live_config.load().buddy.clone();
    if let Some((event, greeting)) = crate::buddy::lifecycle_event(&buddy_cfg, "working") {
        let _ = send_json(
            outbound,
            &serde_json::json!({
                "type": "buddy_event",
                "sessionId": session_id,
                "event": event,
                "greeting": greeting,
                "showNotifications": buddy_cfg.show_notifications,
            }),
        )
        .await;
    }

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

    let mut turn_panicked = false;
    let turn_result: Result<String, String> = match turn_caught {
        Ok(Ok(final_text)) => {
            if forward_panicked {
                tracing::warn!(
                    target: "ws_desktop_turn",
                    session_id,
                    "turn succeeded but event forwarding panicked; delivering final result anyway"
                );
            }
            Ok(final_text)
        }
        Ok(Err(err)) => Err(format!("{err}")),
        Err(panic) => {
            let detail = describe_panic(&*panic);
            tracing::error!(
                target: "ws_desktop_turn",
                session_id,
                "turn execution panicked (recovered): {detail}",
            );
            if !accumulated_text.trim().is_empty() {
                tracing::warn!(
                    target: "ws_desktop_turn",
                    session_id,
                    delivered_bytes = accumulated_text.len(),
                    "turn panicked after partially streaming content; finalizing with already-delivered text instead of failing the whole turn"
                );
                turn_panicked = true;
                Ok(accumulated_text.clone())
            } else {
                Err(format!("internal error recovered: {detail}"))
            }
        }
    };

    match turn_result {
        Ok(final_text) => {
            if !turn_panicked {
                let config_snapshot = state.config.lock().clone();
                let hooks = crate::agent::profile::runtime_hooks::LearningHooks::from_config(
                    &config_snapshot,
                );
                hooks.record_turn_heuristics(content, &final_text, &[]);
            }
            if state.session_backend.is_some() {
                let recorder = sqlite_persist
                    .lock()
                    .ok()
                    .map(|mut g| std::mem::take(&mut *g))
                    .unwrap_or_default();
                let mut rows = if turn_panicked {
                    recorder.finish_for_interrupt()
                } else {
                    recorder.finish()
                };
                if !final_text.trim().is_empty() && (!turn_panicked || rows.is_empty()) {
                    let has_assistant_text = rows
                        .iter()
                        .any(|m| m.role == "assistant" && assistant_content_has_visible_text(&m.content));
                    if !has_assistant_text {
                        let merged_into_blocks = rows.iter_mut().rev().find(|m| m.role == "assistant").is_some_and(|last| {
                            match serde_json::from_str::<serde_json::Value>(&last.content) {
                                Ok(serde_json::Value::Array(mut blocks)) => {
                                    blocks.push(json!({ "type": "text", "text": final_text }));
                                    match serde_json::to_string(&blocks) {
                                        Ok(serialized) => {
                                            last.content = serialized;
                                            true
                                        }
                                        Err(_) => false,
                                    }
                                }
                                Ok(serde_json::Value::Object(mut obj)) => {
                                    if assistant_object_has_visible_text(&obj) {
                                        false
                                    } else {
                                        obj.insert(
                                            "content".to_string(),
                                            serde_json::Value::String(final_text.clone()),
                                        );
                                        match serde_json::to_string(&serde_json::Value::Object(obj)) {
                                            Ok(serialized) => {
                                                last.content = serialized;
                                                true
                                            }
                                            Err(_) => false,
                                        }
                                    }
                                }
                                _ => false,
                            }
                        });
                        if !merged_into_blocks {
                            rows.push(crate::providers::ChatMessage::assistant(final_text.clone()));
                        }
                    }
                }
                if turn_panicked {
                    if let Some(row) = make_error_history_row(
                        "An internal error occurred while generating this turn. Content produced so far has been kept; the rest may be missing.",
                        "TURN_PANIC_PARTIAL",
                    ) {
                        rows.push(row);
                    }
                }
                enqueue_persist(state, session_key, rows).await;
                let _ = wait_session_persist_drained(
                    session_key,
                    std::time::Duration::from_secs(1),
                )
                .await;
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
            if turn_panicked {
                send_error(
                    outbound,
                    "An internal error occurred while generating this turn. Content produced so far has been kept; the rest may be missing.",
                    "TURN_PANIC_PARTIAL",
                )
                .await;
            }
            if let Some((event, greeting)) = crate::buddy::lifecycle_event(&buddy_cfg, "completed") {
                let _ = send_json(
                    outbound,
                    &serde_json::json!({
                        "type": "buddy_event",
                        "sessionId": session_id,
                        "event": event,
                        "greeting": greeting,
                        "showNotifications": buddy_cfg.show_notifications,
                    }),
                )
                .await;
            }
        }
        Err(err) => {
            let msg = format!("{err}");
            let code = crate::agent::error_classify::classify_turn_error_code(&msg);
            if state.session_backend.is_some() {
                let recorder = sqlite_persist
                    .lock()
                    .ok()
                    .map(|mut g| std::mem::take(&mut *g))
                    .unwrap_or_default();
                let mut rows = recorder.finish_for_interrupt();
                if code != "CANCELLED" {
                    if let Some(row) = make_error_history_row(&msg, code) {
                        rows.push(row);
                    }
                }
                enqueue_persist(state, session_key, rows).await;
            }
            let already_streamed = streamed_turn_error
                .as_deref()
                .map(str::trim)
                .is_some_and(|prev| {
                    let now = msg.trim();
                    prev == now || prev.contains(now) || now.contains(prev)
                });
            if !already_streamed {
                send_error(outbound, &msg, code).await;
            }
            if let Some((event, greeting)) = crate::buddy::lifecycle_event(&buddy_cfg, "error") {
                let _ = send_json(
                    outbound,
                    &serde_json::json!({
                        "type": "buddy_event",
                        "sessionId": session_id,
                        "event": event,
                        "greeting": greeting,
                        "showNotifications": buddy_cfg.show_notifications,
                    }),
                )
                .await;
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
            let summary = first_line(title_source_from_turn_content(content))
                .chars()
                .take(60)
                .collect::<String>();
            if !summary.is_empty() {
                let session_key_set = session_key.to_string();
                let backend_set = backend.clone();
                let summary_for_set = summary.clone();
                let persisted = match tokio::task::spawn_blocking(move || {
                    backend_set.set_session_name(&session_key_set, &summary_for_set)
                })
                .await
                {
                    Ok(Ok(())) => true,
                    Ok(Err(e)) => {
                        tracing::warn!(
                            target: "ws_desktop_persist",
                            error = %e,
                            "failed to persist auto-generated session title"
                        );
                        false
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "ws_desktop_persist",
                            error = %e,
                            "session title persist task panicked"
                        );
                        false
                    }
                };
                if persisted {
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

    pending_ask_request_id
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("").trim()
}

fn title_source_from_turn_content(content: &str) -> &str {
    let trimmed = content.trim_start();
    if !(trimmed.starts_with('[') && trimmed.contains("EXCLUSIVE TASK FOR THIS TURN]")) {
        return content;
    }
    if let Some(idx) = trimmed.find("\nBrief:") {
        let after = &trimmed[idx + "\nBrief:".len()..];
        for line in after.lines() {
            let line = line.trim();
            if !line.is_empty() {
                return line;
            }
        }
    }
    content
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
