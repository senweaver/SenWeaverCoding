// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::IntoResponse;
use base64::Engine as _;
use std::path::Path;
use std::sync::OnceLock;

use super::AppState;

pub const TOKEN_ENV: &str = "SEN_GATEWAY_TOKEN";
pub const TOKEN_HEADER: &str = "x-sen-gateway-token";
pub const TOKEN_FILE_NAME: &str = "gateway.token";

pub fn decode_websocket_bearer_protocol(protocol: &str) -> Option<String> {
    if let Some(token) = protocol.strip_prefix("bearer64.") {
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(token)
            .ok()?;
        return String::from_utf8(bytes).ok().filter(|value| !value.is_empty());
    }
    protocol
        .strip_prefix("bearer.")
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

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
    if let Err(err) = crate::util::atomic_write(&path, token.as_bytes()) {
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
                .find_map(decode_websocket_bearer_protocol)
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
    if request.method() == axum::http::Method::OPTIONS {
        return next.run(request).await;
    }
    let path = effective_path(request.uri());
    if is_open_path(path) {
        return next.run(request).await;
    }
    let qt = query_token(request.uri()).map(str::to_string);
    if !state.exposed && request_matches(request.headers(), qt.as_deref()) {
        return next.run(request).await;
    }
    let mut credentials = Vec::new();
    if let Some(bearer) = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|auth| auth.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty())
    {
        credentials.push(bearer.to_string());
    }
    if let Some(t) = qt.as_deref() {
        credentials.push(t.to_string());
    }
    if let Some(protocols) = request
        .headers()
        .get("sec-websocket-protocol")
        .and_then(|value| value.to_str().ok())
    {
        for protocol in protocols.split(',').map(str::trim) {
            if let Some(token) = decode_websocket_bearer_protocol(protocol) {
                credentials.push(token);
            }
        }
    }
    if credentials
        .iter()
        .any(|credential| state.pairing.is_authenticated_strict(credential))
    {
        return next.run(request).await;
    }
    tracing::debug!(
        target: "gateway.security",
        path = %request.uri().path(),
        exposed = state.exposed,
        pairing_required = state.pairing.require_pairing(),
        "rejected gateway request without valid authentication"
    );
    (
        StatusCode::UNAUTHORIZED,
        axum::Json(serde_json::json!({
            "error": "Unauthorized - send a valid Bearer token, X-Sen-Gateway-Token header, \
                      or WebSocket bearer subprotocol"
        })),
    )
        .into_response()
}
