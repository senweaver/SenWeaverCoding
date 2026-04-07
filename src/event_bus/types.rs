// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Event Bus types - pub/sub system for agent lifecycle and inter-agent communication.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Unique identifier for events.
pub type EventId = String;

/// Unique identifier for agents (re-exported pattern).
pub type AgentId = String;

/// Target specification for event routing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventTarget {
    /// Send to a specific agent only.
    Agent(AgentId),
    /// Broadcast to all subscribers.
    Broadcast,
    /// System-level event (not for general agents).
    System,
    /// Pattern-based targeting (future expansion).
    #[serde(skip)]
    Pattern(String),
}

impl Default for EventTarget {
    fn default() -> Self {
        Self::Broadcast
    }
}

/// Payload variants for different event types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventPayload {
    /// Agent lifecycle event.
    Lifecycle {
        /// What happened: spawned, started, stopped, terminated, error.
        phase: LifecyclePhase,
        /// Optional error message for error phase.
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// System-level event (config reload, shutdown, etc.).
    System {
        /// System event category.
        category: SystemCategory,
        /// Human-readable description.
        message: String,
    },
    /// Memory operation event.
    Memory {
        /// Operation type.
        operation: MemoryOperation,
        /// Key affected (if applicable).
        #[serde(skip_serializing_if = "Option::is_none")]
        key: Option<String>,
    },
    /// Tool execution event.
    Tool {
        /// Tool name.
        name: String,
        /// Execution result summary.
        result: ToolResultSummary,
        /// Duration in milliseconds.
        duration_ms: u64,
    },
    /// Inbound message received from a channel.
    MessageReceived {
        /// Channel the message arrived on.
        channel: String,
        /// Truncated preview of the message content.
        preview: String,
    },
    /// Outbound message sent to a channel.
    MessageSent {
        /// Channel the message was sent to.
        channel: String,
        /// Truncated preview of the message content.
        preview: String,
    },
    /// Inter-agent request: one agent asks another to perform work.
    AgentRequest {
        /// Unique request ID for correlation.
        request_id: String,
        /// Required capability (for capability-based routing).
        capability: String,
        /// The task prompt / instruction.
        prompt: String,
        /// Optional context payload.
        #[serde(skip_serializing_if = "Option::is_none")]
        context: Option<serde_json::Value>,
        /// Timeout in seconds for the request.
        timeout_secs: u64,
    },
    /// Inter-agent response: result of an agent request.
    AgentResponse {
        /// Correlation ID matching the original request.
        request_id: String,
        /// Whether the request was fulfilled successfully.
        success: bool,
        /// Response content.
        output: String,
        /// Error message if not successful.
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Task delegation event (agent-to-agent work assignment).
    TaskDelegation {
        /// Task identifier.
        task_id: String,
        /// Delegation action.
        action: TaskDelegationAction,
        /// Task description.
        description: String,
    },
    /// Coordination event for consensus and synchronization.
    Coordination {
        /// Coordination protocol action.
        action: CoordinationAction,
        /// Topic / resource being coordinated.
        topic: String,
        /// Payload data.
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<serde_json::Value>,
    },
    /// Custom JSON payload for application-defined events.
    Custom {
        /// Event subtype identifier.
        subtype: String,
        /// JSON payload.
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<serde_json::Value>,
    },
}

/// Lifecycle phases for agents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LifecyclePhase {
    Spawned,
    Started,
    Stopped,
    Terminated,
    Error,
}

/// System event categories.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SystemCategory {
    Startup,
    ConfigReload,
    Shutdown,
    HealthCheck,
    GatewayStart,
    GatewayStop,
}

/// Memory operation types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryOperation {
    Store,
    Recall,
    Forget,
    Consolidate,
    GraphAdd,
    GraphQuery,
}

/// Tool execution result summary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolResultSummary {
    Success,
    Error,
    Cancelled,
    Timeout,
}

/// Task delegation actions for inter-agent work assignment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskDelegationAction {
    /// A new task has been assigned.
    Assigned,
    /// Task accepted by the target agent.
    Accepted,
    /// Task rejected (agent busy or incapable).
    Rejected,
    /// Task progress update.
    Progress,
    /// Task completed.
    Completed,
    /// Task failed.
    Failed,
}

