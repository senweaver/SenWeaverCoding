// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Desktop-flavoured WebSocket chat handler.
//!
//! Mounts at `/ws/{session_id}` and speaks the cc-haha desktop wire
//! protocol expected by the React frontend in `desktop/src/`:
//!
//! Client → Server (`ClientMessage`):
//!  * `prewarm_session`
//!  * `user_message { content, attachments? }`
//!  * `permission_response { requestId, allowed, rule?, updatedInput? }`
//!  * `computer_use_permission_response { requestId, response }`
//!  * `set_permission_mode { mode }`        (legacy; derived from coding_mode)
//!  * `set_coding_mode { mode }`            (12-mode programming workflow selector)
//!  * `set_runtime_config { provider?, model?, ... }`
//!  * `stop_generation`
//!  * `ping`
//!
//! Server → Client (`ServerMessage`):
//!  * `connected { sessionId }`
//!  * `content_start { blockType, toolName?, toolUseId? }`
//!  * `content_delta { text? | toolInput? }`
//!  * `tool_use_complete { toolName, toolUseId, input }`
//!  * `tool_result { toolUseId, content, isError }`
//!  * `permission_request { requestId, toolName, input, description? }`
//!  * `message_complete { usage }`
//!  * `thinking { text }`
//!  * `status { state, verb?, elapsed?, tokens? }`
//!  * `error { message, code, retryable? }`
//!  * `system_notification { subtype, message?, data? }`
//!  * `pong`
//!  * `session_title_updated { sessionId, title }`
//!
//! The legacy `/ws/chat?session_id=...` route in [`super::ws`] continues
//! to serve clients that speak the older `chunk / done` protocol — the
//! two routes co-exist because the agent loop is stateless across
//! connections.

