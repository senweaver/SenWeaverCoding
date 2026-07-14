// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::AppState;
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

use crate::computer::recorder::{self, RecorderEvent, RecorderStatus};
use crate::config::Config;

pub async fn handle_list_recordings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = super::super::api::require_auth(&state, &headers) {
        return e.into_response();
    }
    let workspace = state.live_config.load_ref().workspace_dir.clone();
    let recordings = recorder::list_recordings(&workspace);
    Json(serde_json::json!({ "recordings": recordings })).into_response()
}

pub async fn handle_delete_recording(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = super::super::api::require_auth(&state, &headers) {
        return e.into_response();
    }
    let workspace = state.live_config.load_ref().workspace_dir.clone();
    match recorder::delete_recording(&workspace, &name).await {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn handle_rename_recording(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = super::super::api::require_auth(&state, &headers) {
        return e.into_response();
    }
    let new_name = body
        .get("newName")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("");
    if new_name.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "newName is required" })),
        )
            .into_response();
    }
    let workspace = state.live_config.load_ref().workspace_dir.clone();
    match recorder::rename_recording(&workspace, &name, new_name).await {
        Ok(renamed) => Json(serde_json::json!({ "ok": true, "name": renamed })).into_response(),
        Err(e) => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

const MAX_EDITED_STEPS: usize = 500;
const MAX_EDITED_DELAY_MS: u64 = 600_000;
const MAX_EDITED_VALUE_CHARS: usize = 10_000;
const MAX_LOOP_COUNT: u32 = 100;
const MAX_LOOP_INTERVAL_MS: u64 = 3_600_000;

const EDITABLE_ACTIONS: [&str; 9] = [
    "click",
    "double_click",
    "right_click",
    "type",
    "key_press",
    "scroll",
    "drag",
    "move_mouse",
    "wait",
];

pub async fn handle_get_recording_steps(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = super::super::api::require_auth(&state, &headers) {
        return e.into_response();
    }
    let workspace = state.live_config.load_ref().workspace_dir.clone();
    match recorder::load_recording(&workspace, &name).await {
        Ok(manifest) => Json(serde_json::json!({
            "name": name,
            "task": manifest.task,
            "display_w": manifest.display_w,
            "display_h": manifest.display_h,
            "run_config": manifest.run_config,
            "steps": manifest.steps,
        }))
        .into_response(),
        Err(e) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn handle_put_recording_steps(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = super::super::api::require_auth(&state, &headers) {
        return e.into_response();
    }
    let workspace = state.live_config.load_ref().workspace_dir.clone();
    let mut manifest = match recorder::load_recording(&workspace, &name).await {
        Ok(manifest) => manifest,
        Err(e) => {
            return (
                axum::http::StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    if let Some(raw_steps) = body.get("steps") {
        let steps: Vec<crate::computer::recorder::RecordedStep> =
            match serde_json::from_value(raw_steps.clone()) {
                Ok(steps) => steps,
                Err(e) => {
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({ "error": format!("invalid steps: {e}") })),
                    )
                        .into_response();
                }
            };
        if steps.is_empty() || steps.len() > MAX_EDITED_STEPS {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("steps must contain 1..={MAX_EDITED_STEPS} entries")
                })),
            )
                .into_response();
        }
        for step in &steps {
            if !EDITABLE_ACTIONS.contains(&step.action_type.as_str()) {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": format!("unsupported action type '{}'", step.action_type)
                    })),
                )
                    .into_response();
            }
            if step.delay_ms > MAX_EDITED_DELAY_MS {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": format!("delay_ms must be <= {MAX_EDITED_DELAY_MS}")
                    })),
                )
                    .into_response();
            }
            if step
                .value
                .as_ref()
                .is_some_and(|v| v.chars().count() > MAX_EDITED_VALUE_CHARS)
            {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": format!("value must be <= {MAX_EDITED_VALUE_CHARS} characters")
                    })),
                )
                    .into_response();
            }
        }
        let mut reindexed = steps;
        for (idx, step) in reindexed.iter_mut().enumerate() {
            step.index = idx as u32;
        }
        manifest.steps = reindexed;
    }

    if let Some(raw_config) = body.get("runConfig") {
        if raw_config.is_null() {
            manifest.run_config = None;
        } else {
            let loop_count = raw_config
                .get("loopCount")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(1) as u32;
            let interval_ms = raw_config
                .get("intervalMs")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            manifest.run_config = Some(crate::computer::recorder::RunConfig {
                loop_count: loop_count.clamp(1, MAX_LOOP_COUNT),
                interval_ms: interval_ms.min(MAX_LOOP_INTERVAL_MS),
            });
        }
    }

    match recorder::save_recording_manifest(&workspace, &name, &manifest).await {
        Ok(()) => Json(serde_json::json!({
            "ok": true,
            "step_count": manifest.steps.len(),
            "run_config": manifest.run_config,
        }))
        .into_response(),
        Err(e) => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn handle_generate_recording_skill(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = super::super::api::require_auth(&state, &headers) {
        return e.into_response();
    }
    let config: Config = state.live_config.load_ref().as_ref().clone();
    let provider = body
        .get("provider")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| config.multimodal.vision_provider.clone())
        .or_else(|| config.default_provider.clone());
    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| config.multimodal.vision_model.clone())
        .or_else(|| config.default_model.clone());
    let (Some(provider), Some(model)) = (provider, model) else {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "no vision provider/model configured for skill generation"
            })),
        )
            .into_response();
    };
    let workspace = state.live_config.load_ref().workspace_dir.clone();
    let recording_path = workspace.join("skills").join(&name).join("recording.json");
    if name.contains('/') || name.contains('\\') || name.contains("..") || !recording_path.exists()
    {
        return (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("recording '{name}' not found") })),
        )
            .into_response();
    }
    if let Err(e) = crate::computer::vision::VisionClient::from_config(&config, &provider, &model)
    {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("failed to initialize vision model '{model}': {e}")
            })),
        )
            .into_response();
    }
    tokio::spawn(async move {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<RecorderEvent>();
        let drain = tokio::spawn(async move { while event_rx.recv().await.is_some() {} });
        if let Err(e) =
            recorder::generate_skill(&config, &provider, &model, &workspace, &name, &event_tx)
                .await
        {
            tracing::warn!("background skill generation for '{name}' failed: {e}");
        }
        drop(event_tx);
        let _ = drain.await;
    });
    Json(serde_json::json!({ "ok": true, "started": true })).into_response()
}