/// Coordination protocol actions for multi-agent consensus.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CoordinationAction {
    /// Request to acquire a resource lock.
    LockRequest,
    /// Lock granted.
    LockGranted,
    /// Lock denied (held by another agent).
    LockDenied,
    /// Release a held lock.
    LockRelease,
    /// Propose a value for consensus.
    Propose,
    /// Vote on a proposal.
    Vote,
    /// Proposal committed (consensus reached).
    Commit,
    /// Barrier synchronization: agent ready.
    BarrierReady,
    /// Barrier synchronization: all agents ready, proceed.
    BarrierRelease,
}

/// Core event structure.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    /// Unique event identifier.
    pub id: EventId,
    /// Source of the event (agent_id, "system", "kernel", etc.).
    pub source: String,
    /// Target routing specification.
    #[serde(default)]
    pub target: EventTarget,
    /// Event payload.
    pub payload: EventPayload,
    /// UTC timestamp in RFC3339 format.
    pub timestamp: String,
    /// Correlation ID for request/response matching.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    /// Reply-to agent ID for response routing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<AgentId>,
}

impl Event {
    /// Create a new event with the current timestamp.
    pub fn new(source: impl Into<String>, target: EventTarget, payload: EventPayload) -> Self {
        Self {
            id: format!(
                "evt-{}-{}",
                Utc::now().timestamp_millis(),
                uuid::Uuid::new_v4().to_string()[..8].to_string()
            ),
            source: source.into(),
            target,
            payload,
            timestamp: Utc::now().to_rfc3339(),
            correlation_id: None,
            reply_to: None,
        }
    }

    /// Set correlation ID for request/response tracking.
    pub fn with_correlation(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }

    /// Set reply-to agent for response routing.
    pub fn with_reply_to(mut self, agent_id: impl Into<String>) -> Self {
        self.reply_to = Some(agent_id.into());
        self
    }

    /// Create an inter-agent request event.
    pub fn agent_request(
        source: impl Into<String>,
        target_agent: AgentId,
        request_id: impl Into<String>,
        capability: impl Into<String>,
        prompt: impl Into<String>,
        timeout_secs: u64,
    ) -> Self {
        let source_str: String = source.into();
        Self::new(
            source_str.clone(),
            EventTarget::Agent(target_agent),
            EventPayload::AgentRequest {
                request_id: request_id.into(),
                capability: capability.into(),
                prompt: prompt.into(),
                context: None,
                timeout_secs,
            },
        )
        .with_reply_to(source_str)
    }

    /// Create an inter-agent response event.
    pub fn agent_response(
        source: impl Into<String>,
        target_agent: AgentId,
        request_id: impl Into<String>,
        success: bool,
        output: impl Into<String>,
        error: Option<String>,
    ) -> Self {
        let req_id: String = request_id.into();
        Self::new(
            source,
            EventTarget::Agent(target_agent),
            EventPayload::AgentResponse {
                request_id: req_id.clone(),
                success,
                output: output.into(),
                error,
            },
        )
        .with_correlation(req_id)
    }

    /// Create a broadcast event.
    pub fn broadcast(source: impl Into<String>, payload: EventPayload) -> Self {
        Self::new(source, EventTarget::Broadcast, payload)
    }

    /// Create an agent-targeted event.
    pub fn to_agent(source: impl Into<String>, agent_id: AgentId, payload: EventPayload) -> Self {
        Self::new(source, EventTarget::Agent(agent_id), payload)
    }

    /// Create a system event.
    pub fn system(
        source: impl Into<String>,
        category: SystemCategory,
        message: impl Into<String>,
    ) -> Self {
        Self::new(
            source,
            EventTarget::System,
            EventPayload::System {
                category,
                message: message.into(),
            },
        )
    }

