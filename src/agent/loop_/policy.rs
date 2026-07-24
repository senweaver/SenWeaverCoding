// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use crate::agent::event_sink::EventSink;
use crate::agent::loop_::{DraftEvent, ModelSwitchCallback};
use crate::agent::loop_::traits::{
    ExperienceRecorderHook, GuiModelSwitchHook, IterationContextBudgetHook, MemorySessionHook,
    PlanModeNudgeHook, ResponseCacheHook, TurnPreambleHook,
};
use crate::approval::ApprovalManager;
use crate::config::PacingConfig;
use crate::hooks::HookRunner;
use crate::i18n::ToolDescriptions;
use crate::observability::traits::Observer;
use crate::providers::traits::Provider;
use crate::security::rbac::{CallerIdentity, RbacEngine};
use crate::tools::{ActivatedToolSet, PlanModeFlag, Tool, ToolRegistry};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopOrigin {
    Gui,
    Cli,
    Delegated,
    Channel,
}

pub struct PolicyBundle<'a> {
    pub origin: LoopOrigin,
    pub provider: &'a dyn Provider,
    pub tools_registry: &'a [Box<dyn Tool>],
    pub observer: &'a dyn Observer,

    pub provider_name: &'a str,
    pub model: &'a str,
    pub temperature: f64,
    pub silent: bool,

    pub approval: Option<&'a ApprovalManager>,

    pub channel_name: &'a str,
    pub channel_reply_target: Option<&'a str>,

    pub multimodal_config: &'a crate::config::MultimodalConfig,
    pub max_tool_iterations: usize,
    pub cancellation_token: Option<CancellationToken>,

    pub on_delta: Option<tokio::sync::mpsc::Sender<DraftEvent>>,
    pub event_sink: EventSink,
    pub hooks: Option<&'a HookRunner>,

    pub excluded_tools: &'a [String],
    pub dedup_exempt_tools: &'a [String],

    pub activated_tools: Option<&'a Arc<Mutex<ActivatedToolSet>>>,
    pub model_switch_callback: Option<ModelSwitchCallback>,

    pub pacing: &'a PacingConfig,
    pub rbac_engine: Option<&'a Arc<RbacEngine>>,
    pub rbac_identity: Option<&'a CallerIdentity>,

    pub plan_mode_flag: Option<&'a PlanModeFlag>,

    pub plan_execution_path: Option<&'a str>,

    pub tool_registry: Option<&'a ToolRegistry>,

    pub response_cache_hook: Option<Arc<dyn ResponseCacheHook>>,
    pub memory_session_hook: Option<Arc<dyn MemorySessionHook>>,
    pub turn_preamble_hook: Option<Arc<dyn TurnPreambleHook>>,
    pub gui_model_switch_hook: Option<Arc<dyn GuiModelSwitchHook>>,
    pub iteration_context_budget_hook: Option<Arc<dyn IterationContextBudgetHook>>,
    pub experience_recorder_hook: Option<Arc<dyn ExperienceRecorderHook>>,
    pub plan_mode_nudge_hook: Option<Arc<dyn PlanModeNudgeHook>>,

    pub tool_descriptions: Option<&'a ToolDescriptions>,
}

impl<'a> PolicyBundle<'a> {
    #[must_use]
    pub fn new(
        origin: LoopOrigin,
        provider: &'a dyn Provider,
        tools_registry: &'a [Box<dyn Tool>],
        observer: &'a dyn Observer,
        provider_name: &'a str,
        model: &'a str,
        multimodal_config: &'a crate::config::MultimodalConfig,
        pacing: &'a PacingConfig,
        excluded_tools: &'a [String],
        dedup_exempt_tools: &'a [String],
    ) -> Self {
        Self {
            origin,
            provider,
            tools_registry,
            observer,
            provider_name,
            model,
            temperature: 0.7,
            silent: matches!(origin, LoopOrigin::Delegated | LoopOrigin::Gui),
            approval: None,
            channel_name: match origin {
                LoopOrigin::Gui => "gui",
                LoopOrigin::Cli => "cli",
                LoopOrigin::Delegated => "delegate",
                LoopOrigin::Channel => "channel",
            },
            channel_reply_target: None,
            multimodal_config,
            max_tool_iterations: crate::config::default_agent_max_tool_iterations(),
            cancellation_token: None,
            on_delta: None,
            event_sink: EventSink::none(),
            hooks: None,
            excluded_tools,
            dedup_exempt_tools,
            activated_tools: None,
            model_switch_callback: None,
            pacing,
            rbac_engine: None,
            rbac_identity: None,
            plan_mode_flag: None,
            plan_execution_path: None,
            tool_registry: None,
            response_cache_hook: None,
            memory_session_hook: None,
            turn_preamble_hook: None,
            gui_model_switch_hook: None,
            iteration_context_budget_hook: None,
            experience_recorder_hook: None,
            plan_mode_nudge_hook: None,
            tool_descriptions: None,
        }
    }

