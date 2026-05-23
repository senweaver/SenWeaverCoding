// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use axum::{Json, Router, routing::get};
use serde_json::{Value, json};

pub fn tools_router() -> Router {
    Router::new().route("/tools", get(list_tools))
}

async fn list_tools() -> Json<Value> {
    let commands = crate::cli::dispatch::list_commands();
    Json(json!({ "tools": commands, "count": commands.len() }))
}
