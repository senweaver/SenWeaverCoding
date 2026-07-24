// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use std::time::Duration;

use futures_util::{FutureExt, future::join_all};
use serde_json::Value;
use std::panic::AssertUnwindSafe;
use tracing::info;

use crate::providers::traits::{ChatMessage, ChatResponse};
use crate::tools::traits::ToolResult;

use super::traits::{HookHandler, HookResult};

pub struct HookRunner {
    handlers: Vec<Box<dyn HookHandler>>,
}

impl HookRunner {

    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    pub fn register(&mut self, handler: Box<dyn HookHandler>) {
        self.handlers.push(handler);
        self.handlers
            .sort_by_key(|h| std::cmp::Reverse(h.priority()));
    }

    async fn join_isolated<'a, F>(event: &str, futs: Vec<(&'a str, F)>)
    where
        F: std::future::Future<Output = ()>,
    {
        let names: Vec<&'a str> = futs.iter().map(|(name, _)| *name).collect();
        let wrapped: Vec<_> = futs
            .into_iter()
            .map(|(_, fut)| AssertUnwindSafe(fut).catch_unwind())
            .collect();
        let results = join_all(wrapped).await;
        for (name, result) in names.into_iter().zip(results) {
            if result.is_err() {
                tracing::error!(
                    target: "hooks",
                    hook = name,
                    hook_event = event,
                    "hook handler panicked; isolated to protect the agent turn"
                );
            }
        }
    }

    pub async fn fire_gateway_start(&self, host: &str, port: u16) {
        let futs: Vec<_> = self
            .handlers
            .iter()
            .map(|h| (h.name(), h.on_gateway_start(host, port)))
            .collect();
        Self::join_isolated("gateway_start", futs).await;
    }

    pub async fn fire_session_start(&self, session_id: &str, channel: &str) {
        let futs: Vec<_> = self
            .handlers
            .iter()
            .map(|h| (h.name(), h.on_session_start(session_id, channel)))
            .collect();
        Self::join_isolated("session_start", futs).await;
    }

    pub async fn fire_session_end(&self, session_id: &str, channel: &str) {
        let futs: Vec<_> = self
            .handlers
            .iter()
            .map(|h| (h.name(), h.on_session_end(session_id, channel)))
            .collect();
        Self::join_isolated("session_end", futs).await;
    }

    pub async fn fire_llm_input(&self, messages: &[ChatMessage], model: &str) {
        let futs: Vec<_> = self
            .handlers
            .iter()
            .map(|h| (h.name(), h.on_llm_input(messages, model)))
            .collect();
        Self::join_isolated("llm_input", futs).await;
    }

    pub async fn fire_llm_output(&self, response: &ChatResponse) {
        let futs: Vec<_> = self
            .handlers
            .iter()
            .map(|h| (h.name(), h.on_llm_output(response)))
            .collect();
        Self::join_isolated("llm_output", futs).await;
    }

    pub async fn fire_after_tool_call(&self, tool: &str, result: &ToolResult, duration: Duration) {
        let futs: Vec<_> = self
            .handlers
            .iter()
            .map(|h| (h.name(), h.on_after_tool_call(tool, result, duration)))
            .collect();
        Self::join_isolated("after_tool_call", futs).await;
    }

    pub async fn fire_turn_end(&self, channel: &str, final_text: &str, tools_used: &[String]) {
        let futs: Vec<_> = self
            .handlers
            .iter()
            .map(|h| (h.name(), h.on_turn_end(channel, final_text, tools_used)))
            .collect();
        Self::join_isolated("turn_end", futs).await;
    }

    pub async fn fire_subagent_stop(&self, worker_id: &str, status: &str, summary: &str) {
        let futs: Vec<_> = self
            .handlers
            .iter()
            .map(|h| (h.name(), h.on_subagent_stop(worker_id, status, summary)))
            .collect();
        Self::join_isolated("subagent_stop", futs).await;
    }

    pub async fn fire_pre_compact(&self, trigger: &str, estimated_tokens: usize) {
        let futs: Vec<_> = self
            .handlers
            .iter()
            .map(|h| (h.name(), h.on_pre_compact(trigger, estimated_tokens)))
            .collect();
        Self::join_isolated("pre_compact", futs).await;
    }

    pub async fn fire_notification(&self, kind: &str, message: &str) {
        let futs: Vec<_> = self
            .handlers
            .iter()
            .map(|h| (h.name(), h.on_notification(kind, message)))
            .collect();
        Self::join_isolated("notification", futs).await;
    }

    pub async fn run_before_prompt_build(&self, mut prompt: String) -> HookResult<String> {
        for h in &self.handlers {
            let hook_name = h.name();
            match AssertUnwindSafe(h.before_prompt_build(prompt.clone()))
                .catch_unwind()
                .await
            {
                Ok(HookResult::Continue(p)) => prompt = p,
                Ok(HookResult::RequireApproval(_, message)) => {
                    let reason = message
                        .unwrap_or_else(|| "manual approval required".to_string());
                    info!(
                        hook = hook_name,
                        reason, "before_prompt_build ask degraded to cancel"
                    );
                    return HookResult::Cancel(format!(
                        "hooks.json requested user confirmation: {reason}"
                    ));
                }
                Ok(HookResult::Cancel(reason)) => {
                    info!(
                        hook = hook_name,
                        reason, "before_prompt_build cancelled by hook"
                    );
                    return HookResult::Cancel(reason);
                }
                Err(_) => {
                    tracing::error!(
                        hook = hook_name,
                        "before_prompt_build hook panicked; continuing with previous value"
                    );
                }
            }
        }
        HookResult::Continue(prompt)
    }

    pub async fn run_before_tool_call(
        &self,
        mut name: String,
        mut args: Value,
    ) -> HookResult<(String, Value)> {
        let mut approval_message: Option<String> = None;
        for h in &self.handlers {
            let hook_name = h.name();
            match AssertUnwindSafe(h.before_tool_call(name.clone(), args.clone()))
                .catch_unwind()
                .await
            {
                Ok(HookResult::Continue((n, a))) => {
                    name = n;
                    args = a;
                }
                Ok(HookResult::RequireApproval((n, a), message)) => {
                    name = n;
                    args = a;
                    info!(
                        hook = hook_name,
                        "before_tool_call requires user approval (hook ask)"
                    );
                    if approval_message.is_none() {
                        approval_message = Some(message.unwrap_or_else(|| {
                            "manual approval required by hooks.json".to_string()
                        }));
                    }
                }
                Ok(HookResult::Cancel(reason)) => {
                    info!(
                        hook = hook_name,
                        reason, "before_tool_call cancelled by hook"
                    );
                    return HookResult::Cancel(reason);
                }
                Err(_) => {
                    tracing::error!(
                        hook = hook_name,
                        "before_tool_call hook panicked; continuing with previous values"
                    );
                }
            }
        }
        match approval_message {
            Some(message) => HookResult::RequireApproval((name, args), Some(message)),
            None => HookResult::Continue((name, args)),
        }
    }
}
