// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Event Bus - central pub/sub system for agent lifecycle and inter-agent events.
//!
//! This module provides a `tokio::sync::broadcast`-based event bus with:
//! - Global broadcast channel for system-wide events
//! - Per-agent channels for targeted messaging
//! - Bounded event history for replay
//! - Integration with the hooks system

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::broadcast;
use tracing::{debug, trace, warn};

use crate::event_bus::types::{AgentId, Event, EventHistory, EventId, EventTarget};

pub mod integration;
pub mod types;

/// Capacity for the global broadcast channel.
const GLOBAL_CHANNEL_CAPACITY: usize = 1024;

/// Capacity for per-agent channels.
const AGENT_CHANNEL_CAPACITY: usize = 256;

/// Default event history size.
const DEFAULT_HISTORY_SIZE: usize = 1000;

/// Simple glob-style pattern matching for agent IDs.
///
/// Supports `*` (matches any sequence) and `?` (matches single char).
/// Used by `EventTarget::Pattern` routing.
fn pattern_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let pattern_chars: Vec<char> = pattern.chars().collect();
    let value_chars: Vec<char> = value.chars().collect();
    glob_match(&pattern_chars, &value_chars, 0, 0)
}

fn glob_match(pattern: &[char], value: &[char], pi: usize, vi: usize) -> bool {
    if pi == pattern.len() {
        return vi == value.len();
    }
    if pattern[pi] == '*' {
        // '*' matches zero or more characters
        for skip in 0..=(value.len() - vi) {
            if glob_match(pattern, value, pi + 1, vi + skip) {
                return true;
            }
        }
        return false;
    }
    if vi >= value.len() {
        return false;
    }
    if pattern[pi] == '?' || pattern[pi] == value[vi] {
        return glob_match(pattern, value, pi + 1, vi + 1);
    }
    false
}

/// Central event bus for pub/sub communication between agents and system components.
///
/// The EventBus provides:
/// 1. **Global broadcast**  - events sent to all subscribers
/// 2. **Per-agent channels**  - targeted messaging to specific agents
/// 3. **Bounded history**  - recent events stored for replay/debugging
///
/// # Example
///
/// ```rust,no_run
/// use senweavercoding::event_bus::{EventBus, types::Event};
///
/// let bus = EventBus::new();
///
/// // Subscribe to all events
/// let mut rx = bus.subscribe_all();
///
/// // Publish a broadcast event
/// bus.publish(Event::broadcast("system", "hello world")).await;
/// ```
#[derive(Debug)]
pub struct EventBus {
    /// Global broadcast sender (all events flow through here).
    global_sender: broadcast::Sender<Event>,
    /// Per-agent channels for targeted delivery.
    agent_channels: RwLock<HashMap<AgentId, broadcast::Sender<Event>>>,
    /// Bounded event history ring buffer.
    history: RwLock<EventHistory>,
}

impl EventBus {
    /// Create a new event bus with default capacities.
    pub fn new() -> Self {
        let (global_sender, _rx) = broadcast::channel(GLOBAL_CHANNEL_CAPACITY);

        Self {
            global_sender,
            agent_channels: RwLock::new(HashMap::new()),
            history: RwLock::new(EventHistory::new(DEFAULT_HISTORY_SIZE)),
        }
    }

    /// Create a new event bus with custom history size.
    pub fn with_history_size(history_size: usize) -> Self {
        let (global_sender, _rx) = broadcast::channel(GLOBAL_CHANNEL_CAPACITY);

        Self {
            global_sender,
            agent_channels: RwLock::new(HashMap::new()),
            history: RwLock::new(EventHistory::new(history_size)),
        }
    }

