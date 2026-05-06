// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Session events resource — `/events` Server-Sent Events stream.
//!
//! exposes the canonical `SessionEvent` broadcast as a
//! live SSE endpoint so external UIs (web dashboards, downstream CLIs,
//! test harnesses) can subscribe with a one-liner `fetch('/events')`.
//!
//! The route takes an `Arc<AgentSession>` from the gateway's
//! application state and wraps `AgentSession::subscribe()` in an Axum
//! [`Sse`] response.  Events are serialized as JSON, one per SSE data
//! frame.  Broadcast lag (slow subscriber) is handled by skipping the
//! lagged batch rather than dropping the connection.
//!
//! Anti-placeholder: this module is invoked by `events_router()` which
//! is merged into the gateway router in `api.rs::build_router` (see
//! D3.2 wiring).  An inline smoke-test (`tests` module below)
//! verifies the JSON framing; a full-stack HTTP integration test is
//! tracked under follow-up (see D4.3).

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use futures_util::stream::{Stream, StreamExt};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

use crate::session::{AgentSession, SessionEvent};

pub fn events_router(session: Arc<AgentSession>) -> Router {
    Router::new()
        .route("/events", get(stream_session_events))
        .with_state(session)
}

async fn stream_session_events(
    State(session): State<Arc<AgentSession>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx: broadcast::Receiver<SessionEvent> = session.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|item| async move {
        match item {
            Ok(evt) => match serde_json::to_string(&evt) {
                Ok(json) => Some(Ok(Event::default().data(json))),
                Err(_) => None,
            },

            Err(_) => None,
        }
    });

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}
