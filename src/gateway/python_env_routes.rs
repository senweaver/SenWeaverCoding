// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::AppState;
use super::api::require_auth;
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        sse::{Event as SseEvent, KeepAlive, Sse},
        IntoResponse, Json,
    },
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::path::PathBuf;
use std::time::Duration;
use tokio_stream::wrappers::ReceiverStream;

use crate::python_env;

#[derive(Debug, Deserialize)]
pub struct WorkspaceQuery {
    pub workspace: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateBody {
    pub workspace: String,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default, rename = "pythonVersion")]
    pub python_version: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SelectBody {
    pub workspace: String,
    #[serde(rename = "interpreterPath")]
    pub interpreter_path: String,
}

#[derive(Debug, Deserialize)]
pub struct InstallBody {
    pub workspace: String,
    #[serde(default)]
    pub file: Option<String>,
}

async fn resolve_workspace(state: &AppState, requested: &str) -> Option<PathBuf> {
    let trimmed = requested.trim();
    if trimmed.is_empty() {
        return None;
    }
    let state = state.clone();
    let requested = trimmed.to_string();
    tokio::task::spawn_blocking(move || {
        let p = PathBuf::from(&requested);
        let canonical = p.canonicalize().unwrap_or(p);
        let workspace_root = state.config.lock().workspace_dir.clone();
        if let Ok(ws) = workspace_root.canonicalize() {
            if ws == canonical {
                return Some(canonical);
            }
        } else if workspace_root == canonical {
            return Some(canonical);
        }
        if let Some(backend) = state.session_backend.as_ref() {
            for meta in backend.list_sessions_with_metadata() {
                let Some(wd) = meta.work_dir.as_deref() else {
                    continue;
                };
                let trimmed_wd = wd.trim();
                if trimmed_wd.is_empty() {
                    continue;
                }
                if let Ok(rp) = PathBuf::from(trimmed_wd).canonicalize() {
                    if rp == canonical {
                        return Some(canonical);
                    }
                }
            }
        }
        None
    })
    .await
    .ok()
    .flatten()
}

fn forbid_workspace() -> axum::response::Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({"error": "Workspace root is not in the allowed list"})),
    )
        .into_response()
}

fn state_to_json(state: &python_env::PythonEnvState) -> Value {
    json!({
        "workspace": state.workspace.to_string_lossy(),
        "interpreterPath": state.interpreter_path.as_ref().map(|p| p.to_string_lossy().to_string()),
        "version": state.version,
        "tool": match state.tool {
            python_env::PythonInterpreterTool::Uv => "uv",
            python_env::PythonInterpreterTool::Venv => "venv",
            python_env::PythonInterpreterTool::System => "system",
            python_env::PythonInterpreterTool::Unknown => "unknown",
        },
        "isIsolated": state.is_isolated,
        "packagesCount": state.packages_count,
        "lastUpdatedMs": state.last_updated_ms,
        "lastError": state.last_error,
        "isPythonProject": state.is_python_project,
    })
}

pub async fn handle_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<WorkspaceQuery>,
) -> axum::response::Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(workspace) = resolve_workspace(&state, &q.workspace).await else {
        return forbid_workspace();
    };
    let env_state = python_env::manager::refresh_status(&workspace).await;
    let required = python_env::read_required_python(&workspace);
    let recommend = python_env::recommend_install_strategy(&workspace);
    let mut payload = state_to_json(&env_state);
    if let Some(obj) = payload.as_object_mut() {
        obj.insert(
            "requiredPython".to_string(),
            serde_json::to_value(&required).unwrap_or(Value::Null),
        );
        obj.insert(
            "installRecommendation".to_string(),
            serde_json::to_value(&recommend).unwrap_or(Value::Null),
        );
    }
    Json(payload).into_response()
}

pub async fn handle_discover(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<WorkspaceQuery>,
) -> axum::response::Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(workspace) = resolve_workspace(&state, &q.workspace).await else {
        return forbid_workspace();
    };
    let items = python_env::discover_interpreters(&workspace).await;
    let json_items: Vec<Value> = items
        .into_iter()
        .map(|i| {
            json!({
                "path": i.path.to_string_lossy(),
                "version": i.version,
                "source": i.source,
                "isVenv": i.is_venv,
            })
        })
        .collect();
    let markers = python_env::detect_workspace_project(&workspace);
    let required = python_env::read_required_python(&workspace);
    let recommend = python_env::recommend_install_strategy(&workspace);
    Json(json!({
        "interpreters": json_items,
        "markers": {
            "isPythonProject": markers.is_python_project(),
            "hasVenvDir": markers.has_venv_dir,
            "hasPyproject": markers.has_pyproject,
            "hasRequirements": markers.has_requirements,
            "hasPipfile": markers.has_pipfile,
            "hasSetupPy": markers.has_setup_py,
            "hasSetupCfg": markers.has_setup_cfg,
            "hasPythonVersionFile": markers.has_python_version_file,
            "hasUvLock": markers.has_uv_lock,
        },
        "requiredPython": required,
        "installRecommendation": recommend,
    })).into_response()
}

