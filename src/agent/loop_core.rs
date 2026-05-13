// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! `AgentLoopCore` — builder-shaped façade over the **stateless tool loop**
//! `run_tool_call_loop`.
//!
//! ## Architecture: two layers, not one
//!
//! After auditing what each path actually does (Apr 2026 review) the
//! project deliberately keeps **two execution layers** that serve
//! different purposes — they are NOT redundant implementations of the
//! same loop:
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │ Upper layer (stateful, user-facing): Agent::turn_streamed    │
//! │  - Memory loader / auto_save                                  │
//! │  - Response cache lookup + write-back                          │
//! │  - apply_turn_preamble (mode filter, hot-reload, sys prompt)   │
//! │  - apply_gui_model_switch (mid-turn model swap from GUI)      │
//! │  - classify_model (per-turn complexity-based model routing)   │
//! │  - finish_turn_experience (experience replay records)         │
//! │  - prepare_iteration_context_budget                            │
//! └──────────────────────────────────────────────────────────────┘
//!                            │
//!                            ▼ (calls provider.stream_chat / execute_tools)
//! ┌──────────────────────────────────────────────────────────────┐
//! │ Lower layer (stateless executor): run_tool_call_loop         │
//! │  - 25 explicit parameters, no implicit state                  │
//! │  - self_consistency resampling + majority vote                │
//! │  - DraftEvent rich event stream                               │
//! │  - tokio_util::CancellationToken multi-level cancel           │
//! │  - Channel routing (channel_name, channel_reply_target)       │
//! │  - RBAC engine integration                                    │
//! └──────────────────────────────────────────────────────────────┘
//! ```
//!
//! Each frontend picks the layer that matches its needs:
//!
//! | Frontend | Loop body | Why |
//! |----------|-----------|-----|
//! | CLI / `channels::mod`              | `run_tool_call_loop` directly | batch / no UI session state |
//! | Subagents (`tools::delegate*`)     | `run_tool_call_loop` directly | scoped child tool execution |
//! | `Agent::turn_via_loop_core` (SDK)  | `AgentLoopCore::run_turn` → `run_tool_call_loop` | thin SDK adapter |
//! | TUI (`session::mod`)               | `Agent::turn_streamed`  | needs cache / memory / model switch |
//! | GUI (`gui::bridge`)                | `Agent::turn_streamed`  | needs cache / memory / hot-reload |
//! | Gateway WS / RPC / ACP             | `Agent::turn_streamed`  | needs full agent state |
//!
//! ## Why not "make everyone go through `run_tool_call_loop`"?
//!
//! It looks tempting from a "single source of truth" angle, but the
//! lower-layer loop intentionally does NOT carry the upper-layer
//! features.  A blanket migration would silently delete:
//!
//! * Response cache → instantly regresses token-saving work.
//! * Memory persistence → cross-session continuity broken.
//! * GUI mid-turn model switching.
//! * `classify_model` automatic complexity-based routing.
//! * Config hot-reload.
//! * `mode_tool_filter` (Plan / Ask allowlists).
//! * `finish_turn_experience` records (kills experience replay /
//!   evaluation pipelines).
//!
//! These features are stateful, agent-scoped, and live deliberately in
//! `Agent::turn_streamed`.  They cannot be "lifted" into
//! `run_tool_call_loop` without turning that function into a 50-parameter
//! stateful API — which defeats its purpose.
//!
//! ## How parity is kept (the actually-correct pattern)
//!
//! Cross-cutting policy that MUST be identical between the two layers
//! lives in **shared modules**, and BOTH paths call those modules.  Any
//! new such policy must be added the same way; never duplicate logic
//! across `run_tool_call_loop` and `Agent::turn_streamed`.
//!
//! Currently shared:
//!
//! * Loop detection — [`crate::agent::loop_control::LoopControlState`].
//! * Mode hooks (auto-verify, post-tool-batch) — [`crate::agent::mode_effects`].
//! * Plan-mode enforcement — [`crate::agent::plan_mode_enforcement`].
//! * Turn-metrics RAII — [`crate::agent::executor_core::TurnMetricsGuard`].
//! * Pacing — [`crate::agent::executor_core::PacingGovernor`].
//!
//! ## What this module provides
//!
//! `AgentLoopCore` exists for the *third* call site —
//! `Agent::turn_via_loop_core` and any future SDK / batch caller that
//! wants a clean builder API into the lower layer without the 25
//! positional arguments.  It is intentionally a thin façade and never
//! grows upper-layer responsibilities.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::config::PacingConfig;
use crate::observability::traits::Observer;
use crate::providers::traits::{ChatMessage, Provider};
use crate::security::rbac::{CallerIdentity, RbacEngine};
use crate::tools::traits::Tool;

