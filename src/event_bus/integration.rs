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

/// Global event bus instance, lazily initialized.
static GLOBAL_BUS: LazyLock<RwLock<Option<EventBusHandle>>> = LazyLock::new(|| RwLock::new(None));

/// Initialize the global event bus. Call once at startup.
pub fn init_global_bus() -> EventBusHandle {
    let handle = EventBusHandle::new(super::EventBus::new());
    *GLOBAL_BUS.write() = Some(handle.clone());
    handle
}

/// Get a reference to the global event bus, if initialized.
pub fn global_bus() -> Option<EventBusHandle> {
    GLOBAL_BUS.read().clone()
}

/// Publish an agent lifecycle event.
pub async fn publish_lifecycle(source: &str, phase: LifecyclePhase, error: Option<String>) {
    if let Some(bus) = global_bus() {
        bus.publish(Event::broadcast(
            source,
            EventPayload::Lifecycle { phase, error },
        ))
        .await;
    } else {
        // SECURITY: Events are silently dropped when bus is uninitialized.
        // This is expected during early startup. Consider logging at trace level
        // if debugging event delivery issues during initialization.
        tracing::trace!(
            source,
            ?phase,
            "Event dropped: global event bus not initialized"
        );
    }
}

/// Publish a system event.
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

/// Publish a tool execution event.
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

/// Publish a memory operation event.
pub async fn publish_memory_op(source: &str, operation: MemoryOperation, key: Option<String>) {
    if let Some(bus) = global_bus() {
        bus.publish(Event::broadcast(
            source,
            EventPayload::Memory { operation, key },
        ))
        .await;
    }
}

/// Publish a message received event.
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

/// Publish a message sent event.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_global_bus_init() {
        let handle = init_global_bus();
        assert!(global_bus().is_some());

        let mut rx = handle.subscribe_all();
        publish_system("test", SystemCategory::HealthCheck, "ping").await;

        let event = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await;
        assert!(event.is_ok());
    }

    #[tokio::test]
    async fn test_publish_lifecycle() {
        let handle = init_global_bus();
        let mut rx = handle.subscribe_all();

        publish_lifecycle("agent", LifecyclePhase::Started, None).await;

        let event = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(event.source, "agent");
        assert!(matches!(
            event.payload,
            EventPayload::Lifecycle {
                phase: LifecyclePhase::Started,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn test_publish_without_init() {
        publish_system("test", SystemCategory::Shutdown, "bye").await;
    }
}