pub async fn handle_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateBody>,
) -> axum::response::Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(workspace) = resolve_workspace(&state, &body.workspace).await else {
        return forbid_workspace();
    };
    let tool = body.tool.as_deref().map(str::to_ascii_lowercase);
    let create_tool = match tool.as_deref() {
        Some("uv") => Some(python_env::CreateTool::Uv),
        Some("venv") => Some(python_env::CreateTool::Venv),
        Some(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "tool must be 'uv' or 'venv'"})),
            )
                .into_response();
        }
        None => None,
    };
    let workspace_for_task = workspace.clone();
    let py_version = body.python_version.clone();
    crate::runtime::spawn_supervised("python_env.create_venv", async move {
        let _ = python_env::create_venv(&workspace_for_task, create_tool, py_version.as_deref())
            .await;
    });
    Json(json!({
        "accepted": true,
        "workspace": workspace.to_string_lossy(),
    }))
    .into_response()
}

pub async fn handle_select(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SelectBody>,
) -> axum::response::Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(workspace) = resolve_workspace(&state, &body.workspace).await else {
        return forbid_workspace();
    };
    let interpreter = PathBuf::from(body.interpreter_path.trim());
    match python_env::select_interpreter(&workspace, &interpreter) {
        Ok(state) => Json(state_to_json(&state)).into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": err})),
        )
            .into_response(),
    }
}

pub async fn handle_install_requirements(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<InstallBody>,
) -> axum::response::Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(workspace) = resolve_workspace(&state, &body.workspace).await else {
        return forbid_workspace();
    };
    let file = body.file.clone();
    let workspace_for_task = workspace.clone();
    crate::runtime::spawn_supervised("python_env.install_requirements", async move {
        let _ = python_env::manager::install_requirements(
            &workspace_for_task,
            file.as_deref(),
        )
        .await;
    });
    Json(json!({"accepted": true})).into_response()
}

#[derive(Debug, Deserialize)]
pub struct WorkspaceBody {
    pub workspace: String,
}

pub async fn handle_install_smart(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<WorkspaceBody>,
) -> axum::response::Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(workspace) = resolve_workspace(&state, &body.workspace).await else {
        return forbid_workspace();
    };
    let workspace_for_task = workspace.clone();
    crate::runtime::spawn_supervised("python_env.install_smart", async move {
        let _ = python_env::manager::install_with_strategy(&workspace_for_task).await;
    });
    Json(json!({"accepted": true})).into_response()
}

pub async fn handle_purge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<WorkspaceBody>,
) -> axum::response::Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(workspace) = resolve_workspace(&state, &body.workspace).await else {
        return forbid_workspace();
    };
    match python_env::manager::purge_venv(&workspace) {
        Ok(_) => Json(json!({"success": true})).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": err})),
        )
            .into_response(),
    }
}

pub async fn handle_activation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<WorkspaceQuery>,
) -> axum::response::Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(workspace) = resolve_workspace(&state, &q.workspace).await else {
        return forbid_workspace();
    };
    let kv = python_env::activation_env(&workspace);
    let mut env_obj = serde_json::Map::new();
    for (k, v) in kv {
        env_obj.insert(k, Value::String(v));
    }
    Json(json!({
        "env": Value::Object(env_obj),
        "unset": ["PYTHONHOME"],
    }))
    .into_response()
}

pub async fn handle_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<WorkspaceQuery>,
) -> axum::response::Response {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(workspace) = resolve_workspace(&state, &q.workspace).await else {
        return forbid_workspace();
    };
    let mut bus = python_env::subscribe_events();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<SseEvent, Infallible>>(32);
    let workspace_filter = workspace.clone();
    crate::runtime::spawn_supervised("python_env.events_stream", async move {
        if let Ok(payload) = serde_json::to_string(&json!({
            "kind": "snapshot",
            "state": state_to_json(&python_env::manager::status_for(&workspace_filter)),
        })) {
            let _ = tx
                .send(Ok(SseEvent::default().event("python-env").data(payload)))
                .await;
        }
        loop {
            match bus.recv().await {
                Ok(event) => {
                    let event_workspace = match &event {
                        python_env::PythonEnvEvent::Creating { workspace, .. }
                        | python_env::PythonEnvEvent::Progress { workspace, .. }
                        | python_env::PythonEnvEvent::Ready { workspace, .. }
                        | python_env::PythonEnvEvent::Failed { workspace, .. }
                        | python_env::PythonEnvEvent::InstallStart { workspace, .. }
                        | python_env::PythonEnvEvent::InstallProgress { workspace, .. }
                        | python_env::PythonEnvEvent::InstallDone { workspace, .. }
                        | python_env::PythonEnvEvent::PackagesCounted { workspace, .. }
                        | python_env::PythonEnvEvent::Purged { workspace, .. } => workspace,
                    };
                    let matches = event_workspace == &workspace_filter;
                    if !matches {
                        continue;
                    }
                    let Ok(payload) = serde_json::to_string(&event) else {
                        continue;
                    };
                    if tx
                        .send(Ok(SseEvent::default().event("python-env").data(payload)))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });
    let stream = ReceiverStream::new(rx);
    Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keep-alive"),
        )
        .into_response()
}
