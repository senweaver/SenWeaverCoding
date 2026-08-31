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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc::{self, UnboundedSender};

pub mod analyze;
pub mod build_ws;
pub mod misc;
pub mod record;

use crate::computer::briefing;
use crate::computer::recorder::ReplayRepeat;
use crate::computer::run::{run_loop, ComputerEvent, RunParams, UserMessage};
use crate::computer::session::run_registry;
use crate::computer::vision::VisionClient;
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

pub async fn handle_plan_draft(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = super::api::require_auth(&state, &headers) {
        return e.into_response();
    }
    let config: Config = state.live_config.load_ref().as_ref().clone();
    let task = body
        .get("task")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let attachments = match briefing::parse_attachments(body.get("attachments"), &config) {
        Ok(attachments) => attachments,
        Err((code, message)) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": message, "code": code })),
            )
                .into_response();
        }
    };
    if task.is_empty() && attachments.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "task or attachments required",
                "code": "plan_draft_failed"
            })),
        )
            .into_response();
    }
    let Some((provider, model)) = resolve_vision_route(&body, &config) else {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "no vision provider/model configured",
                "code": "no_vision_model"
            })),
        )
            .into_response();
    };
    let client = match VisionClient::from_config(&config, &provider, &model) {
        Ok(client) => client,
        Err(err) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("failed to initialize model '{model}': {err}"),
                    "code": "model_init_failed"
                })),
            )
                .into_response();
        }
    };
    match briefing::draft_execution_steps(&client, &task, &attachments).await {
        Ok(steps) => Json(serde_json::json!({ "steps": steps })).into_response(),
        Err(err) => (
            axum::http::StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({
                "error": format!("plan draft failed: {err}"),
                "code": "plan_draft_failed"
            })),
        )
            .into_response(),
    }
}

pub async fn handle_ws_computer(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if let Some(reject) =
        crate::gateway::cors::reject_ws_disallowed_origin(&headers, "/ws/computer")
    {
        return reject;
    }
    if state.exposed || state.pairing.require_pairing() {
        let tokens = super::ws::websocket_tokens(&headers, None);
        let authed = tokens.iter().any(|token| {
            if state.exposed {
                state.pairing.is_authenticated_strict(token)
            } else {
                state.pairing.is_authenticated(token)
            }
        });
        if !authed {
            return (
                axum::http::StatusCode::UNAUTHORIZED,
                "Unauthorized  - provide Authorization header or pairing token",
            )
                .into_response();
        }
    }

    let ws = super::ws::with_websocket_auth_protocol(ws, &headers);
    ws.on_upgrade(move |socket| handle_socket(socket, state, run_id))
        .into_response()
}

enum RunKind {
    Agent,
    Replay { recording: String, task: String },
}

struct ActiveRun {
    handle: tokio::task::JoinHandle<()>,
    kind: RunKind,
    mute: Arc<AtomicBool>,
    user_tx: Option<UnboundedSender<UserMessage>>,
}

impl ActiveRun {
    fn is_alive(&self) -> bool {
        !self.handle.is_finished()
    }
}

fn spawn_event_forwarder(
    event_tx: &UnboundedSender<ComputerEvent>,
    mute: Arc<AtomicBool>,
) -> UnboundedSender<ComputerEvent> {
    let (run_tx, mut run_rx) = mpsc::unbounded_channel::<ComputerEvent>();
    let downstream = event_tx.clone();
    tokio::spawn(async move {
        while let Some(event) = run_rx.recv().await {
            if mute.load(Ordering::Acquire) {
                continue;
            }
            if downstream.send(event).is_err() {
                break;
            }
        }
    });
    run_tx
}

pub(crate) fn resolve_vision_route(
    value: &serde_json::Value,
    config: &Config,
) -> Option<(String, String)> {
    let explicit_provider = value
        .get("provider")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let explicit_model = value
        .get("model")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if let (Some(provider), Some(model)) = (explicit_provider, explicit_model) {
        return Some((provider, model));
    }
    if let (Some(provider), Some(model)) = (
        config.multimodal.vision_provider.as_deref(),
        config.multimodal.vision_model.as_deref(),
    ) {
        let provider = provider.trim();
        let model = model.trim();
        if !provider.is_empty() && !model.is_empty() {
            return Some((provider.to_string(), model.to_string()));
        }
    }
    let models = crate::computer::list_vision_models(config);
    let pick = models
        .iter()
        .find(|m| m.recommended)
        .or_else(|| models.first())?;
    Some((pick.provider.clone(), pick.model.clone()))
}

