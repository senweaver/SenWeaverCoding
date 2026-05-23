// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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
