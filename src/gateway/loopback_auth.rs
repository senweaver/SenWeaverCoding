// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::IntoResponse;
use std::path::Path;
use std::sync::OnceLock;

use super::AppState;

pub const TOKEN_ENV: &str = "SEN_GATEWAY_TOKEN";
pub const TOKEN_HEADER: &str = "x-sen-gateway-token";
pub const TOKEN_FILE_NAME: &str = "gateway.token";

pub fn loopback_token() -> &'static str {
    static TOKEN: OnceLock<String> = OnceLock::new();
    TOKEN.get_or_init(|| {
        if let Some(t) = crate::util::get_runtime_var(TOKEN_ENV) {
            let trimmed = t.trim().to_string();
            if !trimmed.is_empty() {
                return trimmed;
            }
        }
        format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        )
    })
}

pub fn persist_token_file(config_dir: &Path) {
    let path = config_dir.join(TOKEN_FILE_NAME);
    let token = loopback_token();
    if std::fs::read_to_string(&path)
        .map(|existing| existing.trim() == token)
        .unwrap_or(false)
    {
        return;
    }
    if let Err(err) = std::fs::write(&path, token) {
        tracing::warn!(
            path = %path.display(),
            error = %err,
            "failed to persist gateway loopback token file"
        );
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
}

pub fn read_token_file(config_dir: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(config_dir.join(TOKEN_FILE_NAME)).ok()?;
    let trimmed = raw.trim().to_string();
    if trimmed.is_empty() { None } else { Some(trimmed) }
}

static PATH_PREFIX: OnceLock<String> = OnceLock::new();

pub fn set_path_prefix(prefix: &str) {
    let _ = PATH_PREFIX.set(prefix.to_string());
}

fn effective_path(uri: &Uri) -> &str {
    let path = uri.path();
    if let Some(prefix) = PATH_PREFIX.get() {
        if !prefix.is_empty() {
            if let Some(stripped) = path.strip_prefix(prefix.as_str()) {
                if stripped.is_empty() {
                    return "/";
                }
                if stripped.starts_with('/') {
                    return stripped;
                }
            }
        }
    }
    path
}

fn query_token(uri: &Uri) -> Option<&str> {
    uri.query()?
        .split('&')
        .find_map(|pair| pair.strip_prefix("token="))
        .filter(|t| !t.is_empty())
}

pub fn request_matches(headers: &HeaderMap, query_token: Option<&str>) -> bool {
    let expected = loopback_token();
    if let Some(t) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|auth| auth.strip_prefix("Bearer "))
    {
        if t == expected {
            return true;
        }
    }
    if let Some(t) = headers.get(TOKEN_HEADER).and_then(|v| v.to_str().ok()) {
        if t.trim() == expected {
            return true;
        }
    }
    if let Some(t) = headers
        .get("sec-websocket-protocol")
        .and_then(|v| v.to_str().ok())
        .and_then(|protos| {
            protos
                .split(',')
                .map(str::trim)
                .find_map(|p| p.strip_prefix("bearer."))
        })
    {
        if t == expected {
            return true;
        }
    }
    if let Some(t) = query_token {
        if t == expected {
            return true;
        }
    }
    false
}

fn is_open_path(path: &str) -> bool {
    matches!(
        path,
        "/" | "/health"
            | "/metrics"
            | "/pair"
            | "/pair/code"
            | "/webhook"
            | "/whatsapp"
            | "/linq"
            | "/wati"
            | "/nextcloud-talk"
            | "/webhook/gmail"
            | "/api/oauth/callback"
            | "/ws/desktop-bridge"
    ) || path.starts_with("/api/webauthn/")
}

pub async fn enforce(
    axum::extract::State(state): axum::extract::State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if state.exposed || state.pairing.require_pairing() {
        return next.run(request).await;
    }
    if request.method() == axum::http::Method::OPTIONS {
        return next.run(request).await;
    }
    let path = effective_path(request.uri());
    if is_open_path(path) {
        return next.run(request).await;
    }
    let qt = query_token(request.uri()).map(str::to_string);
    if request_matches(request.headers(), qt.as_deref()) {
        return next.run(request).await;
    }
    let bearer = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|auth| auth.strip_prefix("Bearer "))
        .unwrap_or("");
    if !bearer.is_empty() && state.pairing.is_authenticated_strict(bearer) {
        return next.run(request).await;
    }
    if let Some(t) = qt.as_deref() {
        if state.pairing.is_authenticated_strict(t) {
            return next.run(request).await;
        }
    }
    tracing::debug!(
        target: "gateway.security",
        path = %request.uri().path(),
        "rejected local gateway request without a valid loopback token"
    );
    (
        StatusCode::UNAUTHORIZED,
        axum::Json(serde_json::json!({
            "error": "Unauthorized - this local gateway requires its loopback token. Send \
                      Authorization: Bearer <token>, the X-Sen-Gateway-Token header, or \
                      ?token=<token>; the token is in <config dir>/gateway.token"
        })),
    )
        .into_response()
}
