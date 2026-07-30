// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use axum::{
    Json, Router,
    extract::{Path, Query},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::session::event::SessionEvent;
use crate::workers::events::{WorkerMeta, WorkerSummary};
use crate::workers::persistence::{find_worker_root, list_meta, read_meta, replay_worker_events};
use crate::workers::supervisor::{candidate_worker_roots, global_supervisor};

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub workers: Vec<WorkerSummary>,
}

#[derive(Debug, Serialize)]
pub struct DetailResponse {
    pub meta: WorkerMeta,

    pub summary: Option<WorkerSummary>,
}

#[derive(Debug, Serialize)]
pub struct EventsResponse {
    pub worker_id: String,
    pub events: Vec<SessionEvent>,
}

#[derive(Debug, Serialize)]
pub struct CancelResponse {
    pub worker_id: String,
    pub cancelled: bool,
}

pub fn router() -> Router {
    Router::new()
        .route("/api/workers", get(handle_list))
        .route("/api/workers/{id}", get(handle_get))
        .route("/api/workers/{id}/cancel", post(handle_cancel))
        .route("/api/workers/{id}/events", get(handle_events))
        .route("/ws/worker/{id}", get(crate::workers::ws::handle_ws_worker))
}

async fn handle_list(Query(q): Query<ListQuery>) -> impl IntoResponse {
    let supervisor = global_supervisor();

    let mut summaries: Vec<WorkerSummary> = Vec::new();

    let roots = candidate_worker_roots();

    let metas = {
        tokio::task::spawn_blocking(move || {
            let mut out: Vec<WorkerMeta> = Vec::new();
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            for root in &roots {
                for meta in list_meta(root).unwrap_or_default() {
                    if seen.insert(meta.worker_id.clone()) {
                        out.push(meta);
                    }
                }
            }
            out
        })
        .await
        .unwrap_or_default()
    };

    if let Some(parent) = q.session_id.as_deref() {
        if let Some(sup) = supervisor.as_ref() {
            summaries = sup.list_by_parent(parent);
        }
        let known: std::collections::HashSet<String> =
            summaries.iter().map(|s| s.worker_id.clone()).collect();
        for meta in &metas {
            if meta.parent_session_id != parent {
                continue;
            }
            if known.contains(&meta.worker_id) {
                continue;
            }
            summaries.push(meta.to_summary());
        }
    } else {
        if let Some(sup) = supervisor.as_ref() {
            summaries.extend(sup.all_summaries());
        }
        let known: std::collections::HashSet<String> =
            summaries.iter().map(|s| s.worker_id.clone()).collect();
        for meta in &metas {
            if known.contains(&meta.worker_id) {
                continue;
            }
            summaries.push(meta.to_summary());
        }
    }

    summaries.sort_by_key(|s| s.started_at);
    (StatusCode::OK, Json(ListResponse { workers: summaries })).into_response()
}

async fn handle_get(Path(id): Path<String>) -> impl IntoResponse {
    let supervisor = global_supervisor();

    let summary = supervisor.as_ref().and_then(|s| s.summary_for(&id));

    let read_result = {
        let roots = candidate_worker_roots();
        let live_root = supervisor
            .as_ref()
            .and_then(|s| s.get(&id))
            .map(|h| h.workspace_root.clone());
        let id = id.clone();
        tokio::task::spawn_blocking(move || {
            let root = live_root
                .or_else(|| find_worker_root(&roots, &id))
                .or_else(|| roots.first().cloned())
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            read_meta(&root, &id)
        })
        .await
        .unwrap_or_else(|e| Err(std::io::Error::other(e.to_string())))
    };

    let meta = match read_result {
        Ok(Some(m)) => m,
        Ok(None) => {
            if let Some(s) = &summary {
                WorkerMeta {
                    worker_id: s.worker_id.clone(),
                    parent_session_id: s.parent_session_id.clone(),
                    parent_tool_use_id: s.parent_tool_use_id.clone(),
                    title: s.title.clone(),
                    prompt: String::new(),
                    context: None,
                    model: s.model.clone(),
                    status: s.status,
                    last_action: s.last_action.clone(),
                    last_detail: s.last_detail.clone(),
                    started_at: s.started_at,
                    finished_at: s.finished_at,
                    output: None,
                    error: None,
                    workspace_dir: None,
                    resume_count: 0,
                }
            } else {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({ "error": format!("worker '{id}' not found") })),
                )
                    .into_response();
            }
        }
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": err.to_string() })),
            )
                .into_response();
        }
    };

    (StatusCode::OK, Json(DetailResponse { meta, summary })).into_response()
}

async fn handle_cancel(Path(id): Path<String>) -> impl IntoResponse {
    let supervisor = match global_supervisor() {
        Some(s) => s,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "worker supervisor not initialised" })),
            )
                .into_response();
        }
    };

    let cancelled = supervisor.cancel(&id);
    (
        StatusCode::OK,
        Json(CancelResponse {
            worker_id: id,
            cancelled,
        }),
    )
        .into_response()
}

async fn handle_events(Path(id): Path<String>) -> impl IntoResponse {
    let supervisor = global_supervisor();

    let events = {
        let roots = candidate_worker_roots();
        let live_root = supervisor
            .as_ref()
            .and_then(|s| s.get(&id))
            .map(|h| h.workspace_root.clone());
        let id = id.clone();
        tokio::task::spawn_blocking(move || {
            let root = live_root
                .or_else(|| find_worker_root(&roots, &id))
                .or_else(|| roots.first().cloned())
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            replay_worker_events(&root, &id)
        })
        .await
        .unwrap_or_else(|e| Err(std::io::Error::other(e.to_string())))
    };

    let events = match events {
        Ok(evts) => evts.into_iter().map(|record| record.event).collect(),
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": err.to_string() })),
            )
                .into_response();
        }
    };
    (
        StatusCode::OK,
        Json(EventsResponse {
            worker_id: id,
            events,
        }),
    )
        .into_response()
}
