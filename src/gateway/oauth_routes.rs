// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
};
use serde::Deserialize;

use super::api::require_auth;
use super::AppState;
use crate::services::oauth::OAuthProviderConfig;
use crate::services::try_get_services;

#[derive(Debug, Deserialize)]
pub struct StartBody {
    pub provider: String,
    #[serde(default)]
    pub pkce: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct RegisterBody {
    pub provider_name: String,
    pub client_id: String,
    pub auth_url: String,
    pub token_url: String,
    pub scopes: Vec<String>,
    pub redirect_uri: String,
    #[serde(default)]
    pub client_secret: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

pub async fn handle_list_providers(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(svc) = try_get_services() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "services_unavailable" })),
        )
            .into_response();
    };
    let providers = svc.oauth.list_providers().await;
    let mut items = Vec::new();
    for name in providers {
        let authenticated = svc.oauth.is_authenticated(&name).await;
        items.push(serde_json::json!({
            "provider": name,
            "authenticated": authenticated,
        }));
    }
    Json(serde_json::json!({ "providers": items })).into_response()
}

pub async fn handle_register_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RegisterBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(svc) = try_get_services() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "services_unavailable" })),
        )
            .into_response();
    };
    svc.oauth
        .register_provider(OAuthProviderConfig {
            provider_name: body.provider_name.clone(),
            client_id: body.client_id,
            auth_url: body.auth_url,
            token_url: body.token_url,
            scopes: body.scopes,
            redirect_uri: body.redirect_uri,
            client_secret: body.client_secret,
        })
        .await;
    Json(serde_json::json!({ "ok": true, "provider": body.provider_name })).into_response()
}

pub async fn handle_start(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<StartBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(svc) = try_get_services() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "services_unavailable" })),
        )
            .into_response();
    };
    let enable_pkce = body.pkce.unwrap_or(true);
    match svc
        .oauth
        .start_auth_flow_with_pkce(&body.provider, enable_pkce)
        .await
    {
        Ok(url) => Json(serde_json::json!({ "authorization_url": url, "provider": body.provider }))
            .into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

pub async fn handle_callback(
    State(state): State<AppState>,
    Query(q): Query<CallbackQuery>,
) -> impl IntoResponse {
    let _ = &state;
    if let Some(err) = q.error {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err })),
        )
            .into_response();
    }
    let (Some(code), Some(state_token)) = (q.code.as_deref(), q.state.as_deref()) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "missing code or state" })),
        )
            .into_response();
    };
    let Some(svc) = try_get_services() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "services_unavailable" })),
        )
            .into_response();
    };
    match svc.oauth.exchange_code_and_complete(state_token, code).await {
        Ok(provider) => Json(serde_json::json!({ "ok": true, "provider": provider })).into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

pub async fn handle_get_tokens(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(svc) = try_get_services() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "services_unavailable" })),
        )
            .into_response();
    };
    match svc.oauth.get_tokens(&provider).await {
        Some(tokens) => Json(serde_json::json!({
            "provider": provider,
            "authenticated": !tokens.is_expired(),
            "token_type": tokens.token_type,
            "expires_at_epoch_ms": tokens.expires_at_epoch_ms,
            "scope": tokens.scope,
            "has_refresh_token": tokens.refresh_token.is_some(),
            "access_token_preview": tokens.access_token.chars().take(8).collect::<String>(),
        }))
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "not_authenticated" })),
        )
            .into_response(),
    }
}

pub async fn handle_clear_tokens(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(svc) = try_get_services() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "services_unavailable" })),
        )
            .into_response();
    };
    svc.oauth.clear_tokens(&provider).await;
    Json(serde_json::json!({ "ok": true, "provider": provider })).into_response()
}
