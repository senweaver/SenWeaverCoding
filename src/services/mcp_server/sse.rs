// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! SSE transport for the embedded MCP server.
//!
//! Implements the **Server-Sent Events** transport defined by the
//! MCP 2024-11-05 spec:
//!
//! 1. Client opens `GET /sse`.  Server immediately replies with an
//!    `endpoint` event whose data is the URL the client should POST
//!    JSON-RPC requests to (`POST /messages?session=<id>`).
//! 2. Client POSTs JSON-RPC requests to that endpoint.  The server
//!    dispatches them through [`super::McpServer`] and pushes
//!    responses back over the originating SSE stream.
//! 3. Either side may close the stream; the server drops all
//!    per-session state on disconnect via the `SessionGuard`
//!    helper at the bottom of the file.
//!
//! The transport is intentionally minimalist — there is no auth, no
//! rate limiting, no TLS termination.  Operators that need any of
//! that should put a reverse proxy (nginx, Caddy, Traefik) in front
//! of the bind address.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::{
        IntoResponse,
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
}

pub async fn serve(server: McpServer, bind: SocketAddr) -> anyhow::Result<()> {
    let state = AppState {
        server: Arc::new(server),
        sessions: Arc::new(RwLock::new(HashMap::new())),
        messages_path: "/messages".to_string(),
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

async fn handle_sse(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
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

    Sse::new(mapped).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

async fn handle_messages(
    State(state): State<AppState>,
    Query(query): Query<MessagesQuery>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
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
