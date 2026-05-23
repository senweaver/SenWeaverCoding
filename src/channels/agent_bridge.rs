// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub use crate::agent::TurnEvent;
pub use crate::agent::loop_core::AgentLoopCore;
pub use crate::providers::ChatMessage;

#[allow(async_fn_in_trait)]
pub trait ChannelAgentBridge: Send + Sync {

    async fn run_turn(
        &self,
        messages: &mut Vec<ChatMessage>,
        event_tx: Option<tokio::sync::mpsc::Sender<TurnEvent>>,
    ) -> anyhow::Result<String>;

    async fn run_streamed(
        &self,
        messages: &mut Vec<ChatMessage>,
        event_tx: tokio::sync::mpsc::Sender<TurnEvent>,
    ) -> anyhow::Result<String>;
}

pub struct ChannelAgentBridgeImpl {
    _priv: (),
}

impl ChannelAgentBridgeImpl {

    pub fn new() -> Self {
        Self { _priv: () }
    }
}

impl Default for ChannelAgentBridgeImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl ChannelAgentBridge for ChannelAgentBridgeImpl {
    async fn run_turn(
        &self,
        _messages: &mut Vec<ChatMessage>,
        _event_tx: Option<tokio::sync::mpsc::Sender<TurnEvent>>,
    ) -> anyhow::Result<String> {
        anyhow::bail!(
            "ChannelAgentBridgeImpl::run_turn not yet implemented; use AgentLoopCore directly"
        )
    }

    async fn run_streamed(
        &self,
        _messages: &mut Vec<ChatMessage>,
        _event_tx: tokio::sync::mpsc::Sender<TurnEvent>,
    ) -> anyhow::Result<String> {
        anyhow::bail!(
            "ChannelAgentBridgeImpl::run_streamed not yet implemented; use AgentLoopCore directly"
        )
    }
}