    /// Get a human-readable description of the event.
    pub fn describe(&self) -> String {
        format!(
            "[{}] {} -> {:?}: {}",
            self.timestamp,
            self.source,
            self.target,
            match &self.payload {
                EventPayload::Lifecycle { phase, error } => {
                    let err_str = error
                        .as_ref()
                        .map(|e| format!(" (error: {})", e))
                        .unwrap_or_default();
                    format!("lifecycle: {:?}{}", phase, err_str)
                }
                EventPayload::System { category, message } => {
                    format!("system [{:?}]: {}", category, message)
                }
                EventPayload::Memory { operation, key } => {
                    let key_str = key
                        .as_ref()
                        .map(|k| format!(" key={}", k))
                        .unwrap_or_default();
                    format!("memory: {:?}{}", operation, key_str)
                }
                EventPayload::Tool {
                    name,
                    result,
                    duration_ms,
                } => {
                    format!("tool {}: {:?} ({}ms)", name, result, duration_ms)
                }
                EventPayload::MessageReceived { channel, preview } => {
                    format!("message received [{}]: {}", channel, preview)
                }
                EventPayload::MessageSent { channel, preview } => {
                    format!("message sent [{}]: {}", channel, preview)
                }
                EventPayload::AgentRequest {
                    request_id,
                    capability,
                    ..
                } => {
                    format!("agent_request [{}]: capability={}", request_id, capability)
                }
                EventPayload::AgentResponse {
                    request_id,
                    success,
                    ..
                } => {
                    format!("agent_response [{}]: success={}", request_id, success)
                }
                EventPayload::TaskDelegation {
                    task_id, action, ..
                } => {
                    format!("task_delegation [{}]: {:?}", task_id, action)
                }
                EventPayload::Coordination { action, topic, .. } => {
                    format!("coordination [{:?}]: {}", action, topic)
                }
                EventPayload::Custom { subtype, .. } => {
                    format!("custom: {}", subtype)
                }
            }
        )
    }
}

/// Bounded event history ring buffer.
#[derive(Debug, Clone)]
pub struct EventHistory {
    events: VecDeque<Event>,
    capacity: usize,
}

impl EventHistory {
    /// Create a new history buffer with the specified capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            events: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Push an event to the history, evicting oldest if at capacity.
    pub fn push(&mut self, event: Event) {
        if self.events.len() >= self.capacity {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    /// Get events from the history, optionally limited to count.
    pub fn get(&self, limit: Option<usize>) -> Vec<Event> {
        let count = limit.unwrap_or(self.events.len()).min(self.events.len());
        self.events.iter().rev().take(count).cloned().collect()
    }

    /// Get all events.
    pub fn all(&self) -> Vec<Event> {
        self.events.iter().cloned().collect()
    }

    /// Current number of events in history.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Check if history is empty.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Clear all events.
    pub fn clear(&mut self) {
        self.events.clear();
    }
}

impl Default for EventHistory {
    fn default() -> Self {
        Self::new(1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_creation() {
        let event = Event::broadcast(
            "test_source",
            EventPayload::Lifecycle {
                phase: LifecyclePhase::Started,
                error: None,
            },
        );

        assert_eq!(event.source, "test_source");
        assert_eq!(event.target, EventTarget::Broadcast);
        assert!(matches!(
            event.payload,
            EventPayload::Lifecycle {
                phase: LifecyclePhase::Started,
                ..
            }
        ));
        assert!(!event.id.is_empty());
    }

    #[test]
    fn event_to_agent() {
        let event = Event::to_agent(
            "kernel",
            "agent-123".to_string(),
            EventPayload::System {
                category: SystemCategory::HealthCheck,
                message: "ping".to_string(),
            },
        );

        assert_eq!(event.target, EventTarget::Agent("agent-123".to_string()));
    }

    #[test]
    fn event_history_bounded() {
        let mut history = EventHistory::new(3);

        for i in 0..5 {
            history.push(Event::broadcast(
                "test",
                EventPayload::System {
                    category: SystemCategory::HealthCheck,
                    message: format!("event {}", i),
                },
            ));
        }

        assert_eq!(history.len(), 3);
        let all = history.all();
        assert_eq!(all.len(), 3);
        // Should contain the most recent 3 events (2, 3, 4)
    }

    #[test]
    fn event_history_get_limited() {
        let mut history = EventHistory::new(10);

        for i in 0..5 {
            history.push(Event::broadcast(
                "test",
                EventPayload::System {
                    category: SystemCategory::HealthCheck,
                    message: format!("event {}", i),
                },
            ));
        }

        let limited = history.get(Some(2));
        assert_eq!(limited.len(), 2);
    }

    #[test]
    fn event_describe() {
        let event = Event::broadcast(
            "kernel",
            EventPayload::Tool {
                name: "shell".to_string(),
                result: ToolResultSummary::Success,
                duration_ms: 150,
            },
        );

        let desc = event.describe();
        assert!(desc.contains("kernel"));
        assert!(desc.contains("shell"));
        assert!(desc.contains("150ms"));
    }

    #[test]
    fn event_custom_payload() {
        let event = Event::broadcast(
            "app",
            EventPayload::Custom {
                subtype: "user_action".to_string(),
                data: Some(serde_json::json!({"action": "click"})),
            },
        );

        assert!(event.describe().contains("user_action"));
    }
}
