// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::gateway::AppState;
use axum::{
    extract::State,
    http::StatusCode,
    middleware,
    response::Json,
    routing::post,
    Router,
};
use futures_util::FutureExt;
use serde::Deserialize;
use serde_json::json;

pub fn agent_router(state: AppState) -> Router {
    Router::new()
        .route("/v1/agent/turn", post(agent_turn_handler))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            crate::gateway::api::auth_middleware,
        ))
        .with_state(state)
}

#[derive(Deserialize)]
struct AgentTurnRequest {
    message: String,
    session_id: Option<String>,
}

async fn agent_turn_handler(
    State(state): State<AppState>,
    Json(payload): Json<AgentTurnRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    tracing::debug!(message = %payload.message, "agent_turn_handler received request");
    let config = state.config.lock().clone();
    let turn_fut = Box::pin(crate::agent::process_message(
        config,
        &payload.message,
        payload.session_id.as_deref(),
    ));
    let caught = std::panic::AssertUnwindSafe(turn_fut)
        .catch_unwind()
        .await;
    let result = match caught {
        Ok(inner) => inner,
        Err(_) => {
            tracing::error!("agent_turn_handler: turn panicked and was isolated");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    match result {
        Ok(response) => Ok(Json(json!({
            "response": response,
            "session_id": payload.session_id,
        }))),
        Err(e) => {
            tracing::error!("agent_turn_handler error: {e:#}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
