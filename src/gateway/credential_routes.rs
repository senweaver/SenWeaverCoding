// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
};
use serde::Deserialize;

use super::api::require_auth;
use super::AppState;
use crate::services::credential_vault::{
    init_credential_vault, try_get_credential_vault, CredentialKind, CredentialMeta,
};

fn ensure_vault(
    state: &AppState,
) -> Result<std::sync::Arc<crate::services::credential_vault::CredentialVault>, axum::response::Response>
{
    if let Some(v) = try_get_credential_vault() {
        return Ok(v);
    }
    let config = state.config.lock().clone();
    let anchor = if config.workspace_dir.as_os_str().is_empty() {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    } else {
        config.workspace_dir.clone()
    };
    match init_credential_vault(&anchor) {
        Ok(v) => Ok(v),
        Err(err) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "vault_unavailable",
                "detail": err.to_string(),
            })),
        )
            .into_response()),
    }
}

fn meta_to_json(meta: &CredentialMeta) -> serde_json::Value {
    serde_json::json!({
        "name": meta.name,
        "kind": meta.kind,
        "created_at": meta.created_at,
        "updated_at": meta.updated_at,
    })
}

pub async fn handle_list(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let vault = match ensure_vault(&state) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let items: Vec<serde_json::Value> = vault.list().iter().map(meta_to_json).collect();
    Json(serde_json::json!({ "credentials": items })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct PutBody {
    pub name: String,
    pub kind: Option<String>,
    pub value: String,
}

pub async fn handle_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PutBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let vault = match ensure_vault(&state) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let kind = body
        .kind
        .as_deref()
        .map(CredentialKind::parse)
        .unwrap_or(CredentialKind::Other);
    match vault.put(&body.name, kind, &body.value) {
        Ok(meta) => Json(serde_json::json!({
            "status": "ok",
            "credential": meta_to_json(&meta),
        }))
        .into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid",
                "detail": err.to_string(),
            })),
        )
            .into_response(),
    }
}

pub async fn handle_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let vault = match ensure_vault(&state) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match vault.delete(&name) {
        Ok(true) => Json(serde_json::json!({ "status": "deleted", "name": name })).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "not_found",
                "name": name,
            })),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "delete_failed",
                "detail": err.to_string(),
            })),
        )
            .into_response(),
    }
}