    #[must_use]
    pub fn gui(
        provider: &'a dyn Provider,
        tools_registry: &'a [Box<dyn Tool>],
        observer: &'a dyn Observer,
        provider_name: &'a str,
        model: &'a str,
        multimodal_config: &'a crate::config::MultimodalConfig,
        pacing: &'a PacingConfig,
        excluded_tools: &'a [String],
        dedup_exempt_tools: &'a [String],
    ) -> Self {
        Self::new(
            LoopOrigin::Gui,
            provider,
            tools_registry,
            observer,
            provider_name,
            model,
            multimodal_config,
            pacing,
            excluded_tools,
            dedup_exempt_tools,
        )
    }

    #[must_use]
    pub fn cli(
        provider: &'a dyn Provider,
        tools_registry: &'a [Box<dyn Tool>],
        observer: &'a dyn Observer,
        provider_name: &'a str,
        model: &'a str,
        multimodal_config: &'a crate::config::MultimodalConfig,
        pacing: &'a PacingConfig,
        excluded_tools: &'a [String],
        dedup_exempt_tools: &'a [String],
    ) -> Self {
        Self::new(
            LoopOrigin::Cli,
            provider,
            tools_registry,
            observer,
            provider_name,
            model,
            multimodal_config,
            pacing,
            excluded_tools,
            dedup_exempt_tools,
        )
    }

    #[must_use]
    pub fn delegated(
        provider: &'a dyn Provider,
        tools_registry: &'a [Box<dyn Tool>],
        observer: &'a dyn Observer,
        provider_name: &'a str,
        model: &'a str,
        multimodal_config: &'a crate::config::MultimodalConfig,
        pacing: &'a PacingConfig,
        excluded_tools: &'a [String],
        dedup_exempt_tools: &'a [String],
    ) -> Self {
        let mut bundle = Self::new(
            LoopOrigin::Delegated,
            provider,
            tools_registry,
            observer,
            provider_name,
            model,
            multimodal_config,
            pacing,
            excluded_tools,
            dedup_exempt_tools,
        );
        bundle.silent = true;
        bundle
    }

    #[must_use]
    pub fn channel(
        channel_name: &'a str,
        reply_target: Option<&'a str>,
        provider: &'a dyn Provider,
        tools_registry: &'a [Box<dyn Tool>],
        observer: &'a dyn Observer,
        provider_name: &'a str,
        model: &'a str,
        multimodal_config: &'a crate::config::MultimodalConfig,
        pacing: &'a PacingConfig,
        excluded_tools: &'a [String],
        dedup_exempt_tools: &'a [String],
    ) -> Self {
        let mut bundle = Self::new(
            LoopOrigin::Channel,
            provider,
            tools_registry,
            observer,
            provider_name,
            model,
            multimodal_config,
            pacing,
            excluded_tools,
            dedup_exempt_tools,
        );
        bundle.channel_name = channel_name;
        bundle.channel_reply_target = reply_target;
        bundle
    }

    pub fn with_temperature(mut self, t: f64) -> Self {
        self.temperature = t;
        self
    }

    pub fn with_silent(mut self, silent: bool) -> Self {
        self.silent = silent;
        self
    }

    pub fn with_approval(mut self, approval: Option<&'a ApprovalManager>) -> Self {
        self.approval = approval;
        self
    }

    pub fn with_max_iterations(mut self, n: usize) -> Self {
        self.max_tool_iterations = n;
        self
    }

    pub fn with_cancellation(mut self, ct: Option<CancellationToken>) -> Self {
        self.cancellation_token = ct;
        self
    }

    pub fn with_on_delta(
        mut self,
        sender: Option<tokio::sync::mpsc::Sender<DraftEvent>>,
    ) -> Self {
        self.on_delta = sender;
        self
    }

