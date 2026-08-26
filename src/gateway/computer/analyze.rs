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
use std::path::PathBuf;
use tokio::sync::mpsc;

use crate::computer::describe::{
    self, edit_analysis, AnalysisEdit, AnalysisFeedback, AnalysisStep, AnalyzeEvent, AnalyzeRequest,
    FeedbackStepNote,
};
use crate::config::Config;

fn sanitize(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains("..")
    {
        return None;
    }
    Some(trimmed.to_string())
}

fn recording_dir(state: &AppState, name: &str) -> Option<PathBuf> {
    let safe = sanitize(name)?;
    let dir = state
        .live_config
        .load_ref()
        .workspace_dir
        .join("skills")
        .join(&safe);
    dir.join("recording.json").is_file().then_some(dir)
}

pub async fn handle_get_analysis(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = super::super::api::require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(dir) = recording_dir(&state, &name) else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("recording '{name}' not found") })),
        )
            .into_response();
    };
    let analysis = describe::load_analysis(&dir);
    let sensitive = crate::computer::sensitive::load_report(&dir);
    Json(serde_json::json!({
        "analysis": analysis,
        "sensitiveReport": sensitive,
    }))
    .into_response()
}

pub async fn handle_put_analysis(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = super::super::api::require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(dir) = recording_dir(&state, &name) else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("recording '{name}' not found") })),
        )
            .into_response();
    };
    let steps = body.get("steps").and_then(|v| v.as_array()).map(|arr| {
        arr.iter()
            .enumerate()
            .filter_map(|(idx, raw)| {
                let title = raw.get("title").and_then(|v| v.as_str())?;
                Some(AnalysisStep {
                    id: raw
                        .get("id")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.trim().is_empty())
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("s{}", idx + 1)),
                    title: title.trim().to_string(),
                    detail: raw
                        .get("detail")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .trim()
                        .to_string(),
                    start_ms: raw.get("startMs").and_then(|v| v.as_i64()),
                    end_ms: raw.get("endMs").and_then(|v| v.as_i64()),
                    apps: raw
                        .get("apps")
                        .and_then(|v| v.as_array())
                        .map(|a| a.iter().filter_map(|v| v.as_str()).map(str::to_string).collect())
                        .unwrap_or_default(),
                    evidence: raw
                        .get("evidence")
                        .and_then(|v| v.as_array())
                        .map(|a| a.iter().filter_map(|v| v.as_str()).map(str::to_string).collect())
                        .unwrap_or_default(),
                    confidence: describe::analysis::Confidence::Medium,
                })
            })
            .collect::<Vec<_>>()
    });
    let edit = AnalysisEdit {
        title: body.get("title").and_then(|v| v.as_str()).map(str::to_string),
        intent: body.get("intent").and_then(|v| v.as_str()).map(str::to_string),
        steps,
        approved: body.get("approved").and_then(|v| v.as_bool()),
    };
    match edit_analysis(&dir, edit) {
        Ok(analysis) => Json(serde_json::json!({ "ok": true, "analysis": analysis })).into_response(),
        Err(e) => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn handle_get_sensitive_report(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = super::super::api::require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(dir) = recording_dir(&state, &name) else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("recording '{name}' not found") })),
        )
            .into_response();
    };
    Json(serde_json::json!({ "report": crate::computer::sensitive::load_report(&dir) }))
        .into_response()
}

pub async fn handle_ws_analyze(
    State(state): State<AppState>,
    Path(rec_id): Path<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if let Some(reject) =
        crate::gateway::cors::reject_ws_disallowed_origin(&headers, "/ws/computer-analyze")
    {
        return reject;
    }
    if state.exposed || state.pairing.require_pairing() {
        let tokens = super::super::ws::websocket_tokens(&headers, None);
        let authed = tokens.iter().any(|token| {
            if state.exposed {
                state.pairing.is_authenticated_strict(token)
            } else {
                state.pairing.is_authenticated(token)
            }
        });
        if !authed {
            return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
        }
    }
    let _ = rec_id;
    let ws = super::super::ws::with_websocket_auth_protocol(ws, &headers);
    ws.on_upgrade(move |socket| handle_socket_analyze(socket, state))
        .into_response()
}

async fn handle_socket_analyze(socket: WebSocket, state: AppState) {
    let (mut sink, mut receiver) = socket.split();
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AnalyzeEvent>();
    let writer = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let Ok(payload) = serde_json::to_string(&event) else {
                continue;
            };
            if sink.send(Message::Text(payload.into())).await.is_err() {
                break;
            }
        }
    });

    let mut running: Option<tokio::task::JoinHandle<()>> = None;

    while let Some(Ok(message)) = receiver.next().await {
        let Message::Text(text) = message else {
            if matches!(message, Message::Close(_)) {
                break;
            }
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(text.as_str()) else {
            continue;
        };
        match parsed.get("type").and_then(|v| v.as_str()).unwrap_or("") {
            "start" | "feedback" => {
                if running.as_ref().is_some_and(|h| !h.is_finished()) {
                    let _ = event_tx.send(AnalyzeEvent::Error {
                        message: "an analysis is already running for this recording".to_string(),
                    });
                    continue;
                }
                let name = parsed
                    .get("recording")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let Some(dir) = recording_dir(&state, &name) else {
                    let _ = event_tx.send(AnalyzeEvent::Error {
                        message: format!("recording '{name}' not found"),
                    });
                    continue;
                };
                let config: Config = state.live_config.load_ref().as_ref().clone();
                let workspace = state.live_config.load_ref().workspace_dir.clone();
                let Some((provider, model)) = super::resolve_vision_route(&parsed, &config) else {
                    let _ = event_tx.send(AnalyzeEvent::Error {
                        message: "no vision provider/model configured".to_string(),
                    });
                    continue;
                };
                let feedback = parse_feedback(&parsed);
                let request = AnalyzeRequest {
                    dir,
                    session_id: name,
                    provider,
                    model,
                    feedback,
                };
                let tx = event_tx.clone();
                running = Some(tokio::spawn(async move {
                    if let Err(e) =
                        describe::run_analyze(&config, &workspace, request, &tx).await
                    {
                        let _ = tx.send(AnalyzeEvent::Error {
                            message: e.to_string(),
                        });
                    }
                }));
            }
            "cancel" => {
                if let Some(handle) = running.take() {
                    handle.abort();
                }
            }
            _ => {}
        }
    }

    if let Some(handle) = running {
        handle.abort();
    }
    drop(event_tx);
    let _ = writer.await;
}

fn parse_feedback(parsed: &serde_json::Value) -> Option<AnalysisFeedback> {
    let overall = parsed
        .get("overall")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let steps = parsed
        .get("steps")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|raw| {
                    let step_id = raw.get("stepId").and_then(|v| v.as_str())?;
                    let note = raw.get("note").and_then(|v| v.as_str())?;
                    if note.trim().is_empty() {
                        return None;
                    }
                    Some(FeedbackStepNote {
                        step_id: step_id.to_string(),
                        note: note.trim().to_string(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if overall.is_none() && steps.is_empty() {
        None
    } else {
        Some(AnalysisFeedback { overall, steps })
    }
}
