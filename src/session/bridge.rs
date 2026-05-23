// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use tokio::sync::broadcast;

use super::event::{SessionEvent, SessionEventKind};

#[derive(Clone)]
pub struct SessionEventSink {
    sender: broadcast::Sender<SessionEvent>,
}

impl SessionEventSink {
    pub fn new(sender: broadcast::Sender<SessionEvent>) -> Self {
        Self { sender }
    }

    pub fn emit_delta(&self, text: impl Into<String>) {
        let _ = self.sender.send(SessionEvent::new(SessionEventKind::Delta {
            text: text.into(),
        }));
        record_session_event_metric("delta");
    }

    pub fn emit_tool_call(
        &self,
        tool_name: impl Into<String>,
        tool_call_id: impl Into<String>,
        arguments: serde_json::Value,
    ) {
        let _ = self
            .sender
            .send(SessionEvent::new(SessionEventKind::ToolCall {
                tool_name: tool_name.into(),
                tool_call_id: tool_call_id.into(),
                arguments,
            }));
    }

    pub fn emit_tool_result(
        &self,
        tool_call_id: impl Into<String>,
        output: impl Into<String>,
        is_error: bool,
    ) {
        let _ = self
            .sender
            .send(SessionEvent::new(SessionEventKind::ToolResult {
                tool_call_id: tool_call_id.into(),
                output: output.into(),
                is_error,
            }));
    }

    pub fn emit_compressed(&self, tokens_before: usize, tokens_after: usize) {
        let _ = self
            .sender
            .send(SessionEvent::new(SessionEventKind::ContextCompressed {
                tokens_before,
                tokens_after,
            }));
    }

    pub fn emit_error(&self, message: impl Into<String>) {
        let _ = self.sender.send(SessionEvent::new(SessionEventKind::Error {
            message: message.into(),
        }));
    }

    pub fn emit_kind(&self, kind: SessionEventKind) {
        let _ = self.sender.send(SessionEvent::new(kind));
    }

    pub fn as_delta_callback(self) -> Arc<dyn Fn(&str) + Send + Sync> {
        let sink = self;
        Arc::new(move |chunk: &str| {
            sink.emit_delta(chunk);
        })
    }
}

pub(crate) fn record_session_event_metric(kind: &'static str) {
    if let Some(svc) = crate::services::try_get_services() {
        use crate::observability::agent_metrics::LabelSet;
        let labels = LabelSet::new(vec![("kind", kind)]);
        svc.agent_metrics.inc("sen_session_events_total", labels);
    }
}
