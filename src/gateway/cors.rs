// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer};

const LOOPBACK_ORIGIN_HOSTS: [&str; 4] = ["localhost", "tauri.localhost", "127.0.0.1", "::1"];

fn authority_host_is_loopback(authority: &str) -> bool {
    let authority = authority.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let host = if let Some(rest) = authority.strip_prefix('[') {
        match rest.split_once(']') {
            Some((inner, after)) if after.is_empty() || after.starts_with(':') => inner,
            _ => return false,
        }
    } else {
        authority.split(':').next().unwrap_or("")
    };
    LOOPBACK_ORIGIN_HOSTS
        .iter()
        .any(|allowed| host.eq_ignore_ascii_case(allowed))
}

pub(crate) fn origin_value_allowed(value: &str) -> bool {
    let value = value.trim();
    if let Some(rest) = value.strip_prefix("tauri://") {
        return authority_host_is_loopback(rest);
    }
    if let Some(rest) = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"))
    {
        return authority_host_is_loopback(rest);
    }

    if cfg!(debug_assertions) && value == "null" {
        return true;
    }
    false
}

pub(crate) fn ws_origin_allowed(headers: &axum::http::HeaderMap) -> bool {
    match headers
        .get(axum::http::header::ORIGIN)
        .map(|v| v.to_str().map(str::trim))
    {
        None => true,
        Some(Ok(value)) => value.is_empty() || origin_value_allowed(value),
        Some(Err(_)) => false,
    }
}

pub(crate) fn reject_ws_disallowed_origin(
    headers: &axum::http::HeaderMap,
    endpoint: &str,
) -> Option<axum::response::Response> {
    if ws_origin_allowed(headers) {
        return None;
    }
    let origin = headers
        .get(axum::http::header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("<invalid>");
    tracing::warn!(
        target: "gateway.security",
        endpoint,
        origin,
        "rejecting WebSocket upgrade from disallowed browser origin"
    );
    Some(axum::response::IntoResponse::into_response((
        axum::http::StatusCode::FORBIDDEN,
        "Forbidden - WebSocket connections from this browser origin are not allowed",
    )))
}

pub(crate) fn desktop_cors_layer() -> CorsLayer {
    use axum::http::Method;

    let allowed = AllowOrigin::predicate(|origin, _| {
        let Ok(value) = origin.to_str() else {
            return false;
        };
        origin_value_allowed(value)
    });

    let methods = AllowMethods::list([
        Method::GET,
        Method::POST,
        Method::PUT,
        Method::DELETE,
        Method::PATCH,
        Method::HEAD,
        Method::OPTIONS,
    ]);

    CorsLayer::new()
        .allow_origin(allowed)
        .allow_methods(methods)
        .allow_headers(AllowHeaders::any())
        .allow_credentials(false)
}