    pub fn with_event_sink(mut self, sink: EventSink) -> Self {
        if let Some(draft) = sink.draft_sender() {
            self.on_delta = Some(draft);
            self.event_sink = sink;
        } else if let Some(turn) = sink.turn_sender() {
            let (draft_tx, draft_rx) =
                tokio::sync::mpsc::channel::<DraftEvent>(256);
            let draft_rx = Arc::new(tokio::sync::Mutex::new(draft_rx));
            let forward_turn = turn.clone();
            let _ = crate::runtime::spawn_supervised_restartable(
                "agent.policy.event_sink_bridge",
                3,
                move || {
                    let draft_rx = Arc::clone(&draft_rx);
                    let forward_turn = forward_turn.clone();
                    async move {
                    let mut draft_rx = draft_rx.lock().await;
                    use crate::agent::TurnEvent;
                    use std::collections::VecDeque;

                    const MAX_BRIDGE_QUEUE_EVENTS: usize = 1024;

                    fn compact_text_events(queue: &mut VecDeque<TurnEvent>) {
                        let mut compacted: VecDeque<TurnEvent> =
                            VecDeque::with_capacity(queue.len());
                        for event in queue.drain(..) {
                            match event {
                                TurnEvent::Chunk { delta } => {
                                    if let Some(TurnEvent::Chunk { delta: tail }) =
                                        compacted.back_mut()
                                    {
                                        tail.push_str(&delta);
                                    } else {
                                        compacted.push_back(TurnEvent::Chunk { delta });
                                    }
                                }
                                TurnEvent::Thinking { delta } => {
                                    if let Some(TurnEvent::Thinking { delta: tail }) =
                                        compacted.back_mut()
                                    {
                                        tail.push_str(&delta);
                                    } else {
                                        compacted.push_back(TurnEvent::Thinking { delta });
                                    }
                                }
                                other => compacted.push_back(other),
                            }
                        }
                        *queue = compacted;
                    }

                    fn droppable_under_backpressure(event: &TurnEvent) -> bool {
                        matches!(
                            event,
                            TurnEvent::Chunk { .. }
                                | TurnEvent::Thinking { .. }
                                | TurnEvent::StatusUpdate { .. }
                                | TurnEvent::ProgressTick { .. }
                                | TurnEvent::SubagentChunk { .. }
                                | TurnEvent::WorkerProgress { .. }
                                | TurnEvent::CommandPreview { .. }
                        )
                    }

                    fn enqueue(
                        queue: &mut VecDeque<TurnEvent>,
                        event: TurnEvent,
                        dropped_total: &mut u64,
                    ) {
                        match event {
                            TurnEvent::Chunk { delta } => {
                                if let Some(TurnEvent::Chunk { delta: tail }) = queue.back_mut() {
                                    tail.push_str(&delta);
                                } else {
                                    queue.push_back(TurnEvent::Chunk { delta });
                                }
                            }
                            TurnEvent::Thinking { delta } => {
                                if let Some(TurnEvent::Thinking { delta: tail }) =
                                    queue.back_mut()
                                {
                                    tail.push_str(&delta);
                                } else {
                                    queue.push_back(TurnEvent::Thinking { delta });
                                }
                            }
                            other => queue.push_back(other),
                        }
                        if queue.len() > MAX_BRIDGE_QUEUE_EVENTS {
                            compact_text_events(queue);
                            if queue.len() > MAX_BRIDGE_QUEUE_EVENTS {
                                let overflow = queue.len() - MAX_BRIDGE_QUEUE_EVENTS;
                                let mut dropped = 0usize;
                                let mut idx = 0usize;
                                while dropped < overflow && idx < queue.len() {
                                    if droppable_under_backpressure(&queue[idx]) {
                                        queue.remove(idx);
                                        dropped += 1;
                                    } else {
                                        idx += 1;
                                    }
                                }
                                if dropped > 0 {
                                    *dropped_total += dropped as u64;
                                    tracing::warn!(
                                        target: "agent.event_bridge",
                                        dropped,
                                        dropped_total = *dropped_total,
                                        cap = MAX_BRIDGE_QUEUE_EVENTS,
                                        "turn event bridge queue exceeded cap after text compaction; dropped oldest droppable events (consumer too slow)"
                                    );
                                }
                                if queue.len() > MAX_BRIDGE_QUEUE_EVENTS {
                                    tracing::warn!(
                                        target: "agent.event_bridge",
                                        len = queue.len(),
                                        cap = MAX_BRIDGE_QUEUE_EVENTS,
                                        "turn event bridge queue over cap with only critical events; retaining all to avoid losing permission/tool events"
                                    );
                                }
                            }
                        }
                    }

                    let mut queue: VecDeque<TurnEvent> = VecDeque::new();
                    let mut closed = false;
                    let mut dropped_total: u64 = 0;
                    loop {
                        if queue.is_empty() {
                            if closed {
                                break;
                            }
                            match draft_rx.recv().await {
                                Some(ev) => {
                                    if let Some(t) = crate::agent::event_sink::draft_to_turn(ev) {
                                        enqueue(&mut queue, t, &mut dropped_total);
                                    }
                                }
                                None => break,
                            }
                            while let Ok(ev) = draft_rx.try_recv() {
                                if let Some(t) = crate::agent::event_sink::draft_to_turn(ev) {
                                    enqueue(&mut queue, t, &mut dropped_total);
                                }
                            }
                        } else {
                            tokio::select! {
                                maybe_ev = draft_rx.recv(), if !closed => {
                                    match maybe_ev {
                                        Some(ev) => {
                                            if let Some(t) =
                                                crate::agent::event_sink::draft_to_turn(ev)
                                            {
                                                enqueue(&mut queue, t, &mut dropped_total);
                                            }
                                            while let Ok(ev) = draft_rx.try_recv() {
                                                if let Some(t) =
                                                    crate::agent::event_sink::draft_to_turn(ev)
                                                {
                                                    enqueue(&mut queue, t, &mut dropped_total);
                                                }
                                            }
                                        }
                                        None => closed = true,
                                    }
                                }
                                permit = forward_turn.reserve() => {
                                    match permit {
                                        Ok(permit) => {
                                            if let Some(front) = queue.pop_front() {
                                                permit.send(front);
                                            }
                                        }
                                        Err(_) => break,
                                    }
                                }
                            }
                        }
                    }
                    }
                },
            );
            self.on_delta = Some(draft_tx);
            self.event_sink = sink;
        } else {
            self.event_sink = sink;
        }
        self
    }

