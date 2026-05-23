// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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
