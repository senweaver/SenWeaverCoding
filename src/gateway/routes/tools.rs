// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Tools resource — exposes the runtime tool surface over HTTP.
//!
//! `GET /tools` returns a JSON array of registered tool names so
//! gateway clients can discover the tool surface without having to
//! re-implement the CLI's slash-command catalogue.  Real data is read
//! from [`crate::services::ServiceContainer::command_registry`] via
//! [`crate::cli::dispatch::list_commands`] — there is **no** in-memory
//! placeholder.

use axum::{Json, Router, routing::get};
use serde_json::{Value, json};

pub fn tools_router() -> Router {
    Router::new().route("/tools", get(list_tools))
}

async fn list_tools() -> Json<Value> {
    let commands = crate::cli::dispatch::list_commands();
    Json(json!({ "tools": commands, "count": commands.len() }))
}
