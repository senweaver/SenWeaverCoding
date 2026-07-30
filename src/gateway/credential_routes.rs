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
use crate::services::governance::credential_vault::{
    init_credential_vault, try_get_credential_vault, CredentialField, CredentialKind,
    CredentialMeta,
};

fn ensure_vault(
    state: &AppState,
) -> Result<
    std::sync::Arc<crate::services::governance::credential_vault::CredentialVault>,
    Box<axum::response::Response>,
> {
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
        Err(err) => Err(Box::new(
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "vault_unavailable",
                    "detail": err.to_string(),
                })),
            )
                .into_response(),
        )),
    }
}

fn meta_to_json(meta: &CredentialMeta) -> serde_json::Value {
    serde_json::json!({
        "name": meta.name,
        "kind": meta.kind,
        "created_at": meta.created_at,
        "updated_at": meta.updated_at,
        "shape": meta.shape,
        "fields": meta.fields.iter().map(|f| serde_json::json!({
            "key": f.key,
            "kind": f.kind,
        })).collect::<Vec<_>>(),
    })
}

pub async fn handle_list(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let vault = match ensure_vault(&state) {
        Ok(v) => v,
        Err(resp) => return *resp,
    };
    let items: Vec<serde_json::Value> =
        tokio::task::spawn_blocking(move || vault.list().iter().map(meta_to_json).collect())
            .await
            .unwrap_or_default();
    Json(serde_json::json!({ "credentials": items })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct PutFieldBody {
    pub key: String,
    pub kind: Option<String>,
    pub value: String,
}

#[derive(Debug, Deserialize)]
pub struct PutBody {
    pub name: String,
    pub kind: Option<String>,
    pub value: Option<String>,
    #[serde(default)]
    pub fields: Option<Vec<PutFieldBody>>,
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
        Err(resp) => return *resp,
    };
    let name = body.name.clone();
    let result = if let Some(fields_body) = body.fields.filter(|f| !f.is_empty()) {
        let fields: Vec<CredentialField> = fields_body
            .into_iter()
            .map(|f| CredentialField {
                key: f.key,
                kind: f
                    .kind
                    .as_deref()
                    .map(CredentialKind::parse)
                    .unwrap_or(CredentialKind::Other),
                value: f.value,
            })
            .collect();
        tokio::task::spawn_blocking(move || vault.put_group(&name, fields)).await
    } else {
        let value = match body.value {
            Some(v) if !v.is_empty() => v,
            _ => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": "invalid",
                        "detail": "either non-empty value or non-empty fields is required",
                    })),
                )
                    .into_response();
            }
        };
        let kind = body
            .kind
            .as_deref()
            .map(CredentialKind::parse)
            .unwrap_or(CredentialKind::Other);
        tokio::task::spawn_blocking(move || vault.put(&name, kind, &value)).await
    };
    match result {
        Ok(Ok(meta)) => Json(serde_json::json!({
            "status": "ok",
            "credential": meta_to_json(&meta),
        }))
        .into_response(),
        Ok(Err(err)) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid",
                "detail": err.to_string(),
            })),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "vault_unavailable",
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
        Err(resp) => return *resp,
    };
    let name_for_io = name.clone();
    let result = tokio::task::spawn_blocking(move || vault.delete(&name_for_io)).await;
    match result {
        Ok(Ok(true)) => {
            Json(serde_json::json!({ "status": "deleted", "name": name })).into_response()
        }
        Ok(Ok(false)) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "not_found",
                "name": name,
            })),
        )
            .into_response(),
        Ok(Err(err)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "delete_failed",
                "detail": err.to_string(),
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