pub struct AgentLoopCore<'a> {
    pub provider: &'a dyn Provider,
    pub tools_registry: &'a [Box<dyn Tool>],
    pub observer: &'a dyn Observer,

    pub provider_name: &'a str,
    pub model: &'a str,
    pub temperature: f64,
    pub silent: bool,

    pub channel_name: &'a str,
    pub channel_reply_target: Option<&'a str>,

    pub multimodal_config: &'a crate::config::MultimodalConfig,
    pub max_tool_iterations: usize,
    pub cancellation_token: Option<CancellationToken>,

    pub excluded_tools: &'a [String],
    pub dedup_exempt_tools: &'a [String],

    pub pacing: &'a PacingConfig,
    pub rbac_engine: Option<&'a Arc<RbacEngine>>,
    pub rbac_identity: Option<&'a CallerIdentity>,

    pub agent_id: Option<String>,
}

impl<'a> AgentLoopCore<'a> {

    pub fn bare(
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
            provider,
            tools_registry,
            observer,
            provider_name,
            model,
            temperature: 0.7,
            silent: false,
            channel_name: "cli",
            channel_reply_target: None,
            multimodal_config,

            max_tool_iterations: crate::config::default_agent_max_tool_iterations(),
            cancellation_token: None,
            excluded_tools,
            dedup_exempt_tools,
            pacing,
            rbac_engine: None,
            rbac_identity: None,
            agent_id: None,
        }
    }

    pub fn channel(mut self, name: &'a str, reply_target: Option<&'a str>) -> Self {
        self.channel_name = name;
        self.channel_reply_target = reply_target;
        self
    }

    pub fn temperature(mut self, t: f64) -> Self {
        self.temperature = t;
        self
    }

    pub fn cancellation(mut self, ct: CancellationToken) -> Self {
        self.cancellation_token = Some(ct);
        self
    }

    pub fn max_iterations(mut self, n: usize) -> Self {
        self.max_tool_iterations = n;
        self
    }

    pub fn silent(mut self, yes: bool) -> Self {
        self.silent = yes;
        self
    }

    pub fn rbac_allow_all(
        mut self,
        engine: &'a Arc<RbacEngine>,
        identity: &'a CallerIdentity,
    ) -> Self {
        self.rbac_engine = Some(engine);
        self.rbac_identity = Some(identity);
        self
    }

    pub fn with_agent_id(mut self, id: impl Into<String>) -> Self {
        self.agent_id = Some(id.into());
        self
    }

    pub async fn run_turn(
        &self,
        history: &mut Vec<ChatMessage>,
        on_delta: Option<tokio::sync::mpsc::Sender<crate::agent::loop_::DraftEvent>>,
    ) -> anyhow::Result<String> {
        self.run_turn_internal(history, on_delta).await
    }

    pub async fn run_streamed(
        &self,
        history: &mut Vec<ChatMessage>,
        event_tx: tokio::sync::mpsc::Sender<crate::agent::TurnEvent>,
    ) -> anyhow::Result<String> {

        let (delta_tx, mut delta_rx) =
            tokio::sync::mpsc::channel::<crate::agent::loop_::DraftEvent>(64);

        let bridge_handle =
            crate::runtime::spawn_supervised("agent.loop_core.draft_bridge", async move {
                while let Some(event) = delta_rx.recv().await {
                    let turn_event = match event {
                        crate::agent::loop_::DraftEvent::Clear => {

                            continue;
                        }
                        crate::agent::loop_::DraftEvent::Progress(text) => {
                            crate::agent::TurnEvent::StatusUpdate {
                                action: "thinking".into(),
                                detail: text,
                            }
                        }
                        crate::agent::loop_::DraftEvent::Content(text) => {
                            crate::agent::TurnEvent::Chunk { delta: text }
                        }
                        crate::agent::loop_::DraftEvent::Thinking(text) => {
                            crate::agent::TurnEvent::Thinking { delta: text }
                        }
                        crate::agent::loop_::DraftEvent::ToolCall { name, args } => {
                            crate::agent::TurnEvent::ToolCall { name, args }
                        }
                        crate::agent::loop_::DraftEvent::ToolResult { name, output, success } => {
                            crate::agent::TurnEvent::ToolResult { name, output, success }
                        }
                        crate::agent::loop_::DraftEvent::FileEdit {
                            path,
                            additions,
                            deletions,
                            diff,
                            edit_batch_id,
                        } => crate::agent::TurnEvent::FileEdit {
                            path,
                            additions,
                            deletions,
                            diff,
                            edit_batch_id,
                        },
                        crate::agent::loop_::DraftEvent::ProgressTick {
                            iteration,
                            max_iterations,
                            tokens_used,
                        } => crate::agent::TurnEvent::ProgressTick {
                            iteration,
                            max_iterations,
                            tokens_used,
                        },
                        crate::agent::loop_::DraftEvent::ContextCompressed {
                            tokens_before,
                            tokens_after,
                        } => crate::agent::TurnEvent::ContextCompressed {
                            tokens_before,
                            tokens_after,
                        },
                        crate::agent::loop_::DraftEvent::Cancelling { reason } => {
                            crate::agent::TurnEvent::Cancelling { reason }
                        }
                        crate::agent::loop_::DraftEvent::Error { message } => {
                            crate::agent::TurnEvent::Error { message }
                        }
                        crate::agent::loop_::DraftEvent::UsageUpdate { .. } => {

                            continue;
                        }
                        crate::agent::loop_::DraftEvent::Subagent {
                            task_id,
                            agent_id,
                            kind,
                            delta,
                        } => crate::agent::TurnEvent::SubagentChunk {
                            task_id,
                            agent_id,
                            kind,
                            delta,
                        },
                        crate::agent::loop_::DraftEvent::PiiSanitized { report } => {
                            crate::agent::TurnEvent::PiiSanitized { report }
                        }
                    };
                    if event_tx.send(turn_event).await.is_err() {
                        tracing::debug!(
                            "AgentLoopCore stream consumer dropped event channel; bridging stopped"
                        );
                        break;
                    }
                }
            })
            .into_inner();

        let result = self.run_turn_internal(history, Some(delta_tx)).await;

        bridge_handle.abort();

        result
    }

    #[allow(deprecated)]
    async fn run_turn_internal(
        &self,
        history: &mut Vec<ChatMessage>,
        on_delta: Option<tokio::sync::mpsc::Sender<crate::agent::loop_::DraftEvent>>,
    ) -> anyhow::Result<String> {
        crate::agent::loop_::run_tool_call_loop(
            self.provider,
            history,
            self.tools_registry,
            self.observer,
            self.provider_name,
            self.model,
            self.temperature,
            self.silent,
            None,
            self.channel_name,
            self.channel_reply_target,
            self.multimodal_config,
            self.max_tool_iterations,
            self.cancellation_token.clone(),
            on_delta,
            None,
            self.excluded_tools,
            self.dedup_exempt_tools,
            None,
            None,
            self.pacing,
            self.rbac_engine,
            self.rbac_identity,
            None,
            None,
        )
        .await
    }

    pub fn dry_check(&self, history: &[ChatMessage]) -> Result<(), String> {
        if self.tools_registry.is_empty() && !history.iter().any(|m| m.role == "system") {
            return Err(
                "bare loop with no tools and no system prompt is almost always misconfigured"
                    .into(),
            );
        }
        if self.temperature < 0.0 || self.temperature > 2.0 {
            return Err(format!(
                "temperature {} out of range [0.0, 2.0]",
                self.temperature
            ));
        }
        Ok(())
    }
}
