// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use axum::{Router, routing::get};
use serde_json::json;

pub fn health_router() -> Router {
    Router::new().route("/health", get(health_handler))
}

async fn health_handler() -> axum::Json<serde_json::Value> {
    axum::Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}