pub async fn handle_ws_record(
    State(state): State<AppState>,
    Path(rec_id): Path<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if let Some(reject) =
        crate::gateway::cors::reject_ws_disallowed_origin(&headers, "/ws/computer/record")
    {
        return reject;
    }
    if state.exposed || state.pairing.require_pairing() {
        let token = super::super::ws::extract_ws_token(&headers, None).unwrap_or("");
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

    let _ = rec_id;
    ws.on_upgrade(move |socket| handle_socket_record(socket, state))
        .into_response()
}

async fn handle_socket_record(socket: WebSocket, state: AppState) {
    let (mut sink, mut receiver) = socket.split();
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<RecorderEvent>();

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

    let mut owned_generation: Option<u64> = None;

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
                        let task = parsed
                            .get("task")
                            .and_then(|v| v.as_str())
                            .map(str::trim)
                            .unwrap_or("")
                            .to_string();
                        let workspace = state.live_config.load_ref().workspace_dir.clone();
                        match recorder::start_recording(workspace, task, event_tx.clone()).await {
                            Ok(generation) => {
                                owned_generation = Some(generation);
                                let _ = event_tx.send(RecorderEvent::status(
                                    RecorderStatus::Recording,
                                    None,
                                ));
                            }
                            Err(e) => {
                                let _ = event_tx.send(RecorderEvent::error_code(
                                    "recorder_start_failed",
                                    e.to_string(),
                                ));
                                let _ = event_tx
                                    .send(RecorderEvent::status(RecorderStatus::Error, None));
                            }
                        }
                    }
                    "stop" => {
                        let Some(generation) = owned_generation else {
                            continue;
                        };
                        match recorder::stop_recording(generation).await {
                            Ok(summary) => {
                                if summary.step_count > 0 && !summary.name.is_empty() {
                                    let _ = event_tx.send(RecorderEvent::RecordingSaved {
                                        name: summary.name.clone(),
                                    });
                                }
                                let _ = event_tx.send(RecorderEvent::status_code(
                                    RecorderStatus::Stopped,
                                    "recorder_stopped_count",
                                    format!("recorded {} steps", summary.step_count),
                                ));
                            }
                            Err(e) => {
                                let _ = event_tx.send(RecorderEvent::error_code(
                                    "recorder_stop_failed",
                                    e.to_string(),
                                ));
                                let _ = event_tx
                                    .send(RecorderEvent::status(RecorderStatus::Error, None));
                            }
                        }
                    }
                    "discard" => {
                        let Some(generation) = owned_generation.take() else {
                            continue;
                        };
                        let _ = recorder::discard_recording(generation).await;
                        let _ = event_tx.send(RecorderEvent::status(RecorderStatus::Idle, None));
                    }
                    "generate" | "generate_saved" => {
                        let name = parsed
                            .get("name")
                            .and_then(|v| v.as_str())
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                            .or_else(recorder::last_saved_recording);
                        let Some(name) = name else {
                            let _ = event_tx.send(RecorderEvent::error_code(
                                "no_saved_recording",
                                "no saved recording to generate a skill from",
                            ));
                            continue;
                        };
                        let config: Config = state.live_config.load_ref().as_ref().clone();
                        let provider = parsed
                            .get("provider")
                            .and_then(|v| v.as_str())
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                            .or_else(|| config.multimodal.vision_provider.clone())
                            .or_else(|| config.default_provider.clone());
                        let model = parsed
                            .get("model")
                            .and_then(|v| v.as_str())
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                            .or_else(|| config.multimodal.vision_model.clone())
                            .or_else(|| config.default_model.clone());
                        let (Some(provider), Some(model)) = (provider, model) else {
                            let _ = event_tx.send(RecorderEvent::error_code(
                                "no_vision_model",
                                "no vision provider/model configured for skill generation",
                            ));
                            continue;
                        };
                        let workspace = state.live_config.load_ref().workspace_dir.clone();
                        let event_tx = event_tx.clone();
                        tokio::spawn(async move {
                            let _ = event_tx
                                .send(RecorderEvent::status(RecorderStatus::Generating, None));
                            match recorder::generate_skill(
                                &config, &provider, &model, &workspace, &name, &event_tx,
                            )
                            .await
                            {
                                Ok(slug) => {
                                    let _ = event_tx.send(RecorderEvent::SkillSaved {
                                        name: slug,
                                    });
                                    let _ = event_tx
                                        .send(RecorderEvent::status(RecorderStatus::Saved, None));
                                }
                                Err(e) => {
                                    let _ = event_tx.send(RecorderEvent::error_code(
                                        "skill_generate_failed",
                                        e.to_string(),
                                    ));
                                    let _ = event_tx
                                        .send(RecorderEvent::status(RecorderStatus::Error, None));
                                }
                            }
                        });
                    }
                    _ => {}
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    if let Some(generation) = owned_generation {
        let _ = recorder::discard_recording(generation).await;
    }
    drop(event_tx);
    let _ = writer.await;
}