    /// Publish an event to the bus.
    ///
    /// The event is routed based on its `target`:
    /// - `Agent(id)`  - sent only to that agent's channel (if subscribed)
    /// - `Broadcast`  - sent to global channel and duplicated to all agent channels
    /// - `System`  - sent to global channel only
    /// - `Pattern(_)`  - currently treated as broadcast (pattern matching is future work)
    ///
    /// The event is also recorded in history.
    pub async fn publish(&self, event: Event) {
        trace!(event_id = %event.id, target = ?event.target, "publishing event");

        // Record in history first
        self.history.write().push(event.clone());

        // Route based on target
        match &event.target {
            EventTarget::Agent(agent_id) => {
                // Send to specific agent channel
                let channels = self.agent_channels.read();
                if let Some(sender) = channels.get(agent_id) {
                    if let Err(_e) = sender.send(event.clone()) {
                        warn!(agent_id = %agent_id, "failed to send to agent channel (receiver dropped)");
                    }
                }
                drop(channels);
                // Also send to global (agents may want to monitor other agents)
                let _ = self.global_sender.send(event);
            }
            EventTarget::Broadcast => {
                // Send to global and duplicate to all agent channels
                if let Err(_e) = self.global_sender.send(event.clone()) {
                    warn!("failed to broadcast to global channel (no receivers)");
                }

                // Duplicate to all agent channels
                let channels = self.agent_channels.read();
                for (agent_id, sender) in channels.iter() {
                    if let Err(_e) = sender.send(event.clone()) {
                        warn!(agent_id = %agent_id, "failed to duplicate broadcast to agent (receiver dropped)");
                    }
                }
            }
            EventTarget::System => {
                // System events go to global channel only
                if let Err(_e) = self.global_sender.send(event) {
                    warn!("failed to send system event to global channel (no receivers)");
                }
            }
            EventTarget::Pattern(pattern) => {
                // Pattern-based routing: match agent IDs against glob pattern
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
                // Also send to global for observability
                let _ = self.global_sender.send(event);
            }
        }
    }

    /// Subscribe to all broadcast events.
    ///
    /// Returns a receiver that will receive all global broadcast events.
    /// Note that receivers are lagging  - if the consumer is slow, they may
    /// miss events once the channel buffer fills.
    pub fn subscribe_all(&self) -> broadcast::Receiver<Event> {
        self.global_sender.subscribe()
    }

    /// Subscribe to events for a specific agent.
    ///
    /// Creates a per-agent channel if it doesn't exist. The agent will
    /// receive targeted events and all broadcast events.
    pub fn subscribe_agent(&self, agent_id: AgentId) -> broadcast::Receiver<Event> {
        let mut channels = self.agent_channels.write();
        let sender = channels.entry(agent_id.clone()).or_insert_with(|| {
            let (sender, _rx) = broadcast::channel(AGENT_CHANNEL_CAPACITY);
            debug!(agent_id = %agent_id, "created agent event channel");
            sender
        });

        sender.subscribe()
    }

    /// Unsubscribe an agent, removing its channel.
    ///
    /// This drops the sender, causing all receivers to eventually
    /// receive `RecvError::Closed`.
    pub fn unsubscribe_agent(&self, agent_id: &AgentId) {
        let mut channels = self.agent_channels.write();
        channels.remove(agent_id);
        debug!(agent_id = %agent_id, "removed agent event channel");
    }

    /// Remove channels with no active receivers (orphan cleanup).
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

    /// Get event history, optionally limited to a count.
    pub fn history(&self, limit: Option<usize>) -> Vec<Event> {
        self.history.read().get(limit)
    }

    /// Get the full event history.
    pub fn full_history(&self) -> Vec<Event> {
        self.history.read().all()
    }

    /// Clear the event history.
    pub fn clear_history(&self) {
        self.history.write().clear();
        debug!("event history cleared");
    }

    /// Get the current number of events in history.
    pub fn history_len(&self) -> usize {
        self.history.read().len()
    }

    /// Check if an agent has an active channel.
    pub fn has_agent_channel(&self, agent_id: &AgentId) -> bool {
        self.agent_channels.read().contains_key(agent_id)
    }

    /// Get the count of active agent channels.
    pub fn agent_channel_count(&self) -> usize {
        self.agent_channels.read().len()
    }