    pub fn with_hooks(mut self, hooks: Option<&'a HookRunner>) -> Self {
        self.hooks = hooks;
        self
    }

    pub fn with_activated_tools(
        mut self,
        activated: Option<&'a Arc<Mutex<ActivatedToolSet>>>,
    ) -> Self {
        self.activated_tools = activated;
        self
    }

    pub fn with_model_switch_callback(mut self, cb: Option<ModelSwitchCallback>) -> Self {
        self.model_switch_callback = cb;
        self
    }

    pub fn with_rbac(
        mut self,
        engine: Option<&'a Arc<RbacEngine>>,
        identity: Option<&'a CallerIdentity>,
    ) -> Self {
        self.rbac_engine = engine;
        self.rbac_identity = identity;
        self
    }

    pub fn with_plan_mode_flag(mut self, flag: Option<&'a PlanModeFlag>) -> Self {
        self.plan_mode_flag = flag;
        self
    }

    pub fn with_plan_execution_path(mut self, path: Option<&'a str>) -> Self {
        self.plan_execution_path = path;
        self
    }

    pub fn with_tool_registry(mut self, registry: Option<&'a ToolRegistry>) -> Self {
        self.tool_registry = registry;
        self
    }

    pub fn with_channel_name(mut self, name: &'a str) -> Self {
        self.channel_name = name;
        self
    }

    pub fn with_channel_reply_target(mut self, target: Option<&'a str>) -> Self {
        self.channel_reply_target = target;
        self
    }

    pub fn with_response_cache_hook(mut self, hook: Option<Arc<dyn ResponseCacheHook>>) -> Self {
        self.response_cache_hook = hook;
        self
    }

    pub fn with_memory_session_hook(mut self, hook: Option<Arc<dyn MemorySessionHook>>) -> Self {
        self.memory_session_hook = hook;
        self
    }

    pub fn with_turn_preamble_hook(mut self, hook: Option<Arc<dyn TurnPreambleHook>>) -> Self {
        self.turn_preamble_hook = hook;
        self
    }

    pub fn with_gui_model_switch_hook(
        mut self,
        hook: Option<Arc<dyn GuiModelSwitchHook>>,
    ) -> Self {
        self.gui_model_switch_hook = hook;
        self
    }

    pub fn with_iteration_context_budget_hook(
        mut self,
        hook: Option<Arc<dyn IterationContextBudgetHook>>,
    ) -> Self {
        self.iteration_context_budget_hook = hook;
        self
    }

    pub fn with_experience_recorder_hook(
        mut self,
        hook: Option<Arc<dyn ExperienceRecorderHook>>,
    ) -> Self {
        self.experience_recorder_hook = hook;
        self
    }

    pub fn with_plan_mode_nudge_hook(mut self, hook: Option<Arc<dyn PlanModeNudgeHook>>) -> Self {
        self.plan_mode_nudge_hook = hook;
        self
    }

    pub fn with_tool_descriptions(mut self, td: Option<&'a ToolDescriptions>) -> Self {
        self.tool_descriptions = td;
        self
    }
}
