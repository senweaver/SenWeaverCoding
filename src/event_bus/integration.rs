// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;
use std::sync::LazyLock;

use parking_lot::RwLock;
use tokio::io::AsyncWriteExt;

use super::EventBusHandle;
use super::types::{
    CoordinationAction, Event, EventPayload, EventTarget, LifecyclePhase, MemoryOperation,
    SystemCategory, TaskDelegationAction, ToolResultSummary,
};

static GLOBAL_BUS: LazyLock<RwLock<Option<EventBusHandle>>> = LazyLock::new(|| RwLock::new(None));

static AUDIT_SUBSCRIBER: LazyLock<RwLock<Option<crate::runtime::task_manager::TaskHandle>>> =
    LazyLock::new(|| RwLock::new(None));

const AUDIT_QUEUE_CAPACITY: usize = 512;

pub fn init_global_bus(audit_path: Option<PathBuf>) -> EventBusHandle {
    let handle = {
        let mut guard = GLOBAL_BUS.write();
        if let Some(existing) = guard.as_ref() {
            tracing::debug!(
                target: "event_bus",
                "init_global_bus called again; reusing the existing global event bus \
                 (subscribers stay attached)"
            );
            return existing.clone();
        }
        let handle = EventBusHandle::new(super::EventBus::new());
        *guard = Some(handle.clone());
        handle
    };

    let mut rx = super::BoundedSubscriber::new(handle.subscribe_all(), AUDIT_QUEUE_CAPACITY);
    let task = crate::runtime::task_manager::spawn_supervised(
        "event_bus.integration.audit_subscriber",
        async move {
            let mut writer = match audit_path {
                Some(path) => {
                    if let Some(parent) = path.parent() {
                        let _ = tokio::fs::create_dir_all(parent).await;
                    }
                    match tokio::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&path)
                        .await
                    {
                        Ok(file) => Some(tokio::io::BufWriter::new(file)),
                        Err(err) => {
                            tracing::warn!(
                                target: "event_bus",
                                error = %err,
                                path = %path.display(),
                                "failed to open event audit log; events will not be persisted"
                            );
                            None
                        }
                    }
                }
                None => None,
            };

            loop {
                let event = match rx.recv().await {
                    Some(event) => event,
                    None => match global_bus() {
                        Some(bus) => {
                            tracing::warn!(
                                target: "event_bus",
                                "audit subscriber channel closed; resubscribing to current global bus"
                            );
                            rx = super::BoundedSubscriber::new(
                                bus.subscribe_all(),
                                AUDIT_QUEUE_CAPACITY,
                            );
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                            continue;
                        }
                        None => {
                            tracing::info!(
                                target: "event_bus",
                                "audit subscriber stopping: global event bus dropped"
                            );
                            break;
                        }
                    },
                };

                let mut disable_writer = false;
                if let Some(w) = writer.as_mut() {
                    match serde_json::to_string(&event) {
                        Ok(line) => {
                            if w.write_all(line.as_bytes()).await.is_err()
                                || w.write_all(b"\n").await.is_err()
                                || w.flush().await.is_err()
                            {
                                disable_writer = true;
                            }
                        }
                        Err(err) => {
                            tracing::debug!(
                                target: "event_bus",
                                error = %err,
                                "failed to serialize event for audit log"
                            );
                        }
                    }
                }
                if disable_writer {
                    tracing::warn!(
                        target: "event_bus",
                        "event audit log write failed; disabling persistence for this run"
                    );
                    writer = None;
                }
                tracing::trace!(
                    source = %event.source,
                    "event_bus: {:?}",
                    event.payload,
                );
            }
        },
    );

    {
        let mut guard = AUDIT_SUBSCRIBER.write();
        if let Some(previous) = guard.take() {
            previous.abort();
        }
        *guard = Some(task);
    }

    handle
}

pub fn global_bus() -> Option<EventBusHandle> {
    GLOBAL_BUS.read().clone()
}

pub async fn publish_lifecycle(source: &str, phase: LifecyclePhase, error: Option<String>) {
    if let Some(bus) = global_bus() {
        bus.publish(Event::broadcast(
            source,
            EventPayload::Lifecycle { phase, error },
        ))
        .await;
    } else {

        tracing::trace!(
            source,
            ?phase,
            "Event dropped: global event bus not initialized"
        );
    }
}

pub async fn publish_system(source: &str, category: SystemCategory, message: &str) {
    if let Some(bus) = global_bus() {
        bus.publish(Event::system(source, category, message)).await;
    } else {
        tracing::trace!(
            source,
            ?category,
            "System event dropped: global event bus not initialized"
        );
    }
}

pub async fn publish_tool_call(source: &str, tool_name: &str, success: bool, duration_ms: u64) {
    if let Some(bus) = global_bus() {
        let result = if success {
            ToolResultSummary::Success
        } else {
            ToolResultSummary::Error
        };
        bus.publish(Event::broadcast(
            source,
            EventPayload::Tool {
                name: tool_name.to_string(),
                result,
                duration_ms,
            },
        ))
        .await;
    }
}

pub async fn publish_memory_op(source: &str, operation: MemoryOperation, key: Option<String>) {
    if let Some(bus) = global_bus() {
        bus.publish(Event::broadcast(
            source,
            EventPayload::Memory { operation, key },
        ))
        .await;
    }
}

pub async fn publish_message_received(source: &str, channel: &str, preview: &str) {
    if let Some(bus) = global_bus() {
        bus.publish(Event::broadcast(
            source,
            EventPayload::MessageReceived {
                channel: channel.to_string(),
                preview: preview.chars().take(100).collect(),
            },
        ))
        .await;
    }
}

pub async fn publish_message_sent(source: &str, channel: &str, preview: &str) {
    if let Some(bus) = global_bus() {
        bus.publish(Event::broadcast(
            source,
            EventPayload::MessageSent {
                channel: channel.to_string(),
                preview: preview.chars().take(100).collect(),
            },
        ))
        .await;
    }
}

pub fn publish_agent_request_now(
    source: &str,
    target_agent: &str,
    request_id: &str,
    capability: &str,
    prompt: &str,
    timeout_secs: u64,
) {
    if let Some(bus) = global_bus() {
        bus.publish_now(Event::agent_request(
            source,
            target_agent.to_string(),
            request_id,
            capability,
            prompt.chars().take(200).collect::<String>(),
            timeout_secs,
        ));
    }
}

pub fn publish_agent_response_now(
    source: &str,
    target_agent: &str,
    request_id: &str,
    success: bool,
    output: &str,
    error: Option<String>,
) {
    if let Some(bus) = global_bus() {
        bus.publish_now(Event::agent_response(
            source,
            target_agent.to_string(),
            request_id,
            success,
            output.chars().take(200).collect::<String>(),
            error.map(|e| e.chars().take(200).collect()),
        ));
    }
}

pub fn publish_task_delegation_now(
    source: &str,
    task_id: &str,
    action: TaskDelegationAction,
    description: &str,
) {
    if let Some(bus) = global_bus() {
        bus.publish_now(Event::broadcast(
            source,
            EventPayload::TaskDelegation {
                task_id: task_id.to_string(),
                action,
                description: description.chars().take(200).collect(),
            },
        ));
    }
}

pub fn publish_coordination_now(
    source: &str,
    action: CoordinationAction,
    topic: &str,
    data: Option<serde_json::Value>,
) {
    if let Some(bus) = global_bus() {
        bus.publish_now(Event::new(
            source,
            EventTarget::System,
            EventPayload::Coordination {
                action,
                topic: topic.to_string(),
                data,
            },
        ));
    }
}
