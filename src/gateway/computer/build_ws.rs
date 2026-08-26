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
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use std::path::PathBuf;
use tokio::sync::mpsc;

use crate::computer::build::{
    automation, catalogue::Architecture, engine, skill, BuiltAutomation,
};
use crate::computer::describe::load_analysis;
use crate::computer::vision::VisionClient;
use crate::config::Config;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum BuildEvent {
    Progress { phase: String, message: String },
    SkillPlan { plan: skill::SkillPlan },
    AutomationPlan { plan: automation::AutomationPlan },
    Built { placement: String, path: String },
    Error { message: String },
}

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

pub async fn handle_ws_build(
    State(state): State<AppState>,
    Path(rec_id): Path<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if let Some(reject) =
        crate::gateway::cors::reject_ws_disallowed_origin(&headers, "/ws/computer-build")
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
    ws.on_upgrade(move |socket| handle_socket_build(socket, state))
        .into_response()
}

async fn handle_socket_build(socket: WebSocket, state: AppState) {
    let (mut sink, mut receiver) = socket.split();
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<BuildEvent>();
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
        let msg_type = parsed.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if msg_type == "cancel" {
            if let Some(handle) = running.take() {
                handle.abort();
            }
            continue;
        }
        if running.as_ref().is_some_and(|h| !h.is_finished()) {
            let _ = event_tx.send(BuildEvent::Error {
                message: "a build step is already running".to_string(),
            });
            continue;
        }

        let name = parsed
            .get("recording")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let Some(dir) = recording_dir(&state, &name) else {
            let _ = event_tx.send(BuildEvent::Error {
                message: format!("recording '{name}' not found"),
            });
            continue;
        };
        let config: Config = state.live_config.load_ref().as_ref().clone();
        let tx = event_tx.clone();

        match msg_type {
            "propose" | "refine" => {
                let kind = parsed
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("skill")
                    .to_string();
                let architecture = parsed
                    .get("architecture")
                    .and_then(|v| v.as_str())
                    .and_then(Architecture::parse)
                    .unwrap_or(Architecture::SenAgent);
                let feedback = parsed
                    .get("feedback")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let Some((provider, model)) = super::resolve_vision_route(&parsed, &config) else {
                    let _ = tx.send(BuildEvent::Error {
                        message: "no vision provider/model configured".to_string(),
                    });
                    continue;
                };
                running = Some(tokio::spawn(async move {
                    run_propose(&config, &dir, &name, &kind, architecture, provider, model, feedback, &tx)
                        .await;
                }));
            }
            "create" => {
                let placement = parsed
                    .get("placement")
                    .and_then(|v| v.as_str())
                    .unwrap_or("install")
                    .to_string();
                let export_dir = parsed
                    .get("exportDir")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let plan_value = parsed.get("plan").cloned();
                let Some((provider, model)) = super::resolve_vision_route(&parsed, &config) else {
                    let _ = tx.send(BuildEvent::Error {
                        message: "no vision provider/model configured".to_string(),
                    });
                    continue;
                };
                let workspace = state.live_config.load_ref().workspace_dir.clone();
                running = Some(tokio::spawn(async move {
                    run_create(
                        &config, &workspace, &dir, &name, plan_value, &placement, export_dir,
                        provider, model, &tx,
                    )
                    .await;
                }));
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

#[allow(clippy::too_many_arguments)]
async fn run_propose(
    config: &Config,
    dir: &std::path::Path,
    _session_id: &str,
    kind: &str,
    architecture: Architecture,
    provider: String,
    model: String,
    feedback: Option<String>,
    tx: &mpsc::UnboundedSender<BuildEvent>,
) {
    let Some(analysis) = load_analysis(dir) else {
        let _ = tx.send(BuildEvent::Error {
            message: "analyze this recording before building".to_string(),
        });
        return;
    };
    let client = match VisionClient::from_config(config, &provider, &model) {
        Ok(client) => client,
        Err(e) => {
            let _ = tx.send(BuildEvent::Error {
                message: format!("failed to initialize model '{model}': {e}"),
            });
            return;
        }
    };
    let _ = tx.send(BuildEvent::Progress {
        phase: "planning".to_string(),
        message: "Planning…".to_string(),
    });
    if kind == "automation" {
        if !architecture.supports_automation() {
            let _ = tx.send(BuildEvent::Error {
                message: format!("{} does not support automations", architecture.label()),
            });
            return;
        }
        match engine::propose_automation_plan(&client, architecture, &analysis, feedback.as_deref())
            .await
        {
            Ok(plan) => {
                automation::save_plan(dir, &plan);
                let _ = tx.send(BuildEvent::AutomationPlan { plan });
            }
            Err(e) => {
                let _ = tx.send(BuildEvent::Error {
                    message: e.to_string(),
                });
            }
        }
    } else {
        match engine::propose_skill_plan(&client, architecture, &analysis, feedback.as_deref()).await
        {
            Ok(plan) => {
                skill::save_plan(dir, &plan);
                let _ = tx.send(BuildEvent::SkillPlan { plan });
            }
            Err(e) => {
                let _ = tx.send(BuildEvent::Error {
                    message: e.to_string(),
                });
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_create(
    config: &Config,
    workspace: &std::path::Path,
    dir: &std::path::Path,
    session_id: &str,
    plan_value: Option<serde_json::Value>,
    placement: &str,
    export_dir: Option<String>,
    provider: String,
    model: String,
    tx: &mpsc::UnboundedSender<BuildEvent>,
) {
    let is_automation = plan_value
        .as_ref()
        .and_then(|p| p.get("schedule"))
        .is_some();
    if is_automation {
        create_automation(config, workspace, dir, session_id, plan_value, placement, export_dir, tx)
            .await;
    } else {
        create_skill(config, dir, session_id, plan_value, placement, export_dir, provider, model, tx)
            .await;
    }
}

#[allow(clippy::too_many_arguments)]
async fn create_skill(
    config: &Config,
    dir: &std::path::Path,
    session_id: &str,
    plan_value: Option<serde_json::Value>,
    placement: &str,
    export_dir: Option<String>,
    provider: String,
    model: String,
    tx: &mpsc::UnboundedSender<BuildEvent>,
) {
    let architecture = plan_value
        .as_ref()
        .and_then(|p| p.get("architecture"))
        .and_then(|v| v.as_str())
        .and_then(Architecture::parse)
        .unwrap_or(Architecture::SenAgent);
    let plan = match plan_value.and_then(|p| serde_json::from_value::<skill::SkillPlan>(p).ok()) {
        Some(plan) => plan,
        None => match skill::load_plan(dir) {
            Some(plan) => plan,
            None => {
                let _ = tx.send(BuildEvent::Error {
                    message: "no plan to build from".to_string(),
                });
                return;
            }
        },
    };
    skill::save_plan(dir, &plan);
    let client = match VisionClient::from_config(config, &provider, &model) {
        Ok(client) => client,
        Err(e) => {
            let _ = tx.send(BuildEvent::Error {
                message: format!("failed to initialize model '{model}': {e}"),
            });
            return;
        }
    };
    let _ = tx.send(BuildEvent::Progress {
        phase: "drafting".to_string(),
        message: "Writing the skill…".to_string(),
    });
    let args = match engine::create_skill_body(&client, architecture, &plan).await {
        Ok(args) => args,
        Err(e) => {
            let _ = tx.send(BuildEvent::Error {
                message: e.to_string(),
            });
            return;
        }
    };
    let mut built = match skill::built_skill_from_submission(session_id, &plan, &args) {
        Ok(built) => built,
        Err(e) => {
            let _ = tx.send(BuildEvent::Error {
                message: e.to_string(),
            });
            return;
        }
    };
    let markdown = skill::render_skill_markdown(&built);

    let (placement_label, path) = if placement == "export" {
        let base = match export_dir {
            Some(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
            _ => {
                let _ = tx.send(BuildEvent::Error {
                    message: "export requires a destination folder".to_string(),
                });
                return;
            }
        };
        match write_export(&base, &built.name, &markdown).await {
            Ok(path) => ("export", path),
            Err(e) => {
                let _ = tx.send(BuildEvent::Error {
                    message: e.to_string(),
                });
                return;
            }
        }
    } else {
        let path = dir.join("SKILL.md");
        if let Err(e) = tokio::fs::write(&path, markdown.as_bytes()).await {
            let _ = tx.send(BuildEvent::Error {
                message: format!("failed to install skill: {e}"),
            });
            return;
        }
        ("install", path.to_string_lossy().to_string())
    };
    built.exported_path = Some(path.clone());
    skill::persist_built(dir, &built);
    let _ = tx.send(BuildEvent::Built {
        placement: placement_label.to_string(),
        path,
    });
}

#[allow(clippy::too_many_arguments)]
async fn create_automation(
    config: &Config,
    _workspace: &std::path::Path,
    dir: &std::path::Path,
    session_id: &str,
    plan_value: Option<serde_json::Value>,
    placement: &str,
    export_dir: Option<String>,
    tx: &mpsc::UnboundedSender<BuildEvent>,
) {
    let plan = match plan_value
        .and_then(|p| serde_json::from_value::<automation::AutomationPlan>(p).ok())
    {
        Some(plan) => plan,
        None => match automation::load_plan(dir) {
            Some(plan) => plan,
            None => {
                let _ = tx.send(BuildEvent::Error {
                    message: "no plan to build from".to_string(),
                });
                return;
            }
        },
    };
    automation::save_plan(dir, &plan);
    let mut built: BuiltAutomation = match automation::built_from_plan(session_id, &plan) {
        Ok(built) => built,
        Err(e) => {
            let _ = tx.send(BuildEvent::Error {
                message: e.to_string(),
            });
            return;
        }
    };

    let (placement_label, path) = if placement == "export" {
        let base = match export_dir {
            Some(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
            _ => {
                let _ = tx.send(BuildEvent::Error {
                    message: "export requires a destination folder".to_string(),
                });
                return;
            }
        };
        let json = automation::render_automation_json(&built);
        match write_export_named(&base, &built.name, "automation.json", &json).await {
            Ok(path) => ("export", path),
            Err(e) => {
                let _ = tx.send(BuildEvent::Error {
                    message: e.to_string(),
                });
                return;
            }
        }
    } else {
        let schedule = crate::cron::Schedule::Cron {
            expr: built.schedule.to_cron_expr(),
            tz: None,
        };
        let prompt = automation::rendered_prompt(&built);
        let opts = crate::cron::AgentJobOptions {
            model: if built.model.trim().is_empty() {
                None
            } else {
                Some(built.model.clone())
            },
            task_description: Some(built.description.clone()),
            ..crate::cron::AgentJobOptions::default()
        };
        match crate::cron::add_agent_job(config, Some(built.name.clone()), schedule, &prompt, opts) {
            Ok(job) => ("install", job.id),
            Err(e) => {
                let _ = tx.send(BuildEvent::Error {
                    message: format!("failed to schedule automation: {e}"),
                });
                return;
            }
        }
    };
    built.exported_path = Some(path.clone());
    automation::persist_built(dir, &built);
    let _ = tx.send(BuildEvent::Built {
        placement: placement_label.to_string(),
        path,
    });
}

async fn write_export(base: &std::path::Path, name: &str, markdown: &str) -> anyhow::Result<String> {
    write_export_named(base, name, "SKILL.md", markdown).await
}

async fn write_export_named(
    base: &std::path::Path,
    name: &str,
    file: &str,
    contents: &str,
) -> anyhow::Result<String> {
    let mut dir = base.join(name);
    let mut counter = 2u32;
    while tokio::fs::try_exists(&dir).await.unwrap_or(false) {
        dir = base.join(format!("{name}-{counter}"));
        counter += 1;
    }
    tokio::fs::create_dir_all(&dir).await?;
    let path = dir.join(file);
    tokio::fs::write(&path, contents.as_bytes()).await?;
    Ok(path.to_string_lossy().to_string())
}
