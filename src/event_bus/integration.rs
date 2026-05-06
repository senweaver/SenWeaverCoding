// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! EventBus integration helpers for agent and gateway systems.
//!
//! Provides a global EventBus instance and convenience functions
//! for publishing events from anywhere in the system.

use std::sync::LazyLock;

use parking_lot::RwLock;

use super::EventBusHandle;
use super::types::{
    Event, EventPayload, LifecyclePhase, MemoryOperation, SystemCategory, ToolResultSummary,
};

static GLOBAL_BUS: LazyLock<RwLock<Option<EventBusHandle>>> = LazyLock::new(|| RwLock::new(None));

pub fn init_global_bus() -> EventBusHandle {
    let handle = EventBusHandle::new(super::EventBus::new());
    *GLOBAL_BUS.write() = Some(handle.clone());

    let mut rx = handle.subscribe_all();
    let _ =
        crate::runtime::spawn_supervised("event_bus.integration.logging_subscriber", async move {
            while let Ok(event) = rx.recv().await {
                tracing::debug!(
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
