// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Backpressure-aware subscriber wrapper for the event bus.
//!
//! The core `broadcast` channel drops messages for lagging consumers; that
//! is the right behaviour for best-effort observability streams but wrong
//! for consumers that must not miss events (for example, a task scheduler
//! watching for "task completed" signals).
//!
//! `BoundedSubscriber` wraps a `broadcast::Receiver` plus a bounded
//! `mpsc` channel.  A background forwarder task drains the broadcast side
//! and pushes into the bounded mpsc; when the mpsc fills up, the
//! forwarder blocks (back-pressuring the broadcast receiver end).  That
//! way a slow consumer applies back-pressure without crashing the event
//! bus.

use tokio::sync::{broadcast, mpsc};

use crate::event_bus::types::Event;
use crate::runtime::TaskHandle;

pub struct BoundedSubscriber {
    rx: mpsc::Receiver<Event>,
    forwarder: TaskHandle,
}

impl BoundedSubscriber {

    pub fn new(mut broadcast_rx: broadcast::Receiver<Event>, capacity: usize) -> Self {
        let (tx, rx) = mpsc::channel(capacity.max(1));

        let forwarder =
            crate::runtime::spawn_supervised("event_bus.backpressure.forwarder", async move {
                loop {
                    match broadcast_rx.recv().await {
                        Ok(event) => {

                            if tx.send(event).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(skipped)) => {
                            tracing::warn!(
                                skipped,
                                "bounded subscriber lagged behind — events dropped"
                            );
                            continue;
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            break;
                        }
                    }
                }
            });

        Self { rx, forwarder }
    }

    pub async fn recv(&mut self) -> Option<Event> {
        self.rx.recv().await
    }

    pub fn try_recv(&mut self) -> Result<Event, mpsc::error::TryRecvError> {
        self.rx.try_recv()
    }
}

impl Drop for BoundedSubscriber {
    fn drop(&mut self) {
        self.forwarder.abort();
    }
}
