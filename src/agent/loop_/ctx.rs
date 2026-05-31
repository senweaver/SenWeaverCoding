// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::approval::ApprovalManager;
use crate::config::{MultimodalConfig, PacingConfig};
use crate::hooks::HookRunner;
use crate::observability::traits::Observer;
use crate::providers::traits::{ChatMessage, Provider};
use crate::security::rbac::{CallerIdentity, RbacEngine};
use crate::tools::{ActivatedToolSet, Tool};

pub struct LoopContext<'a> {

    pub provider: &'a dyn Provider,

    pub history: &'a mut Vec<ChatMessage>,

    pub tools_registry: &'a [Box<dyn Tool>],

    pub observer: &'a dyn Observer,

    pub provider_name: &'a str,

    pub model: &'a str,

    pub temperature: f64,

    pub silent: bool,

    pub approval: Option<&'a ApprovalManager>,

    pub rbac_engine: Option<&'a Arc<RbacEngine>>,

    pub rbac_identity: Option<&'a CallerIdentity>,

    pub channel_name: &'a str,

    pub channel_reply_target: Option<&'a str>,

    pub multimodal_config: &'a MultimodalConfig,

    pub max_tool_iterations: usize,

    pub cancellation_token: Option<CancellationToken>,

    pub on_delta: Option<tokio::sync::mpsc::Sender<crate::agent::loop_::DraftEvent>>,

    pub hooks: Option<&'a HookRunner>,

    pub excluded_tools: &'a [String],

    pub dedup_exempt_tools: &'a [String],

    pub activated_tools: Option<&'a Arc<parking_lot::Mutex<ActivatedToolSet>>>,

    pub model_switch_callback: Option<crate::agent::loop_::ModelSwitchCallback>,

    pub pacing: &'a PacingConfig,

    pub plan_mode_flag: Option<&'a crate::tools::PlanModeFlag>,

    pub tool_registry: Option<&'a crate::tools::registry::ToolRegistry>,
}

impl core::fmt::Debug for LoopContext<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LoopContext")
            .field("provider_name", &self.provider_name)
            .field("model", &self.model)
            .field("temperature", &self.temperature)
            .field("silent", &self.silent)
            .field("channel_name", &self.channel_name)
            .field("channel_reply_target", &self.channel_reply_target)
            .field("max_tool_iterations", &self.max_tool_iterations)
            .field("excluded_tools", &self.excluded_tools)
            .field("dedup_exempt_tools", &self.dedup_exempt_tools)
            .field("tool_registry", &self.tool_registry.is_some())
            .finish()
    }
}

impl<'a> LoopContext<'a> {

    pub fn new(
        provider: &'a dyn Provider,
        history: &'a mut Vec<ChatMessage>,
        tools_registry: &'a [Box<dyn Tool>],
        observer: &'a dyn Observer,
        provider_name: &'a str,
        model: &'a str,
        multimodal_config: &'a MultimodalConfig,
        pacing: &'a PacingConfig,
    ) -> Self {
        Self {
            provider,
            history,
            tools_registry,
            observer,
            provider_name,
            model,
            temperature: 0.7,
            silent: false,
            approval: None,
            rbac_engine: None,
            rbac_identity: None,
            channel_name: "cli",
            channel_reply_target: None,
            multimodal_config,

            max_tool_iterations: crate::config::default_agent_max_tool_iterations(),
            cancellation_token: None,
            on_delta: None,
            hooks: None,
            excluded_tools: &[],
            dedup_exempt_tools: &[],
            activated_tools: None,
            model_switch_callback: None,
            pacing,
            plan_mode_flag: None,
            tool_registry: None,
        }
    }

    pub fn model_config(
        mut self,
        provider_name: &'a str,
        model: &'a str,
        temperature: f64,
    ) -> Self {
        self.provider_name = provider_name;
        self.model = model;
        self.temperature = temperature;
        self
    }

    pub fn channel(mut self, name: &'a str, reply_target: Option<&'a str>) -> Self {
        self.channel_name = name;
        self.channel_reply_target = reply_target;
        self
    }

    pub fn max_iterations(mut self, n: usize) -> Self {
        self.max_tool_iterations = n;
        self
    }

    pub fn cancellation(mut self, token: CancellationToken) -> Self {
        self.cancellation_token = Some(token);
        self
    }

    pub fn streaming(
        mut self,
        tx: tokio::sync::mpsc::Sender<crate::agent::loop_::DraftEvent>,
    ) -> Self {
        self.on_delta = Some(tx);
        self
    }

    pub fn rbac(mut self, engine: &'a Arc<RbacEngine>, identity: &'a CallerIdentity) -> Self {
        self.rbac_engine = Some(engine);
        self.rbac_identity = Some(identity);
        self
    }

    pub fn silent(mut self, yes: bool) -> Self {
        self.silent = yes;
        self
    }

    pub fn excluded_tools(mut self, tools: &'a [String]) -> Self {
        self.excluded_tools = tools;
        self
    }

    pub fn approval_manager(mut self, manager: &'a ApprovalManager) -> Self {
        self.approval = Some(manager);
        self
    }

    #[allow(clippy::type_complexity)]
    pub fn into_loop_params(
        self,
    ) -> (
        &'a dyn Provider,
        &'a mut Vec<ChatMessage>,
        &'a [Box<dyn Tool>],
        &'a dyn Observer,
        &'a str,
        &'a str,
        f64,
        bool,
        Option<&'a ApprovalManager>,
        &'a str,
        Option<&'a str>,
        &'a MultimodalConfig,
        usize,
        Option<CancellationToken>,
        Option<tokio::sync::mpsc::Sender<crate::agent::loop_::DraftEvent>>,
        Option<&'a HookRunner>,
        &'a [String],
        &'a [String],
        Option<&'a Arc<parking_lot::Mutex<ActivatedToolSet>>>,
        Option<crate::agent::loop_::ModelSwitchCallback>,
        &'a PacingConfig,
        Option<&'a Arc<RbacEngine>>,
        Option<&'a CallerIdentity>,
        Option<&'a crate::tools::PlanModeFlag>,
        Option<&'a crate::tools::registry::ToolRegistry>,
    ) {
        (
            self.provider,
            self.history,
            self.tools_registry,
            self.observer,
            self.provider_name,
            self.model,
            self.temperature,
            self.silent,
            self.approval,
            self.channel_name,
            self.channel_reply_target,
            self.multimodal_config,
            self.max_tool_iterations,
            self.cancellation_token,
            self.on_delta,
            self.hooks,
            self.excluded_tools,
            self.dedup_exempt_tools,
            self.activated_tools,
            self.model_switch_callback,
            self.pacing,
            self.rbac_engine,
            self.rbac_identity,
            self.plan_mode_flag,
            self.tool_registry,
        )
    }
}
