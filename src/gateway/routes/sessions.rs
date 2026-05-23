// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use axum::{Json, Router, routing::get};
use serde_json::{Value, json};

pub fn sessions_router() -> Router {
    Router::new().route("/sessions", get(list_sessions))
}

async fn list_sessions() -> Json<Value> {

    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let infos: Vec<_> = crate::cli::bg::list_sessions_sync(&cwd)
        .unwrap_or_default()
        .into_iter()
        .map(|s| {
            json!({
                "id": s.id,
                "pid": s.pid,
                "started_at": s.started_at,
            })
        })
        .collect();
    let count = infos.len();
    Json(json!({ "sessions": infos, "count": count }))
}
