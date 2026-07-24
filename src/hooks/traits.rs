// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use async_trait::async_trait;
use serde_json::Value;
use std::time::Duration;

use crate::providers::traits::{ChatMessage, ChatResponse};
use crate::tools::traits::ToolResult;

#[derive(Debug, Clone)]
pub enum HookResult<T> {
    Continue(T),
    Cancel(String),
    RequireApproval(T, Option<String>),
}

impl<T> HookResult<T> {
    pub fn is_cancel(&self) -> bool {
        matches!(self, HookResult::Cancel(_))
    }
}

#[async_trait]
pub trait HookHandler: Send + Sync {
    fn name(&self) -> &str;
    fn priority(&self) -> i32 {
        0
    }

    async fn on_gateway_start(&self, _host: &str, _port: u16) {}
    async fn on_session_start(&self, _session_id: &str, _channel: &str) {}
    async fn on_session_end(&self, _session_id: &str, _channel: &str) {}
    async fn on_llm_input(&self, _messages: &[ChatMessage], _model: &str) {}
    async fn on_llm_output(&self, _response: &ChatResponse) {}
    async fn on_after_tool_call(&self, _tool: &str, _result: &ToolResult, _duration: Duration) {}
    async fn on_turn_end(&self, _channel: &str, _final_text: &str, _tools_used: &[String]) {}
    async fn on_subagent_stop(&self, _worker_id: &str, _status: &str, _summary: &str) {}
    async fn on_pre_compact(&self, _trigger: &str, _estimated_tokens: usize) {}
    async fn on_notification(&self, _kind: &str, _message: &str) {}

    async fn before_prompt_build(&self, prompt: String) -> HookResult<String> {
        HookResult::Continue(prompt)
    }

    async fn before_tool_call(&self, name: String, args: Value) -> HookResult<(String, Value)> {
        HookResult::Continue((name, args))
    }
}
