// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer};

pub(crate) fn desktop_cors_layer() -> CorsLayer {
    use axum::http::Method;

    let allowed = AllowOrigin::predicate(|origin, _| {
        let Ok(value) = origin.to_str() else {
            return false;
        };
        let base_allowed = value.starts_with("tauri://")
            || value.starts_with("http://tauri.localhost")
            || value.starts_with("https://tauri.localhost")
            || value.starts_with("http://localhost")
            || value.starts_with("http://127.0.0.1")
            || value.starts_with("http://[::1]");
        if base_allowed {
            return true;
        }

        if cfg!(debug_assertions) && value == "null" {
            return true;
        }
        false
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

    let headers = if cfg!(debug_assertions) {
        AllowHeaders::any()
    } else {
        use axum::http::header::{
            ACCEPT, ACCEPT_LANGUAGE, AUTHORIZATION, CACHE_CONTROL, CONTENT_LANGUAGE, CONTENT_TYPE,
            IF_MATCH, IF_NONE_MATCH,
        };
        AllowHeaders::list([
            ACCEPT,
            ACCEPT_LANGUAGE,
            AUTHORIZATION,
            CACHE_CONTROL,
            CONTENT_LANGUAGE,
            CONTENT_TYPE,
            IF_MATCH,
            IF_NONE_MATCH,
            axum::http::HeaderName::from_static("x-session-id"),
            axum::http::HeaderName::from_static("x-requested-with"),
            axum::http::HeaderName::from_static("idempotency-key"),
            axum::http::HeaderName::from_static("x-sen-pairing-token"),
            axum::http::HeaderName::from_static("x-sen-client"),
        ])
    };

    CorsLayer::new()
        .allow_origin(allowed)
        .allow_methods(methods)
        .allow_headers(headers)
        .allow_credentials(false)
}
