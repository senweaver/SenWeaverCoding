// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use parking_lot::RwLock;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_stream::Stream;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use super::McpServer;

type SessionRegistry = Arc<RwLock<HashMap<String, mpsc::Sender<OutFrame>>>>;

#[derive(Clone)]
struct OutFrame {
    event: &'static str,
    data: String,
}

#[derive(Clone)]
struct AppState {
    server: Arc<McpServer>,
    sessions: SessionRegistry,

    messages_path: String,

    auth_token: Option<Arc<String>>,
}

const MCP_SSE_TOKEN_VAR: &str = "SEN_MCP_SSE_TOKEN";

pub fn resolve_sse_auth_token(config_token: Option<&str>) -> Option<String> {
    if let Some(token) = crate::util::get_runtime_var(MCP_SSE_TOKEN_VAR) {
        let trimmed = token.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    config_token
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn authorize(state: &AppState, headers: &HeaderMap) -> Result<(), Response> {
    let Some(expected) = state.auth_token.as_deref() else {
        return Ok(());
    };
    let presented = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .unwrap_or("");
    if crate::security::pairing::constant_time_eq(presented, expected) {
        Ok(())
    } else {
        Err((StatusCode::UNAUTHORIZED, "unauthorized").into_response())
    }
}

pub async fn serve(
    server: McpServer,
    bind: SocketAddr,
    config_token: Option<String>,
) -> anyhow::Result<()> {
    let auth_token = resolve_sse_auth_token(config_token.as_deref()).map(Arc::new);
    if !bind.ip().is_loopback() && auth_token.is_none() {
        anyhow::bail!(
            "MCP SSE transport refused: bind address '{bind}' is not loopback and no auth token \
             is configured ({MCP_SSE_TOKEN_VAR} or mcp_server.sse_token). Bind to 127.0.0.1, or \
             set a token to expose it with Bearer auth."
        );
    }
    if auth_token.is_none() {
        tracing::warn!(
            target: "mcp.server.sse",
            %bind,
            "SECURITY: MCP SSE transport is running WITHOUT authentication (neither \
             {MCP_SSE_TOKEN_VAR} nor mcp_server.sse_token set) on loopback; any local process \
             can invoke the exposed tools. Set a token to require Bearer auth."
        );
    }
    let state = AppState {
        server: Arc::new(server),
        sessions: Arc::new(RwLock::new(HashMap::new())),
        messages_path: "/messages".to_string(),
        auth_token,
    };

    let app = Router::new()
        .route("/sse", get(handle_sse))
        .route("/messages", post(handle_messages))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind).await.map_err(|e| {
        anyhow::anyhow!("MCP SSE bind failed on {bind}: {e}")
    })?;
    tracing::info!(
        target: "mcp.server.sse",
        %bind,
        "MCP SSE transport listening"
    );

    axum::serve(listener, app)
        .await
        .map_err(|e| anyhow::anyhow!("MCP SSE serve loop exited: {e}"))?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct MessagesQuery {
    session: String,
}

async fn handle_sse(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(resp) = authorize(&state, &headers) {
        return resp;
    }
    let session_id = Uuid::new_v4().to_string();
    let (tx, rx) = mpsc::channel::<OutFrame>(64);
    state
        .sessions
        .write()
        .insert(session_id.clone(), tx.clone());

    let endpoint_url = format!("{}?session={}", state.messages_path, session_id);
    if let Err(e) = tx
        .send(OutFrame {
            event: "endpoint",
            data: endpoint_url.clone(),
        })
        .await
    {
        tracing::warn!(
            target: "mcp.server.sse",
            session = %session_id,
            error = %e,
            "failed to seed endpoint event; client will see an empty stream"
        );
    }

    let guard = SessionGuard {
        rx: ReceiverStream::new(rx),
        sessions: state.sessions.clone(),
        session_id: session_id.clone(),
    };

    let mapped = guard.map(|frame| {
        Ok::<Event, Infallible>(Event::default().event(frame.event).data(frame.data))
    });

    Sse::new(mapped)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keep-alive"),
        )
        .into_response()
}

async fn handle_messages(
    State(state): State<AppState>,
    Query(query): Query<MessagesQuery>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if let Err(resp) = authorize(&state, &headers) {
        return resp;
    }
    let sender = {
        let sessions = state.sessions.read();
        sessions.get(&query.session).cloned()
    };
    let Some(sender) = sender else {
        return (
            StatusCode::NOT_FOUND,
            "session not found or already closed",
        )
            .into_response();
    };

    let server = state.server.clone();
    let response = server.dispatch(body).await;
    if let Some(resp) = response {
        let payload = match serde_json::to_string(&resp) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(
                    target: "mcp.server.sse",
                    error = %e,
                    "failed to serialise dispatch response"
                );
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "response serialisation failed",
                )
                    .into_response();
            }
        };
        if sender
            .send(OutFrame {
                event: "message",
                data: payload,
            })
            .await
            .is_err()
        {
            return (
                StatusCode::GONE,
                "client SSE stream closed before response could be delivered",
            )
                .into_response();
        }
    }
    StatusCode::ACCEPTED.into_response()
}

struct SessionGuard {
    rx: ReceiverStream<OutFrame>,
    sessions: SessionRegistry,
    session_id: String,
}

impl Stream for SessionGuard {
    type Item = OutFrame;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::pin::Pin::new(&mut self.rx).poll_next(cx)
    }
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        self.sessions.write().remove(&self.session_id);
        tracing::debug!(
            target: "mcp.server.sse",
            session = %self.session_id,
            "session disconnected; entry removed from registry"
        );
    }
}
