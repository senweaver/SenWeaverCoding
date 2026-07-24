// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::Path;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use arc_swap::ArcSwapOption;
use serde_json::Value;

use crate::config::Config;
use crate::providers::traits::{ChatMessage, ChatResponse};
use crate::tools::traits::ToolResult;

use super::runner::HookRunner;
use super::traits::HookResult;

pub struct HotHookRunner {
    inner: ArcSwapOption<HookRunner>,
}

impl HotHookRunner {

    #[must_use]
    pub fn empty() -> Arc<Self> {
        Arc::new(Self {
            inner: ArcSwapOption::empty(),
        })
    }

    pub fn rebuild(&self, config: &Config, workspace_dir: &Path) {
        let new_runner = build_runner(config, workspace_dir);
        self.inner.store(new_runner);
    }

    #[must_use]
    pub fn current(&self) -> Option<Arc<HookRunner>> {
        self.inner.load_full()
    }

    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.inner.load().is_some()
    }

    pub async fn fire_gateway_start(&self, host: &str, port: u16) {
        if let Some(r) = self.current() {
            r.fire_gateway_start(host, port).await;
        }
    }

    pub async fn fire_session_start(&self, session_id: &str, channel: &str) {
        if let Some(r) = self.current() {
            r.fire_session_start(session_id, channel).await;
        }
    }

    pub async fn fire_session_end(&self, session_id: &str, channel: &str) {
        if let Some(r) = self.current() {
            r.fire_session_end(session_id, channel).await;
        }
    }

    pub async fn fire_llm_input(&self, messages: &[ChatMessage], model: &str) {
        if let Some(r) = self.current() {
            r.fire_llm_input(messages, model).await;
        }
    }

    pub async fn fire_llm_output(&self, response: &ChatResponse) {
        if let Some(r) = self.current() {
            r.fire_llm_output(response).await;
        }
    }

    pub async fn fire_after_tool_call(&self, tool: &str, result: &ToolResult, duration: Duration) {
        if let Some(r) = self.current() {
            r.fire_after_tool_call(tool, result, duration).await;
        }
    }

    pub async fn fire_turn_end(&self, channel: &str, final_text: &str, tools_used: &[String]) {
        if let Some(r) = self.current() {
            r.fire_turn_end(channel, final_text, tools_used).await;
        }
    }

    pub async fn fire_subagent_stop(&self, worker_id: &str, status: &str, summary: &str) {
        if let Some(r) = self.current() {
            r.fire_subagent_stop(worker_id, status, summary).await;
        }
    }

    pub async fn fire_pre_compact(&self, trigger: &str, estimated_tokens: usize) {
        if let Some(r) = self.current() {
            r.fire_pre_compact(trigger, estimated_tokens).await;
        }
    }

    pub async fn fire_notification(&self, kind: &str, message: &str) {
        if let Some(r) = self.current() {
            r.fire_notification(kind, message).await;
        }
    }

    pub async fn run_before_prompt_build(&self, prompt: String) -> HookResult<String> {
        if let Some(r) = self.current() {
            r.run_before_prompt_build(prompt).await
        } else {
            HookResult::Continue(prompt)
        }
    }

    pub async fn run_before_tool_call(
        &self,
        name: String,
        args: Value,
    ) -> HookResult<(String, Value)> {
        if let Some(r) = self.current() {
            r.run_before_tool_call(name, args).await
        } else {
            HookResult::Continue((name, args))
        }
    }
}

static GLOBAL_HOOKS: OnceLock<Arc<HotHookRunner>> = OnceLock::new();

pub fn install_global_hooks(runner: Arc<HotHookRunner>) {
    let _ = GLOBAL_HOOKS.set(runner);
}

#[must_use]
pub fn global_hooks() -> Option<Arc<HotHookRunner>> {
    GLOBAL_HOOKS.get().cloned()
}

#[must_use]
pub fn build_runner(config: &Config, workspace_dir: &Path) -> Option<Arc<HookRunner>> {
    if !config.hooks.enabled {
        return None;
    }

    let hook_schema = crate::schemas::hooks::HookSchema::default_schema();
    if config.hooks.builtin.webhook_audit.enabled {
        let audit_cfg = serde_json::to_value(&config.hooks.builtin.webhook_audit).unwrap_or_default();
        if let Err(errors) =
            crate::schemas::hooks::validate_hook_config(&hook_schema, "post_tool_use", &audit_cfg)
        {
            tracing::warn!(
                ?errors,
                "webhook-audit hook config failed schema validation; continuing with built-in checks"
            );
        }
    }

    let mut runner = HookRunner::new();

    if config.hooks.builtin.command_logger {
        runner.register(Box::new(
            super::builtin::command_logger::CommandLoggerHook::new(),
        ));
    }

    match super::builtin::webhook_audit::WebhookAuditHook::new(
        config.hooks.builtin.webhook_audit.clone(),
    ) {
        Ok(hook) => runner.register(Box::new(hook)),
        Err(e) => {
            tracing::error!(
                hook = "webhook-audit",
                error = %e,
                "skipping webhook-audit hook registration due to invalid configuration"
            );
        }
    }

    let script_runner = super::script_runner::ScriptHookRunner::load_default(
        workspace_dir.to_path_buf(),
        config.hooks.allow_workspace_hooks,
    );
    if script_runner.source_count() > 0 {
        tracing::info!(
            sources = script_runner.source_count(),
            "loaded hooks.json script runner sources"
        );
        runner.register(Box::new(script_runner));
    }

    Some(Arc::new(runner))
}