fn parse_repeat(value: &serde_json::Value) -> Option<ReplayRepeat> {
    let repeat = value.get("repeat")?;
    let count = repeat
        .get("count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1) as u32;
    let interval_ms = repeat
        .get("intervalMs")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    Some(
        ReplayRepeat {
            count,
            interval_ms,
        }
        .clamped(),
    )
}

fn parse_user_message(
    parsed: &serde_json::Value,
    config: &Config,
) -> Result<Option<UserMessage>, (&'static str, String)> {
    let attachments = briefing::parse_attachments(parsed.get("attachments"), config)?;
    let mut text = parsed
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if !attachments.document_block.is_empty() {
        text.push_str(&attachments.document_block);
    }
    if text.is_empty() && attachments.image_data_uris.is_empty() {
        return Ok(None);
    }
    Ok(Some(UserMessage {
        text,
        image_data_uris: attachments.image_data_uris,
    }))
}

async fn handle_socket(socket: WebSocket, state: AppState, run_id: String) {
    let (mut sink, mut receiver) = socket.split();

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<ComputerEvent>();

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
    let mut active: Option<ActiveRun> = None;

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
                        if active.as_ref().is_some_and(ActiveRun::is_alive) {
                            continue;
                        }
                        let config: Config = state.live_config.load_ref().as_ref().clone();
                        let mode = parsed.get("mode").and_then(|v| v.as_str()).unwrap_or("agent");

                        if mode == "replay" {
                            let Some(name) = parsed
                                .get("recording")
                                .and_then(|v| v.as_str())
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                            else {
                                let _ = event_tx.send(ComputerEvent::error_code(
                                    "replay_missing_recording",
                                    "replay start message missing recording name",
                                ));
                                continue;
                            };
                            let manifest = match crate::computer::recorder::load_recording(
                                &config.workspace_dir,
                                name,
                            )
                            .await
                            {
                                Ok(manifest) => manifest,
                                Err(e) => {
                                    let _ = event_tx.send(ComputerEvent::error_code(
                                        "replay_load_failed",
                                        format!("failed to load recording: {e}"),
                                    ));
                                    continue;
                                }
                            };
                            let repeat = parse_repeat(&parsed)
                                .or_else(|| {
                                    manifest.run_config.map(|rc| {
                                        ReplayRepeat {
                                            count: rc.loop_count.max(1),
                                            interval_ms: rc.interval_ms,
                                        }
                                        .clamped()
                                    })
                                })
                                .unwrap_or_default();
                            let smart = parsed
                                .get("smart")
                                .and_then(serde_json::Value::as_bool)
                                .unwrap_or(false);
                            let replay_kind = RunKind::Replay {
                                recording: name.to_string(),
                                task: manifest.task.clone(),
                            };
                            if smart {
                                let Some((provider, model)) =
                                    resolve_vision_route(&parsed, &config)
                                else {
                                    let _ = event_tx.send(ComputerEvent::error_code(
                                        "no_vision_model",
                                        "no vision provider/model configured for smart replay",
                                    ));
                                    continue;
                                };
                                let recording_dir =
                                    config.workspace_dir.join("skills").join(name);
                                let cancel = registry.register(&run_id);
                                let mute = Arc::new(AtomicBool::new(false));
                                let run_tx = spawn_event_forwarder(&event_tx, mute.clone());
                                let handle = tokio::spawn(
                                    crate::computer::recorder::replay_recording_smart(
                                        manifest,
                                        recording_dir,
                                        config,
                                        provider,
                                        model,
                                        repeat,
                                        cancel,
                                        run_tx,
                                    ),
                                );
                                active = Some(ActiveRun {
                                    handle,
                                    kind: replay_kind,
                                    mute,
                                    user_tx: None,
                                });
                                continue;
                            }
                            let cancel = registry.register(&run_id);
                            let mute = Arc::new(AtomicBool::new(false));
                            let run_tx = spawn_event_forwarder(&event_tx, mute.clone());
                            let handle = tokio::spawn(
                                crate::computer::recorder::replay_recording(
                                    manifest, repeat, cancel, run_tx,
                                ),
                            );
                            active = Some(ActiveRun {
                                handle,
                                kind: replay_kind,
                                mute,
                                user_tx: None,
                            });
                            continue;
                        }

                        let Some(mut params) = parse_start(&parsed, &run_id) else {
                            let _ = event_tx.send(ComputerEvent::error_code(
                                "start_missing_params",
                                "start message missing task/provider/model",
                            ));
                            continue;
                        };
                        match briefing::parse_attachments(parsed.get("attachments"), &config) {
                            Ok(attachments) => {
                                params.reference_images = attachments.image_data_uris;
                                if !attachments.document_block.is_empty() {
                                    params.task.push_str(&attachments.document_block);
                                }
                            }
                            Err((code, message)) => {
                                let _ = event_tx.send(ComputerEvent::error_code(code, message));
                                let _ = event_tx.send(ComputerEvent::status_code(
                                    crate::computer::run::RunStatus::Error,
                                    code,
                                    "run aborted due to invalid attachments",
                                ));
                                continue;
                            }
                        }
                        if let Some(skill) = parsed
                            .get("skill")
                            .and_then(|v| v.as_str())
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                        {
                            let Some(instructions) =
                                crate::computer::recorder::load_skill_instructions(
                                    &config.workspace_dir,
                                    skill,
                                )
                            else {
                                let _ = event_tx.send(ComputerEvent::error_code(
                                    "skill_not_found",
                                    format!("skill '{skill}' not found"),
                                ));
                                let _ = event_tx.send(ComputerEvent::status_code(
                                    crate::computer::run::RunStatus::Error,
                                    "skill_not_found",
                                    format!("skill '{skill}' not found; run aborted"),
                                ));
                                continue;
                            };
                            params.task = format!(
                                "{instructions}\n\n---\nValues for this run:\n{}",
                                params.task
                            );
                        }
                        let (user_tx, user_rx) = mpsc::unbounded_channel::<UserMessage>();
                        let cancel = registry.register(&run_id);
                        let mute = Arc::new(AtomicBool::new(false));
                        let run_tx = spawn_event_forwarder(&event_tx, mute.clone());
                        let handle =
                            tokio::spawn(run_loop(params, config, cancel, run_tx, user_rx));
                        active = Some(ActiveRun {
                            handle,
                            kind: RunKind::Agent,
                            mute,
                            user_tx: Some(user_tx),
                        });
                    }
                    "stop" => {
                        registry.cancel(&run_id);
                    }
                    "user_reply" | "steer" => {
                        let config: Config = state.live_config.load_ref().as_ref().clone();
                        let msg = match parse_user_message(&parsed, &config) {
                            Ok(Some(msg)) => msg,
                            Ok(None) => continue,
                            Err((code, message)) => {
                                let _ = event_tx.send(ComputerEvent::error_code(code, message));
                                continue;
                            }
                        };
                        let Some(run) = active.as_ref() else {
                            continue;
                        };
                        if !run.is_alive() {
                            continue;
                        }
                        match &run.kind {
                            RunKind::Agent => {
                                if let Some(user_tx) = &run.user_tx {
                                    let _ = user_tx.send(msg);
                                }
                            }
                            RunKind::Replay { .. } => {
                                takeover_replay(
                                    &mut active,
                                    &registry,
                                    &run_id,
                                    &event_tx,
                                    config,
                                    &parsed,
                                    msg,
                                )
                                .await;
                            }
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
    if let Some(run) = active.take() {
        let _ = run.handle.await;
    }
    drop(event_tx);
    let _ = writer.await;
}

async fn takeover_replay(
    active: &mut Option<ActiveRun>,
    registry: &Arc<crate::computer::session::ComputerRunRegistry>,
    run_id: &str,
    event_tx: &UnboundedSender<ComputerEvent>,
    config: Config,
    parsed: &serde_json::Value,
    msg: UserMessage,
) {
    let Some((provider, model)) = resolve_vision_route(parsed, &config) else {
        let _ = event_tx.send(ComputerEvent::error_code(
            "steer_requires_model",
            "steering a replay requires a vision provider/model; the replay continues",
        ));
        return;
    };
    let Some(run) = active.take() else {
        return;
    };
    run.mute.store(true, Ordering::Release);
    registry.cancel(run_id);
    let _ = run.handle.await;
    let (recording, task) = match run.kind {
        RunKind::Replay { recording, task } => (recording, task),
        RunKind::Agent => (String::new(), String::new()),
    };

    let goal = if task.trim().is_empty() {
        "Follow the user's live instructions.".to_string()
    } else {
        task
    };
    let params = RunParams {
        run_id: run_id.to_string(),
        task: goal,
        provider,
        model,
        max_steps: DEFAULT_MAX_STEPS,
        step_delay_ms: DEFAULT_STEP_DELAY_MS,
        reference_images: Vec::new(),
        initial_history: vec![format!(
            "Was replaying the recording '{recording}' when the user interrupted with a live \
             instruction; continue from the current screen state."
        )],
    };
    let (user_tx, user_rx) = mpsc::unbounded_channel::<UserMessage>();
    let _ = user_tx.send(msg);
    let cancel = registry.register(run_id);
    let mute = Arc::new(AtomicBool::new(false));
    let run_tx = spawn_event_forwarder(event_tx, mute.clone());
    let _ = event_tx.send(ComputerEvent::status_code(
        crate::computer::run::RunStatus::Running,
        "steer_takeover",
        format!("replay '{recording}' interrupted; continuing with the live agent"),
    ));
    let handle = tokio::spawn(run_loop(params, config, cancel, run_tx, user_rx));
    *active = Some(ActiveRun {
        handle,
        kind: RunKind::Agent,
        mute,
        user_tx: Some(user_tx),
    });
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
        reference_images: Vec::new(),
        initial_history: Vec::new(),
    })
}
