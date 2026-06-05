// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;
use std::sync::LazyLock;

use parking_lot::RwLock;
use tokio::io::AsyncWriteExt;

use super::EventBusHandle;
use super::types::{
    Event, EventPayload, LifecyclePhase, MemoryOperation, SystemCategory, ToolResultSummary,
};

static GLOBAL_BUS: LazyLock<RwLock<Option<EventBusHandle>>> = LazyLock::new(|| RwLock::new(None));

pub fn init_global_bus(audit_path: Option<PathBuf>) -> EventBusHandle {
    let handle = EventBusHandle::new(super::EventBus::new());
    *GLOBAL_BUS.write() = Some(handle.clone());

    let mut rx = handle.subscribe_all();
    let _ =
        crate::runtime::spawn_supervised("event_bus.integration.audit_subscriber", async move {
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

            while let Ok(event) = rx.recv().await {
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
        });

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
