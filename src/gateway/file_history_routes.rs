// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::AppState;
use super::api::require_auth;
use super::workspace_files::{allowed_workspace_root, resolve_within};
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

const GW_SESSION_PREFIX: &str = "gw_";
const MAX_SNAPSHOT_PREVIEW_BYTES: usize = 4 * 1024 * 1024;

type SessionBackendArc = Arc<dyn crate::channels::session::backend::SessionBackend>;

fn lookup_session_name(backend: &SessionBackendArc, key: &str) -> Option<String> {
    backend
        .get_session_name(key)
        .ok()
        .flatten()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
}

fn resolve_session_name(
    backend: Option<&SessionBackendArc>,
    cache: &mut HashMap<String, Option<String>>,
    session_id: &str,
) -> Option<String> {
    if let Some(cached) = cache.get(session_id) {
        return cached.clone();
    }
    let name = backend.and_then(|be| {
        lookup_session_name(be, session_id).or_else(|| {
            if session_id.starts_with(GW_SESSION_PREFIX) {
                None
            } else {
                lookup_session_name(be, &format!("{GW_SESSION_PREFIX}{session_id}"))
            }
        })
    });
    cache.insert(session_id.to_string(), name.clone());
    name
}

#[derive(Debug, Deserialize)]
pub struct HistoryFilesQuery {
    pub root: String,
}

pub async fn handle_file_history_files(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<HistoryFilesQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let root = match allowed_workspace_root(&state, &q.root).await {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let files = tokio::task::spawn_blocking(move || {
        let history = crate::tools::edit_history::EditHistory::shared_for_workspace(&root);
        history
            .files_summary()
            .into_iter()
            .map(|(rel_path, count, last_timestamp)| {
                json!({
                    "relPath": rel_path,
                    "count": count,
                    "lastTimestamp": last_timestamp,
                })
            })
            .collect::<Vec<serde_json::Value>>()
    })
    .await
    .unwrap_or_default();

    Json(json!({ "files": files })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct HistoryListQuery {
    pub root: String,
    pub path: String,
}

pub async fn handle_file_history_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<HistoryListQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let root = match allowed_workspace_root(&state, &q.root).await {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let target = match resolve_within(&root, &q.path, false) {
        Ok(t) => t,
        Err(e) => return e.into_response(),
    };
    let backend = state.session_backend.clone();
    let rel_path = q.path.clone();
    let entries = tokio::task::spawn_blocking(move || {
        let history = crate::tools::edit_history::EditHistory::shared_for_workspace(&root);
        let rows = history.file_history_with_batches(&target);
        let mut name_cache: HashMap<String, Option<String>> = HashMap::new();
        let mut batch_cache: HashMap<String, Option<String>> = HashMap::new();
        rows.into_iter()
            .enumerate()
            .map(|(index, (snap, batch_id))| {
                let session_id = snap.session_id.clone().or_else(|| {
                    let batch = batch_id.as_deref()?;
                    let be = backend.as_ref()?;
                    batch_cache
                        .entry(batch.to_string())
                        .or_insert_with(|| be.session_for_edit_batch(batch))
                        .clone()
                });
                let session_name = session_id.as_deref().and_then(|sid| {
                    resolve_session_name(backend.as_ref(), &mut name_cache, sid)
                });
                json!({
                    "index": index,
                    "timestamp": snap.timestamp,
                    "toolName": snap.tool_name,
                    "description": snap.description,
                    "byteSize": snap.byte_size,
                    "absent": snap.absent,
                    "sha256": snap.sha256,
                    "sessionId": session_id,
                    "sessionName": session_name,
                })
            })
            .collect::<Vec<serde_json::Value>>()
    })
    .await
    .unwrap_or_default();

    Json(json!({ "relPath": rel_path, "entries": entries })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct HistorySnapshotQuery {
    pub root: String,
    pub path: String,
    pub index: usize,
}

pub async fn handle_file_history_snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<HistorySnapshotQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let root = match allowed_workspace_root(&state, &q.root).await {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let target = match resolve_within(&root, &q.path, false) {
        Ok(t) => t,
        Err(e) => return e.into_response(),
    };
    let index = q.index;
    let result = tokio::task::spawn_blocking(move || {
        let history = crate::tools::edit_history::EditHistory::shared_for_workspace(&root);
        let chain = history.get_file_history(&target);
        let Some(snap) = chain.get(index).cloned() else {
            return Err((
                StatusCode::NOT_FOUND,
                "Snapshot index out of range".to_string(),
            ));
        };
        if snap.absent {
            return Ok(json!({
                "content": "",
                "absent": true,
                "binary": false,
                "tooLarge": false,
            }));
        }
        let bytes = history.read_blob(&snap.sha256).map_err(|e| {
            (StatusCode::NOT_FOUND, e.to_string())
        })?;
        if bytes.len() > MAX_SNAPSHOT_PREVIEW_BYTES {
            return Ok(json!({
                "content": "",
                "absent": false,
                "binary": false,
                "tooLarge": true,
            }));
        }
        match String::from_utf8(bytes) {
            Ok(content) => Ok(json!({
                "content": content,
                "absent": false,
                "binary": false,
                "tooLarge": false,
            })),
            Err(_) => Ok(json!({
                "content": "",
                "absent": false,
                "binary": true,
                "tooLarge": false,
            })),
        }
    })
    .await
    .unwrap_or_else(|join_err| {
        Err((StatusCode::INTERNAL_SERVER_ERROR, join_err.to_string()))
    });

    match result {
        Ok(body) => Json(body).into_response(),
        Err((status, message)) => {
            (status, Json(json!({ "error": message }))).into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRevertBody {
    pub root: String,
    pub path: String,
    pub index: usize,
    #[serde(default)]
    pub expected_sha256: Option<String>,
}

pub async fn handle_file_history_revert(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<HistoryRevertBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let root = match allowed_workspace_root(&state, &body.root).await {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let target = match resolve_within(&root, &body.path, false) {
        Ok(t) => t,
        Err(e) => return e.into_response(),
    };
    let rel_path = body.path.clone();
    let index = body.index;
    let expected_sha256 = body.expected_sha256.clone();
    let result = tokio::task::spawn_blocking(move || {
        let history = crate::tools::edit_history::EditHistory::shared_for_workspace(&root);
        history.restore_snapshot_with_stash(
            &target,
            index,
            expected_sha256.as_deref(),
            "user_restore",
            "restore from file history",
        )
    })
    .await
    .unwrap_or_else(|join_err| Err(anyhow::anyhow!(join_err.to_string())));

    match result {
        Ok(()) => Json(json!({ "ok": true, "relPath": rel_path })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
