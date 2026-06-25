// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocket},
        Path, State, WebSocketUpgrade,
    },
    http::HeaderMap,
    response::{IntoResponse, Json},
};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;

use crate::computer::run::{run_loop, ComputerEvent, RunParams};
use crate::computer::session::run_registry;
use crate::config::Config;

const DEFAULT_MAX_STEPS: u32 = 40;
const MAX_ALLOWED_STEPS: u32 = 200;
const DEFAULT_STEP_DELAY_MS: u64 = 600;
const MAX_STEP_DELAY_MS: u64 = 10_000;

pub async fn handle_vision_models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = super::api::require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.live_config.load_ref();
    let models = crate::computer::list_vision_models(config.as_ref());
    Json(serde_json::json!({ "models": models })).into_response()
}

pub async fn handle_stop(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = super::api::require_auth(&state, &headers) {
        return e.into_response();
    }
    let run_id = body
        .get("runId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("");
    if run_id.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "runId is required" })),
        )
            .into_response();
    }
    let cancelled = run_registry().cancel(run_id);
    Json(serde_json::json!({ "ok": true, "cancelled": cancelled })).into_response()
}

pub async fn handle_ws_computer(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if state.exposed || state.pairing.require_pairing() {
        let token = super::ws::extract_ws_token(&headers, None).unwrap_or("");
        let authed = if state.exposed {
            state.pairing.is_authenticated_strict(token)
        } else {
            state.pairing.is_authenticated(token)
        };
        if !authed {
            return (
                axum::http::StatusCode::UNAUTHORIZED,
                "Unauthorized  - provide Authorization header or pairing token",
            )
                .into_response();
        }
    }

    ws.on_upgrade(move |socket| handle_socket(socket, state, run_id))
        .into_response()
}

async fn handle_socket(socket: WebSocket, state: AppState, run_id: String) {
    let (mut sink, mut receiver) = socket.split();

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<ComputerEvent>();
    let (reply_tx, reply_rx) = mpsc::unbounded_channel::<String>();
    let mut reply_rx = Some(reply_rx);

    let writer = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let payload = match serde_json::to_string(&event) {
                Ok(text) => text,
                Err(_) => continue,
            };
            if sink.send(Message::Text(payload.into())).await.is_err() {
                break;
            }
        }
    });

    let registry = run_registry();
    let mut loop_handle: Option<tokio::task::JoinHandle<()>> = None;
    let mut started = false;

    while let Some(Ok(message)) = receiver.next().await {
        match message {
            Message::Text(text) => {
                let parsed: serde_json::Value = match serde_json::from_str(text.as_str()) {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                let msg_type = parsed.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match msg_type {
                    "start" => {
                        if started {
                            continue;
                        }
                        let Some(params) = parse_start(&parsed, &run_id) else {
                            let _ = event_tx.send(ComputerEvent::Error {
                                message: "start message missing task/provider/model".into(),
                            });
                            continue;
                        };
                        let Some(reply_rx) = reply_rx.take() else {
                            continue;
                        };
                        started = true;
                        let config: Config = state.live_config.load_ref().as_ref().clone();
                        let cancel = registry.register(&run_id);
                        let event_tx = event_tx.clone();
                        loop_handle = Some(tokio::spawn(run_loop(
                            params, config, cancel, event_tx, reply_rx,
                        )));
                    }
                    "stop" => {
                        registry.cancel(&run_id);
                    }
                    "user_reply" => {
                        if let Some(reply) = parsed.get("text").and_then(|v| v.as_str()) {
                            let _ = reply_tx.send(reply.to_string());
                        }
                    }
                    _ => {}
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    registry.cancel(&run_id);
    registry.unregister(&run_id);
    if let Some(handle) = loop_handle {
        let _ = handle.await;
    }
    drop(event_tx);
    let _ = writer.await;
}

fn parse_start(value: &serde_json::Value, run_id: &str) -> Option<RunParams> {
    let task = value
        .get("task")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    let provider = value
        .get("provider")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    let model = value
        .get("model")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();

    let max_steps = value
        .get("maxSteps")
        .and_then(serde_json::Value::as_u64)
        .map(|n| (n as u32).clamp(1, MAX_ALLOWED_STEPS))
        .unwrap_or(DEFAULT_MAX_STEPS);
    let step_delay_ms = value
        .get("stepDelayMs")
        .and_then(serde_json::Value::as_u64)
        .map(|n| n.min(MAX_STEP_DELAY_MS))
        .unwrap_or(DEFAULT_STEP_DELAY_MS);

    Some(RunParams {
        run_id: run_id.to_string(),
        task,
        provider,
        model,
        max_steps,
        step_delay_ms,
    })
}