use super::AppState;
use axum::{
    extract::{
        Path, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::HeaderMap,
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::mpsc;

const GW_SESSION_PREFIX: &str = "gw_";

pub async fn handle_ws_desktop(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if state.pairing.require_pairing() {
        let token = super::ws::extract_ws_token(&headers, None).unwrap_or("");
        if !state.pairing.is_authenticated(token) {
            return (
                axum::http::StatusCode::UNAUTHORIZED,
                "Unauthorized — provide Authorization header or pairing token",
            )
                .into_response();
        }
    }

    ws.on_upgrade(move |socket| handle_socket(socket, state, session_id))
        .into_response()
}

async fn handle_socket(socket: WebSocket, state: AppState, session_id: String) {
    let (mut sender, mut receiver) = socket.split();

    let session_key = format!("{GW_SESSION_PREFIX}{session_id}");

    let _ = send_json(
        &mut sender,
        &serde_json::json!({
            "type": "connected",
            "sessionId": session_id,
        }),
    )
    .await;

    let config = {
        let mut cfg = state.config.lock();
        if super::desktop_routes::sanitize_active_profile_in_place(&mut cfg) {
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
            send_error(&mut sender, &format!("agent init failed: {e}"), "AGENT_INIT_FAILED").await;
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

    enum InboundMsg {
        Json(serde_json::Value),
        Pong(Vec<u8>),
    }

    let (inbound_tx, mut inbound_rx) = tokio::sync::mpsc::unbounded_channel::<InboundMsg>();

    let inbound_tx_lsp = inbound_tx.clone();
    let inbound_tx_replay = inbound_tx.clone();

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
                            if inbound_tx.send(InboundMsg::Json(v)).is_err() {
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
                                let evt = crate::session::SessionEvent::new(
                                    crate::session::SessionEventKind::ApprovalResponded {
                                        id: request_id.to_string(),
                                        decision: if allowed { "yes" } else { "no" }
                                            .to_string(),
                                        responder: Some("desktop".to_string()),
                                        updated_input,
                                    },
                                );
                                let _ = super::ws::approval_sender_for_desktop().send(evt);
                                tracing::debug!(
                                    target: "ws_desktop.gate",
                                    request_id = %request_id,
                                    allowed,
                                    "desktop approval frame fast-pathed to bus"
                                );
                            }

                            continue;
                        }
                        _ => {}
                    }
                    if inbound_tx.send(InboundMsg::Json(parsed)).is_err() {
                        break;
                    }
                }
                Ok(Message::Ping(payload)) => {

                    if inbound_tx
                        .send(InboundMsg::Pong(payload.to_vec()))
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
                            if tx.send(InboundMsg::Json(wrapped)).is_err() {
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

    {
        let svc = state.lsp.service();
        let cached = svc.get_all_diagnostics().await;
        for (path, diags) in cached {
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
                let _ = inbound_tx_replay.send(InboundMsg::Json(wrapped));
            }
        }
    }

    while let Some(inbound) = inbound_rx.recv().await {
        let parsed = match inbound {
            InboundMsg::Pong(payload) => {

                let _ = sender.send(Message::Pong(payload.into())).await;
                continue;
            }
            InboundMsg::Json(v) => v,
        };

        if parsed.get("type").and_then(|v| v.as_str()) == Some("__invalid_json__") {
            let raw = parsed
                .get("raw")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            send_error(
                &mut sender,
                &format!("invalid JSON: {} (...)", raw.chars().take(120).collect::<String>()),
                "INVALID_JSON",
            )
            .await;
            continue;
        }

        if parsed.get("type").and_then(|v| v.as_str()) == Some("__lsp_forward__") {
            if let Some(payload) = parsed.get("payload") {
                let _ = send_json(&mut sender, payload).await;
            }
            continue;
        }

        let msg_type = parsed.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match msg_type {
            "ping" => {
                let _ = send_json(&mut sender, &serde_json::json!({ "type": "pong" })).await;
            }
            "prewarm_session" => {

                let _ = send_json(
                    &mut sender,
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
                    &mut sender,
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
                    &mut sender,
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
                if let Some(parsed_mode) =
                    crate::agent::coding_mode::CodingMode::from_str_loose(mode_str)
                {
                    if let Some(svc) = crate::services::try_get_services() {
                        *svc.coding_mode.write() = parsed_mode;
                    }
                    agent.set_coding_mode(parsed_mode);
                    let derived = super::desktop_routes::derive_permission_from_coding(&parsed_mode);
                    desktop_runtime_state().set_permission_mode(derived);
                    let _ = send_json(
                        &mut sender,
                        &serde_json::json!({
                            "type": "system_notification",
                            "subtype": "coding_mode_updated",
                            "message": format!("Coding mode: {}", parsed_mode.label()),
                            "data": {
                                "mode": parsed_mode.display_name(),
                                "label": parsed_mode.label(),
                                "permissionMode": derived,
                            },
                        }),
                    )
                    .await;
                } else {
                    send_error(
                        &mut sender,
                        &format!("unknown coding mode: {mode_str}"),
                        "UNKNOWN_CODING_MODE",
                    )
                    .await;
                }
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
                    if let Some(m) = model {
                        cfg.default_model = Some(m);
                    }
                    cfg.clone()
                };

                state.push_live_config(snapshot);
                let _ = send_json(
                    &mut sender,
                    &serde_json::json!({
                        "type": "system_notification",
                        "subtype": "runtime_config_updated",
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
                    let _ = super::ws::approval_sender_for_desktop().send(evt);
                }
            }
            "user_message" => {
                let content = parsed
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if content.is_empty() {
                    send_error(&mut sender, "empty user_message.content", "EMPTY_CONTENT").await;
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
                    let global_mode = *svc.coding_mode.read();
                    agent.set_coding_mode(global_mode);
                }

                {
                    let snap = state.config.lock().clone();
                    state.push_live_config(snap);
                }

                run_turn(&state, &mut agent, &mut sender, &session_id, &session_key, &content).await;
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
                if plan_path.is_empty() {
                    send_error(
                        &mut sender,
                        "empty start_plan_execution.planPath",
                        "EMPTY_PLAN_PATH",
                    )
                    .await;
                    continue;
                }

                let agent_mode = crate::agent::coding_mode::CodingMode::Agent;
                if let Some(svc) = crate::services::try_get_services() {
                    *svc.coding_mode.write() = agent_mode;
                }
                agent.set_coding_mode(agent_mode);
                let derived =
                    super::desktop_routes::derive_permission_from_coding(&agent_mode);
                desktop_runtime_state().set_permission_mode(derived);
                let _ = send_json(
                    &mut sender,
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

                let trigger_content = if resume {
                    format!(
                        "[Plan execution resume — Agent mode]\n\
                         The user clicked **Continue** on the plan card because the \
                         previous execution turn ended with unfinished todos. The plan \
                         file is `{plan_path}` and the in-memory plan tracker is \
                         already populated from the prior turn (do NOT re-run \
                         `update_plan(action=\"set\")` — that would wipe completion \
                         status).\n\n\
                         Your job for this turn:\n\
                         1. **Inspect current progress** — call \
                            `update_plan(action=\"get\")` to see which steps are \
                            `completed` / `skipped` / `in_progress` / `pending`.\n\
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
                         3. **Run the verification commands** in the plan's `## 验收` / \
                            Verification section before declaring done.\n\n\
                         **CRITICAL — never batch status flips at the end of the turn.** \
                         The user is watching a live progress bar fed by every \
                         `update_plan` call. If you do all the real work first and then \
                         fire a flurry of `update_plan(action=\"update\", …, \
                         status=\"completed\")` calls in a row at the end, the bar stays \
                         stuck and then jumps to 100% in one frame — that is exactly the \
                         failure mode this prompt forbids. Each step's `completed` flip \
                         must visibly precede the next step's `in_progress` flip.\n\n\
                         Do NOT stop, do NOT ask for confirmation, do NOT summarise \
                         what's already done. Work straight through every remaining \
                         step."
                    )
                } else {
                    format!(
                        "[Plan execution trigger — Agent mode]\n\
                         The user clicked **Build** on the plan card. The finalised plan \
                         is saved at `{plan_path}`.\n\n\
                         Your job for this turn:\n\
                         1. **Load the plan** — call `file_read(path=\"{plan_path}\")` to \
                            get the full markdown so you can see every todo / track / \
                            verification command.\n\
                         2. **Hydrate the in-memory plan tracker — verbatim copy from \
                            the plan file's `todos:` block.** Call \
                            `update_plan(action=\"set\", steps=[…])` exactly once. The \
                            `steps` array MUST be a 1-to-1 verbatim copy of the YAML \
                            `todos:` list you just loaded — same number of entries, \
                            same `id` values, same `content` (use it as `title`), all \
                            `status=\"pending\"`. Do NOT merge entries, do NOT split \
                            entries, do NOT rephrase or summarise — the user's progress \
                            bar is already showing those exact todos and any divergence \
                            here causes a visible \"plan jumped\" glitch. Without this \
                            hydration the per-step status updates below will fail with \
                            \"Step not found\". After this single `set` call, never call \
                            `set` again for the rest of the turn — use `update` only.\n\
                         3. **Execute each todo ONE AT A TIME, in order**. For every step, \
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
                         4. **Run the verification commands** in the `## 验收` section \
                            before declaring done.\n\n\
                         **CRITICAL — never batch status flips at the end of the turn.** \
                         The user is watching a live progress bar fed by every \
                         `update_plan` call. If you do all the real work first and then \
                         fire a flurry of `update_plan(action=\"update\", …, \
                         status=\"completed\")` calls in a row at the end, the bar stays \
                         stuck and then jumps to 100% in one frame — that is exactly the \
                         failure mode this prompt forbids. Each step's `completed` flip \
                         must visibly precede the next step's `in_progress` flip.\n\n\
                         Do NOT narrate the mode switch, do NOT re-ask questions the user \
                         already answered in Plan mode. Start with step 1 immediately."
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

                {
                    let snap = state.config.lock().clone();
                    state.push_live_config(snap);
                }

                agent.arm_plan_execution(plan_path.clone());

                run_turn(
                    &state,
                    &mut agent,
                    &mut sender,
                    &session_id,
                    &session_key,
                    &trigger_content,
                )
                .await;
            }
            other => {
                send_error(
                    &mut sender,
                    &format!("unsupported message type: {other}"),
                    "UNKNOWN_MESSAGE_TYPE",
                )
                .await;
            }
        }
    }

    reader_handle.abort();
    lsp_forwarder.abort();

    state.hooks.fire_session_end(&session_id, "ws_desktop").await;
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
            if let Err(err) = persist_permission_mode_to_disk(&p, mode) {
                tracing::warn!(
                    error = %err,
                    path = %p.display(),
                    "[desktop] failed to persist permission_mode to desktop_user.json"
                );
            }
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

async fn send_json(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    value: &serde_json::Value,
) -> Result<(), axum::Error> {
    sender.send(Message::Text(value.to_string().into())).await
}

async fn send_error(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    message: &str,
    code: &str,
) {
    let _ = send_json(
        sender,
        &serde_json::json!({
            "type": "error",
            "message": message,
            "code": code,
            "retryable": false,
        }),
    )
    .await;
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
        let t = self.thinking_buf.trim_end();
        if !t.is_empty() {
            let completed_ms = Self::wallclock_ms_unix();
            let mut obj = serde_json::Map::new();
            obj.insert("type".to_string(), serde_json::json!("thinking"));
            obj.insert(
                "thinking".to_string(),
                serde_json::Value::String(t.to_string()),
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
        }
        let tx = self.text_buf.trim_end();
        if !tx.is_empty() {
            self.assistant_segment
                .push(json!({ "type": "text", "text": tx }));
            self.text_buf.clear();
        }
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

    fn on_chunk(&mut self, delta: &str) {
        self.text_buf.push_str(delta);
    }

    fn on_thinking(&mut self, delta: &str) {
        if self.thinking_buf.is_empty() && !delta.is_empty() {
            self.thinking_segment_started_ms = Some(Self::wallclock_ms_unix());
        }
        self.thinking_buf.push_str(delta);
    }

    fn on_tool_use(&mut self, name: &str, tool_use_id: &str, input: serde_json::Value) {
        self.absorb_pending_text_into_segment();
        self.assistant_segment.push(json!({
            "type": "tool_use",
            "name": name,
            "id": tool_use_id,
            "input": input,
        }));
    }

    fn on_tool_result(&mut self, tool_use_id: &str, output: String, is_error: bool) {
        self.absorb_pending_text_into_segment();
        self.finalize_assistant_segment();
        let payload = vec![json!({
            "type": "tool_result",
            "tool_use_id": tool_use_id,
            "content": output,
            "is_error": is_error,
        })];
        if let Ok(s) = serde_json::to_string(&payload) {
            self.out.push(crate::providers::ChatMessage::tool(s));
        }
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
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    session_id: &str,
    session_key: &str,
    content: &str,
) {
    use crate::agent::TurnEvent;

    let _ = send_json(
        sender,
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
    if let (Some(svc), Some(ref label)) = (
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

    let turn_fut = async {
        let inner = async {
            crate::agent::scope_tool_loop_cost_tracking(
                cost_tracking_ctx,
                agent.turn_streamed(&content_owned, event_tx),
            )
            .await
        };
        let result = crate::evolution::scope_evolution_ctx(evolution_ctx.clone(), inner).await;
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
                            sender,
                            &serde_json::json!({
                                "type": "content_start",
                                "blockType": "text",
                            }),
                        )
                        .await;
                        text_block_open = true;
                    }
                    accumulated_text.push_str(&delta);
                    let _ = send_json(
                        sender,
                        &serde_json::json!({
                            "type": "content_delta",
                            "text": delta,
                        }),
                    )
                    .await;
                }
                TurnEvent::Thinking { delta } => {
                    if let Ok(mut pg) = sqlite_persist_forward.lock() {
                        pg.on_thinking(&delta);
                    }
                    let _ = send_json(
                        sender,
                        &serde_json::json!({
                            "type": "thinking",
                            "text": delta,
                        }),
                    )
                    .await;
                }
                TurnEvent::ToolCall { name, args } => {
                    text_block_open = false;
                    let id = next_tool_use_id();
                    current_tool_use_id = Some(id.clone());
                    tool_use_id_for_name.insert(name.clone(), id.clone());
                    if let Ok(mut pg) = sqlite_persist_forward.lock() {
                        pg.on_tool_use(&name, &id, args.clone());
                    }
                    let _ = send_json(
                        sender,
                        &serde_json::json!({
                            "type": "content_start",
                            "blockType": "tool_use",
                            "toolName": name,
                            "toolUseId": id,
                        }),
                    )
                    .await;
                    let _ = send_json(
                        sender,
                        &serde_json::json!({
                            "type": "tool_use_complete",
                            "toolName": name,
                            "toolUseId": id,
                            "input": args,
                        }),
                    )
                    .await;
                }
                TurnEvent::ToolResult { name, output } => {
                    let id = tool_use_id_for_name
                        .remove(&name)
                        .or_else(|| current_tool_use_id.clone())
                        .unwrap_or_else(next_tool_use_id);
                    current_tool_use_id = None;
                    let is_error = output.starts_with("Error:") || output.starts_with("error:");
                    if let Ok(mut pg) = sqlite_persist_forward.lock() {
                        pg.on_tool_result(&id, output.clone(), is_error);
                    }
                    let _ = send_json(
                        sender,
                        &serde_json::json!({
                            "type": "tool_result",
                            "toolUseId": id,
                            "content": output,
                            "isError": is_error,
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
                                if let Err(e) = backend.record_edit_batch(
                                    session_key,
                                    user_message_index,
                                    batch,
                                ) {
                                    tracing::warn!(
                                        target: "rewind",
                                        "record_edit_batch failed: session={} idx={} batch={} err={}",
                                        session_key, user_message_index, batch, e
                                    );
                                }
                            }
                        }
                    }

                    let _ = send_json(
                        sender,
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
                        sender,
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
                            sender,
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
                        sender,
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
                    let _ = send_json(
                        sender,
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
                        sender,
                        &serde_json::json!({
                            "type": "status",
                            "state": "idle",
                            "verb": "cancelling",
                        }),
                    )
                    .await;
                    let _ = send_json(
                        sender,
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
                    let _ = send_json(sender, &frame).await;
                }
                TurnEvent::SubagentChunk {
                    task_id,
                    agent_id,
                    kind,
                    delta,
                } => {
                    let mut data = serde_json::json!({
                        "taskId": task_id,
                        "agentId": agent_id,
                        "kind": format!("{kind:?}"),
                        "delta": delta,
                    });
                    if let Some(parent_id) = current_tool_use_id.as_ref()
                        && let serde_json::Value::Object(obj) = &mut data
                    {
                        obj.insert(
                            "parentToolUseId".to_string(),
                            serde_json::Value::String(parent_id.clone()),
                        );
                    }
                    let _ = send_json(
                        sender,
                        &serde_json::json!({
                            "type": "system_notification",
                            "subtype": "subagent_chunk",
                            "data": data,
                        }),
                    )
                    .await;
                }
                TurnEvent::Error { message } => {
                    send_error(sender, &message, "TURN_ERROR").await;
                }
            }
        }
    };

    let (turn_result, _) = tokio::join!(turn_fut, forward_fut);

    match turn_result {
        Ok(final_text) => {
            if let Some(ref backend) = state.session_backend {
                let recorder = sqlite_persist
                    .lock()
                    .ok()
                    .map(|mut g| std::mem::take(&mut *g))
                    .unwrap_or_default();
                let mut rows = recorder.finish();
                if rows.is_empty() && !final_text.trim().is_empty() {
                    rows.push(crate::providers::ChatMessage::assistant(final_text.clone()));
                }
                let backend_arc = std::sync::Arc::clone(backend);
                let session_key_owned = session_key.to_string();
                let _ = tokio::task::spawn_blocking(move || {
                    for msg in rows {
                        let _ = backend_arc.append(&session_key_owned, &msg);
                    }
                })
                .await;
            }
            let usage = agent.last_usage();
            let _ = send_json(
                sender,
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
            send_error(sender, &format!("{err}"), "AGENT_TURN_FAILED").await;
        }
    }

    let _ = send_json(
        sender,
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
                    sender,
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
