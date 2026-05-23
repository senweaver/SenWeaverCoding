// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::agent::loop_policy::{LoopOrigin, PolicyBundle};
use crate::agent::loop_unified::UnifiedLoop;
use crate::config::PacingConfig;
use crate::observability::traits::Observer;
use crate::providers::traits::{ChatMessage, Provider};
use crate::security::rbac::{CallerIdentity, RbacEngine};
use crate::tools::traits::Tool;

pub struct AgentLoopCore<'a> {
    provider: &'a dyn Provider,
    tools_registry: &'a [Box<dyn Tool>],
    observer: &'a dyn Observer,

    provider_name: &'a str,
    model: &'a str,
    temperature: f64,
    silent: bool,

    channel_name: &'a str,
    channel_reply_target: Option<&'a str>,

    multimodal_config: &'a crate::config::MultimodalConfig,
    max_tool_iterations: usize,
    cancellation_token: Option<CancellationToken>,

    excluded_tools: &'a [String],
    dedup_exempt_tools: &'a [String],

    pacing: &'a PacingConfig,
    rbac_engine: Option<&'a Arc<RbacEngine>>,
    rbac_identity: Option<&'a CallerIdentity>,

    agent_id: Option<String>,
}

impl<'a> AgentLoopCore<'a> {
    #[must_use]
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

    fn to_policy(
        &self,
        on_delta: Option<tokio::sync::mpsc::Sender<crate::agent::loop_::DraftEvent>>,
    ) -> PolicyBundle<'a> {
        let origin = match self.channel_name {
            "cli" => LoopOrigin::Cli,
            "delegate" => LoopOrigin::Delegated,
            "gui" => LoopOrigin::Gui,
            _ => LoopOrigin::Channel,
        };
        PolicyBundle::new(
            origin,
            self.provider,
            self.tools_registry,
            self.observer,
            self.provider_name,
            self.model,
            self.multimodal_config,
            self.pacing,
            self.excluded_tools,
            self.dedup_exempt_tools,
        )
        .with_temperature(self.temperature)
        .with_silent(self.silent)
        .with_channel_name(self.channel_name)
        .with_channel_reply_target(self.channel_reply_target)
        .with_max_iterations(self.max_tool_iterations)
        .with_cancellation(self.cancellation_token.clone())
        .with_on_delta(on_delta)
        .with_rbac(self.rbac_engine, self.rbac_identity)
    }

    pub async fn run_turn(
        &self,
        history: &mut Vec<ChatMessage>,
        on_delta: Option<tokio::sync::mpsc::Sender<crate::agent::loop_::DraftEvent>>,
    ) -> anyhow::Result<String> {
        if let Some(ref id) = self.agent_id {
            tracing::debug!(target: "agent.loop_core", agent_id = %id, "dispatching via UnifiedLoop");
        }
        UnifiedLoop::new(self.to_policy(on_delta))
            .run(history)
            .await
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
                    let Some(turn_event) = crate::agent::event_sink::draft_to_turn(event) else {
                        continue;
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

        let result = self.run_turn(history, Some(delta_tx)).await;

        bridge_handle.abort();
        result
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
