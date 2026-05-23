// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::AppState;
use super::api::require_auth;
use crate::runtime::task_manager::spawn_supervised;
use axum::{
    extract::{
        Path, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Json},
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct CanvasPostBody {
    pub content_type: Option<String>,
    pub content: String,
}

pub async fn handle_canvas_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let ids = state.canvas_store.list();
    Json(serde_json::json!({ "canvases": ids })).into_response()
}

pub async fn handle_canvas_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    match state.canvas_store.snapshot(&id) {
        Some(frame) => Json(serde_json::json!({
            "canvas_id": id,
            "frame": frame,
        }))
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Canvas '{}' not found", id) })),
        )
            .into_response(),
    }
}

pub async fn handle_canvas_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let history = state.canvas_store.history(&id);
    Json(serde_json::json!({
        "canvas_id": id,
        "frames": history,
    }))
    .into_response()
}

pub async fn handle_canvas_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<CanvasPostBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let content_type = body.content_type.as_deref().unwrap_or("html");

    if !crate::tools::canvas::ALLOWED_CONTENT_TYPES.contains(&content_type) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!(
                    "Invalid content_type '{}'. Allowed: {:?}",
                    content_type,
                    crate::tools::canvas::ALLOWED_CONTENT_TYPES
                )
            })),
        )
            .into_response();
    }

    if body.content.len() > crate::tools::canvas::MAX_CONTENT_SIZE {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(serde_json::json!({
                "error": format!(
                    "Content exceeds maximum size of {} bytes",
                    crate::tools::canvas::MAX_CONTENT_SIZE
                )
            })),
        )
            .into_response();
    }

    match state.canvas_store.render(&id, content_type, &body.content) {
        Some(frame) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "canvas_id": id,
                "frame": frame,
            })),
        )
            .into_response(),
        None => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "error": "Maximum canvas count reached. Clear unused canvases first."
            })),
        )
            .into_response(),
    }
}

pub async fn handle_canvas_clear(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    state.canvas_store.clear(&id);
    Json(serde_json::json!({
        "canvas_id": id,
        "status": "cleared",
    }))
    .into_response()
}

pub async fn handle_ws_canvas(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {

    if state.pairing.require_pairing() {
        let token = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|auth| auth.strip_prefix("Bearer "))
            .or_else(|| {

                headers
                    .get("sec-websocket-protocol")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|protos| {
                        protos
                            .split(',')
                            .map(|p| p.trim())
                            .find_map(|p| p.strip_prefix("bearer."))
                    })
            })
            .unwrap_or("");

        if !state.pairing.is_authenticated(token) {
            return (
                StatusCode::UNAUTHORIZED,
                "Unauthorized — provide Authorization header or Sec-WebSocket-Protocol bearer",
            )
                .into_response();
        }
    }

    ws.on_upgrade(move |socket| handle_canvas_socket(socket, state, id))
        .into_response()
}

async fn handle_canvas_socket(socket: WebSocket, state: AppState, canvas_id: String) {
    let (mut sender, mut receiver) = socket.split();

    let mut rx = match state.canvas_store.subscribe(&canvas_id) {
        Some(rx) => rx,
        None => {
            let msg = serde_json::json!({
                "type": "error",
                "error": "Maximum canvas count reached",
            });
            let _ = sender.send(Message::Text(msg.to_string().into())).await;
            return;
        }
    };

    if let Some(frame) = state.canvas_store.snapshot(&canvas_id) {
        let msg = serde_json::json!({
            "type": "frame",
            "canvas_id": canvas_id,
            "frame": frame,
        });
        let _ = sender.send(Message::Text(msg.to_string().into())).await;
    }

    let ack = serde_json::json!({
        "type": "connected",
        "canvas_id": canvas_id,
    });
    let _ = sender.send(Message::Text(ack.to_string().into())).await;

    let canvas_id_clone = canvas_id.clone();
    let send_task = spawn_supervised("gateway.canvas.broadcast_forwarder", async move {
        loop {
            match rx.recv().await {
                Ok(frame) => {
                    let msg = serde_json::json!({
                        "type": "frame",
                        "canvas_id": canvas_id_clone,
                        "frame": frame,
                    });
                    if sender
                        .send(Message::Text(msg.to_string().into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {

                    let msg = serde_json::json!({
                        "type": "lagged",
                        "canvas_id": canvas_id_clone,
                        "missed_frames": n,
                    });
                    let _ = sender.send(Message::Text(msg.to_string().into())).await;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Close(_)) | Err(_) => break,
            _ => {}
        }
    }

    send_task.abort();
}
