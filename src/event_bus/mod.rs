// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::broadcast;
use tracing::{trace, warn};

use crate::event_bus::types::{Event, EventHistory, EventId, EventTarget};

pub mod backpressure;
pub mod integration;
pub mod types;
pub use backpressure::BoundedSubscriber;

const GLOBAL_CHANNEL_CAPACITY: usize = 8192;

const DEFAULT_HISTORY_SIZE: usize = 1000;

#[derive(Debug)]
pub struct EventBus {

    global_sender: broadcast::Sender<Event>,

    history: RwLock<EventHistory>,
}

impl EventBus {

    pub fn new() -> Self {
        let (global_sender, _rx) = broadcast::channel(GLOBAL_CHANNEL_CAPACITY);

        Self {
            global_sender,
            history: RwLock::new(EventHistory::new(DEFAULT_HISTORY_SIZE)),
        }
    }

    pub fn with_history_size(history_size: usize) -> Self {
        let (global_sender, _rx) = broadcast::channel(GLOBAL_CHANNEL_CAPACITY);

        Self {
            global_sender,
            history: RwLock::new(EventHistory::new(history_size)),
        }
    }

    pub async fn publish(&self, event: Event) {
        self.publish_now(event);
    }

    pub fn publish_now(&self, event: Event) {
        trace!(event_id = %event.id, target = ?event.target, "publishing event");

        self.history.write().push(event.clone());

        match &event.target {
            EventTarget::Broadcast | EventTarget::System => {
                if let Err(_e) = self.global_sender.send(event) {
                    warn!("failed to send event to global channel (no receivers)");
                }
            }
            EventTarget::Agent(_) | EventTarget::Pattern(_) => {
                let _ = self.global_sender.send(event);
            }
        }
    }

    pub fn subscribe_all(&self) -> broadcast::Receiver<Event> {
        self.global_sender.subscribe()
    }

    pub fn history(&self, limit: Option<usize>) -> Vec<Event> {
        self.history.read().get(limit)
    }

    pub fn full_history(&self) -> Vec<Event> {
        self.history.read().all()
    }

    pub fn clear_history(&self) {
        self.history.write().clear();
        trace!("event history cleared");
    }

    pub fn history_len(&self) -> usize {
        self.history.read().len()
    }

    pub fn get_event(&self, event_id: EventId) -> Option<Event> {
        let history = self.history.read();
        history.find_by_id(event_id)
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct EventBusHandle {
    inner: Arc<EventBus>,
}

impl EventBusHandle {

    pub fn new(bus: EventBus) -> Self {
        Self {
            inner: Arc::new(bus),
        }
    }

    pub fn from_arc(arc: Arc<EventBus>) -> Self {
        Self { inner: arc }
    }

    pub fn inner(&self) -> &EventBus {
        &self.inner
    }

    pub fn into_inner(self) -> Arc<EventBus> {
        self.inner
    }

    pub async fn publish(&self, event: Event) {
        self.inner.publish_now(event);
    }

    pub fn publish_now(&self, event: Event) {
        self.inner.publish_now(event);
    }

    pub fn subscribe_all(&self) -> broadcast::Receiver<Event> {
        self.inner.subscribe_all()
    }

    pub fn history(&self, limit: Option<usize>) -> Vec<Event> {
        self.inner.history(limit)
    }
}

impl From<EventBus> for EventBusHandle {
    fn from(bus: EventBus) -> Self {
        Self::new(bus)
    }
}

impl From<Arc<EventBus>> for EventBusHandle {
    fn from(arc: Arc<EventBus>) -> Self {
        Self::from_arc(arc)
    }
}
