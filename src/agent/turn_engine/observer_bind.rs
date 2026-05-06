// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Canonical `ObserverEvent` emission helpers for tool execution.
//!
//! Prior to D2.1, every code path that invoked a tool built
//! its own `ObserverEvent::ToolCallStart` / `ToolCall` payloads.  The
//! shape is small but drift produced subtle differences (e.g. missing
//! arg summaries on the error path).  These helpers centralize the
//! wire format so new call sites get it right by construction.

use std::time::Duration;

use crate::observability::{Observer, ObserverEvent};

pub fn emit_tool_call_start(observer: &dyn Observer, tool: &str, args: &serde_json::Value) {
    let summary = crate::util::truncate_with_ellipsis(&args.to_string(), 300);
    observer.record_event(&ObserverEvent::ToolCallStart {
        tool: tool.to_string(),
        arguments: Some(summary),
    });
}

pub fn emit_tool_call_end(observer: &dyn Observer, tool: &str, duration: Duration, success: bool) {
    observer.record_event(&ObserverEvent::ToolCall {
        tool: tool.to_string(),
        duration,
        success,
    });
}