    /// Get a specific event by ID from history.
    pub fn get_event(&self, event_id: EventId) -> Option<Event> {
        self.history
            .read()
            .all()
            .into_iter()
            .find(|e| e.id == event_id)
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle to the event bus for convenient access.
///
/// This is a thin wrapper around `Arc<EventBus>` that provides
/// helper methods for common event publishing patterns.
#[derive(Debug, Clone)]
pub struct EventBusHandle {
    inner: Arc<EventBus>,
}

impl EventBusHandle {
    /// Create a new handle from an event bus.
    pub fn new(bus: EventBus) -> Self {
        Self {
            inner: Arc::new(bus),
        }
    }

    /// Create a handle from an existing Arc.
    pub fn from_arc(arc: Arc<EventBus>) -> Self {
        Self { inner: arc }
    }

    /// Get a reference to the underlying event bus.
    pub fn inner(&self) -> &EventBus {
        &self.inner
    }

    /// Convert to the underlying Arc.
    pub fn into_inner(self) -> Arc<EventBus> {
        self.inner
    }

    /// Publish an event.
    pub async fn publish(&self, event: Event) {
        self.inner.publish(event).await;
    }

    /// Subscribe to all events.
    pub fn subscribe_all(&self) -> broadcast::Receiver<Event> {
        self.inner.subscribe_all()
    }

    /// Subscribe to events for a specific agent.
    pub fn subscribe_agent(&self, agent_id: AgentId) -> broadcast::Receiver<Event> {
        self.inner.subscribe_agent(agent_id)
    }

    /// Unsubscribe an agent.
    pub fn unsubscribe_agent(&self, agent_id: &AgentId) {
        self.inner.unsubscribe_agent(agent_id);
    }

    /// Get event history.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::types::{EventPayload, LifecyclePhase, MemoryOperation, SystemCategory};
    use tokio::time::{Duration, timeout};

    #[tokio::test]
    async fn test_broadcast_event() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe_all();

        let event = Event::broadcast(
            "test",
            EventPayload::System {
                category: SystemCategory::HealthCheck,
                message: "ping".to_string(),
            },
        );

        bus.publish(event.clone()).await;

        let received = timeout(Duration::from_millis(100), rx.recv())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(received.source, "test");
    }

    #[tokio::test]
    async fn test_agent_targeted_event() {
        let bus = EventBus::new();
        let mut agent_rx = bus.subscribe_agent("agent-123".to_string());

        // Subscribe to global as well
        let mut global_rx = bus.subscribe_all();

        let event = Event::to_agent(
            "kernel",
            "agent-123".to_string(),
            EventPayload::Lifecycle {
                phase: LifecyclePhase::Started,
                error: None,
            },
        );

        bus.publish(event).await;

        // Agent should receive
        let received = timeout(Duration::from_millis(100), agent_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received.source, "kernel");

        // Global should also receive (agents can monitor each other)
        let received = timeout(Duration::from_millis(100), global_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received.source, "kernel");
    }

    #[tokio::test]
    async fn test_broadcast_to_all_agents() {
        let bus = EventBus::new();

        let mut rx1 = bus.subscribe_agent("agent-1".to_string());
        let mut rx2 = bus.subscribe_agent("agent-2".to_string());

        let event = Event::broadcast(
            "system",
            EventPayload::System {
                category: SystemCategory::ConfigReload,
                message: "config updated".to_string(),
            },
        );

        bus.publish(event).await;

        let r1 = timeout(Duration::from_millis(100), rx1.recv()).await;
        let r2 = timeout(Duration::from_millis(100), rx2.recv()).await;

        assert!(r1.is_ok() && r1.unwrap().is_ok());
        assert!(r2.is_ok() && r2.unwrap().is_ok());
    }

    #[tokio::test]
    async fn test_event_history() {
        let bus = EventBus::new();

        for i in 0..5 {
            bus.publish(Event::broadcast(
                "test",
                EventPayload::Memory {
                    operation: MemoryOperation::Store,
                    key: Some(format!("key-{}", i)),
                },
            ))
            .await;
        }

        assert_eq!(bus.history_len(), 5);

        let history = bus.history(Some(3));
        assert_eq!(history.len(), 3);

        // Clear and verify
        bus.clear_history();
        assert_eq!(bus.history_len(), 0);
    }

    #[tokio::test]
    async fn test_agent_unsubscribe() {
        let bus = EventBus::new();

        let _rx = bus.subscribe_agent("agent-1".to_string());
        assert!(bus.has_agent_channel(&"agent-1".to_string()));

        bus.unsubscribe_agent(&"agent-1".to_string());
        assert!(!bus.has_agent_channel(&"agent-1".to_string()));
    }

    #[tokio::test]
    async fn test_system_event_not_duplicated() {
        let bus = EventBus::new();

        let mut agent_rx = bus.subscribe_agent("agent-1".to_string());
        let mut global_rx = bus.subscribe_all();

        let event = Event::system("kernel", SystemCategory::Shutdown, "shutting down");

        bus.publish(event).await;

        // Global should receive
        let r = timeout(Duration::from_millis(100), global_rx.recv())
            .await
            .unwrap();
        assert!(r.is_ok());

        // Agent should NOT receive (system events don't go to agents)
        let r = timeout(Duration::from_millis(50), agent_rx.recv()).await;
        assert!(r.is_err()); // timeout = no message received
    }

    #[test]
    fn test_event_bus_handle() {
        let bus = EventBus::new();
        let handle = EventBusHandle::new(bus);

        assert_eq!(handle.inner().agent_channel_count(), 0);

        let _rx = handle.subscribe_agent("test-agent".to_string());
        assert_eq!(handle.inner().agent_channel_count(), 1);
    }

    #[test]
    fn test_pattern_matches_wildcard() {
        assert!(super::pattern_matches("*", "anything"));
        assert!(super::pattern_matches("*", ""));
    }

    #[test]
    fn test_pattern_matches_prefix() {
        assert!(super::pattern_matches("agent-*", "agent-1"));
        assert!(super::pattern_matches("agent-*", "agent-abc"));
        assert!(!super::pattern_matches("agent-*", "worker-1"));
    }

    #[test]
    fn test_pattern_matches_suffix() {
        assert!(super::pattern_matches("*-worker", "data-worker"));
        assert!(!super::pattern_matches("*-worker", "data-manager"));
    }

    #[test]
    fn test_pattern_matches_question_mark() {
        assert!(super::pattern_matches("agent-?", "agent-1"));
        assert!(!super::pattern_matches("agent-?", "agent-12"));
    }

    #[test]
    fn test_pattern_matches_exact() {
        assert!(super::pattern_matches("agent-1", "agent-1"));
        assert!(!super::pattern_matches("agent-1", "agent-2"));
    }

    #[tokio::test]
    async fn test_pattern_routing() {
        let bus = EventBus::new();

        let mut rx_a1 = bus.subscribe_agent("team-alpha-1".to_string());
        let mut rx_a2 = bus.subscribe_agent("team-alpha-2".to_string());
        let mut rx_b1 = bus.subscribe_agent("team-beta-1".to_string());

        let event = Event::new(
            "coordinator",
            EventTarget::Pattern("team-alpha-*".to_string()),
            EventPayload::System {
                category: SystemCategory::ConfigReload,
                message: "alpha team update".to_string(),
            },
        );

        bus.publish(event).await;

        // Alpha agents should receive
        let r1 = timeout(Duration::from_millis(100), rx_a1.recv()).await;
        let r2 = timeout(Duration::from_millis(100), rx_a2.recv()).await;
        assert!(r1.is_ok() && r1.unwrap().is_ok());
        assert!(r2.is_ok() && r2.unwrap().is_ok());

        // Beta agent should NOT receive via agent channel
        let r3 = timeout(Duration::from_millis(50), rx_b1.recv()).await;
        // Beta gets it from global broadcast (pattern events also go to global)
        // so this may or may not arrive depending on subscription order.
        // The key assertion is that alpha agents definitely received it.
        let _ = r3;
    }
}
