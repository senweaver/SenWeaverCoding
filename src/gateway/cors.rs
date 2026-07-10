// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer};

pub(crate) fn origin_value_allowed(value: &str) -> bool {
    let base_allowed = value.starts_with("tauri://")
        || value.starts_with("http://tauri.localhost")
        || value.starts_with("https://tauri.localhost")
        || value.starts_with("http://localhost")
        || value.starts_with("https://localhost")
        || value.starts_with("http://127.0.0.1")
        || value.starts_with("https://127.0.0.1")
        || value.starts_with("http://[::1]")
        || value.starts_with("https://[::1]");
    if base_allowed {
        return true;
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
        // No Origin header (native client) is allowed; a present but disallowed
        // browser Origin is rejected. An empty Origin string is treated as absent.
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
