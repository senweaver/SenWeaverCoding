// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub type EventId = String;

pub type AgentId = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventTarget {

    Agent(AgentId),

    Broadcast,

    System,

    #[serde(skip)]
    Pattern(String),
}

impl Default for EventTarget {
    fn default() -> Self {
        Self::Broadcast
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventPayload {

    Lifecycle {

        phase: LifecyclePhase,

        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },

    System {

        category: SystemCategory,

        message: String,
    },

    Memory {

        operation: MemoryOperation,

        #[serde(skip_serializing_if = "Option::is_none")]
        key: Option<String>,
    },

    Tool {

        name: String,

        result: ToolResultSummary,

        duration_ms: u64,
    },

    MessageReceived {

        channel: String,

        preview: String,
    },

    MessageSent {

        channel: String,

        preview: String,
    },

    AgentRequest {

        request_id: String,

        capability: String,

        prompt: String,

        #[serde(skip_serializing_if = "Option::is_none")]
        context: Option<serde_json::Value>,

        timeout_secs: u64,
    },

    AgentResponse {

        request_id: String,

        success: bool,

        output: String,

        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },

    TaskDelegation {

        task_id: String,

        action: TaskDelegationAction,

        description: String,
    },

    Coordination {

        action: CoordinationAction,

        topic: String,

        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<serde_json::Value>,
    },

    Custom {

        subtype: String,

        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<serde_json::Value>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LifecyclePhase {
    Spawned,
    Started,
    Stopped,
    Terminated,
    Error,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolResultSummary {
    Success,
    Error,
    Cancelled,
    Timeout,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskDelegationAction {

    Assigned,

    Accepted,

    Rejected,

    Progress,

    Completed,

    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CoordinationAction {

    LockRequest,

    LockGranted,

    LockDenied,

    LockRelease,

    Propose,

    Vote,

    Commit,

    BarrierReady,

    BarrierRelease,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {

    pub id: EventId,

    pub source: String,

    #[serde(default)]
    pub target: EventTarget,

    pub payload: EventPayload,

    pub timestamp: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<AgentId>,
}

impl Event {

    pub fn new(source: impl Into<String>, target: EventTarget, payload: EventPayload) -> Self {
        Self {
            id: format!(
                "evt-{}-{}",
                Utc::now().timestamp_millis(),
                &uuid::Uuid::new_v4().to_string()[..8]
            ),
            source: source.into(),
            target,
            payload,
            timestamp: Utc::now().to_rfc3339(),
            correlation_id: None,
            reply_to: None,
        }
    }

    pub fn with_correlation(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }

    pub fn with_reply_to(mut self, agent_id: impl Into<String>) -> Self {
        self.reply_to = Some(agent_id.into());
        self
    }

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

    pub fn broadcast(source: impl Into<String>, payload: EventPayload) -> Self {
        Self::new(source, EventTarget::Broadcast, payload)
    }

    pub fn to_agent(source: impl Into<String>, agent_id: AgentId, payload: EventPayload) -> Self {
        Self::new(source, EventTarget::Agent(agent_id), payload)
    }

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

#[derive(Debug, Clone)]
pub struct EventHistory {
    events: VecDeque<Event>,
    capacity: usize,
}

impl EventHistory {

    pub fn new(capacity: usize) -> Self {
        Self {
            events: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn push(&mut self, event: Event) {
        if self.events.len() >= self.capacity {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    pub fn get(&self, limit: Option<usize>) -> Vec<Event> {
        let count = limit.unwrap_or(self.events.len()).min(self.events.len());
        self.events.iter().rev().take(count).cloned().collect()
    }

    pub fn all(&self) -> Vec<Event> {
        self.events.iter().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }

    pub fn find_by_id(&self, id: EventId) -> Option<Event> {
        self.events.iter().find(|e| e.id == id).cloned()
    }
}

impl Default for EventHistory {
    fn default() -> Self {
        Self::new(1000)
    }
}
