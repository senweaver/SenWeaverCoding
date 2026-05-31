// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::broadcast;
use tracing::{debug, trace, warn};

use crate::event_bus::types::{AgentId, Event, EventHistory, EventId, EventTarget};

pub mod backpressure;
pub mod integration;
pub mod types;
pub use backpressure::BoundedSubscriber;

const GLOBAL_CHANNEL_CAPACITY: usize = 1024;

const AGENT_CHANNEL_CAPACITY: usize = 256;

const DEFAULT_HISTORY_SIZE: usize = 1000;

fn pattern_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let pattern_chars: Vec<char> = pattern.chars().collect();
    let value_chars: Vec<char> = value.chars().collect();
    glob_match_dp(&pattern_chars, &value_chars)
}

fn glob_match_dp(pattern: &[char], value: &[char]) -> bool {
    let plen = pattern.len();
    let vlen = value.len();
    let mut prev = vec![false; vlen + 1];
    let mut curr = vec![false; vlen + 1];
    prev[0] = true;
    for pi in 1..=plen {
        if pattern[pi - 1] == '*' {
            curr[0] = prev[0];
            for vi in 1..=vlen {
                curr[vi] = prev[vi] || curr[vi - 1];
            }
        } else {
            curr[0] = false;
            for vi in 1..=vlen {
                curr[vi] = prev[vi - 1]
                    && (pattern[pi - 1] == '?' || pattern[pi - 1] == value[vi - 1]);
            }
        }
        std::mem::swap(&mut prev, &mut curr);
        for v in curr.iter_mut() {
            *v = false;
        }
    }
    prev[vlen]
}

#[derive(Debug)]
pub struct EventBus {

    global_sender: broadcast::Sender<Event>,

    agent_channels: RwLock<HashMap<AgentId, broadcast::Sender<Event>>>,

    history: RwLock<EventHistory>,
}

impl EventBus {

    pub fn new() -> Self {
        let (global_sender, _rx) = broadcast::channel(GLOBAL_CHANNEL_CAPACITY);

        Self {
            global_sender,
            agent_channels: RwLock::new(HashMap::new()),
            history: RwLock::new(EventHistory::new(DEFAULT_HISTORY_SIZE)),
        }
    }

    pub fn with_history_size(history_size: usize) -> Self {
        let (global_sender, _rx) = broadcast::channel(GLOBAL_CHANNEL_CAPACITY);

        Self {
            global_sender,
            agent_channels: RwLock::new(HashMap::new()),
            history: RwLock::new(EventHistory::new(history_size)),
        }
    }

    pub async fn publish(&self, event: Event) {
        trace!(event_id = %event.id, target = ?event.target, "publishing event");

        self.history.write().push(event.clone());

        match &event.target {
            EventTarget::Agent(agent_id) => {

                let channels = self.agent_channels.read();
                if let Some(sender) = channels.get(agent_id) {
                    if let Err(_e) = sender.send(event.clone()) {
                        warn!(agent_id = %agent_id, "failed to send to agent channel (receiver dropped)");
                    }
                }
                drop(channels);

                let _ = self.global_sender.send(event);
            }
            EventTarget::Broadcast => {

                if let Err(_e) = self.global_sender.send(event.clone()) {
                    warn!("failed to broadcast to global channel (no receivers)");
                }

                let channels = self.agent_channels.read();
                for (agent_id, sender) in channels.iter() {
                    if let Err(_e) = sender.send(event.clone()) {
                        warn!(agent_id = %agent_id, "failed to duplicate broadcast to agent (receiver dropped)");
                    }
                }
            }
            EventTarget::System => {

                if let Err(_e) = self.global_sender.send(event) {
                    warn!("failed to send system event to global channel (no receivers)");
                }
            }
            EventTarget::Pattern(pattern) => {

                let channels = self.agent_channels.read();
                let mut matched = 0usize;
                for (agent_id, sender) in channels.iter() {
                    if pattern_matches(pattern, agent_id) {
                        if let Err(_e) = sender.send(event.clone()) {
                            warn!(agent_id = %agent_id, "failed to send pattern-matched event (receiver dropped)");
                        }
                        matched += 1;
                    }
                }
                drop(channels);
                debug!(pattern = %pattern, matched, "pattern-based routing complete");

                let _ = self.global_sender.send(event);
            }
        }
    }

    pub fn subscribe_all(&self) -> broadcast::Receiver<Event> {
        self.global_sender.subscribe()
    }

    pub fn subscribe_agent(&self, agent_id: AgentId) -> broadcast::Receiver<Event> {
        let mut channels = self.agent_channels.write();
        let sender = channels.entry(agent_id.clone()).or_insert_with(|| {
            let (sender, _rx) = broadcast::channel(AGENT_CHANNEL_CAPACITY);
            debug!(agent_id = %agent_id, "created agent event channel");
            sender
        });

        sender.subscribe()
    }

    pub fn unsubscribe_agent(&self, agent_id: &AgentId) {
        let mut channels = self.agent_channels.write();
        channels.remove(agent_id);
        debug!(agent_id = %agent_id, "removed agent event channel");
    }

    pub fn prune_orphaned_channels(&self) {
        let mut channels = self.agent_channels.write();
        let before = channels.len();
        channels.retain(|id, sender| {
            if sender.receiver_count() == 0 {
                debug!(agent_id = %id, "pruning orphaned agent channel");
                false
            } else {
                true
            }
        });
        let pruned = before - channels.len();
        if pruned > 0 {
            debug!(pruned, "pruned orphaned agent event channels");
        }
    }

    pub fn history(&self, limit: Option<usize>) -> Vec<Event> {
        self.history.read().get(limit)
    }

    pub fn full_history(&self) -> Vec<Event> {
        self.history.read().all()
    }

    pub fn clear_history(&self) {
        self.history.write().clear();
        debug!("event history cleared");
    }

    pub fn history_len(&self) -> usize {
        self.history.read().len()
    }

    pub fn has_agent_channel(&self, agent_id: &AgentId) -> bool {
        self.agent_channels.read().contains_key(agent_id)
    }

    pub fn agent_channel_count(&self) -> usize {
        self.agent_channels.read().len()
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
        self.inner.publish(event).await;
    }

    pub fn subscribe_all(&self) -> broadcast::Receiver<Event> {
        self.inner.subscribe_all()
    }

    pub fn subscribe_agent(&self, agent_id: AgentId) -> broadcast::Receiver<Event> {
        self.inner.subscribe_agent(agent_id)
    }

    pub fn unsubscribe_agent(&self, agent_id: &AgentId) {
        self.inner.unsubscribe_agent(agent_id);
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
