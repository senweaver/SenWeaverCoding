// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Observer, ObserverEvent, ObserverMetric};
use std::any::Any;

pub struct VerboseObserver;

impl VerboseObserver {
    pub fn new() -> Self {
        Self
    }
}

impl Observer for VerboseObserver {
    fn record_event(&self, event: &ObserverEvent) {
        match event {
            ObserverEvent::LlmRequest {
                provider,
                model,
                messages_count,
            } => {
                tracing::info!("> Thinking");
                tracing::info!(
                    "> Send (provider={}, model={}, messages={})",
                    provider,
                    model,
                    messages_count
                );
            }
            ObserverEvent::LlmResponse {
                duration, success, ..
            } => {
                let ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
                tracing::info!("< Receive (success={success}, duration_ms={ms})");
            }
            ObserverEvent::ToolCallStart { tool, .. } => {
                tracing::info!("> Tool {tool}");
            }
            ObserverEvent::ToolCall {
                tool,
                duration,
                success,
            } => {
                let ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
                tracing::info!("< Tool {tool} (success={success}, duration_ms={ms})");
            }
            ObserverEvent::TurnComplete => {
                tracing::info!("< Complete");
            }
            _ => {}
        }
    }

    #[inline(always)]
    fn record_metric(&self, _metric: &ObserverMetric) {}

    fn name(&self) -> &str {
        "verbose"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
