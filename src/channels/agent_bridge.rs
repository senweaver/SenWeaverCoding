// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//!
//! Stable bridge between the channels layer and the agent loop.
//!
//! This module provides a clean interface that channels use to invoke the agent,
//! decoupling them from the internal implementation details of `loop_.rs`.
//!
//! ## Design
//!
//! `ChannelAgentBridge` is the trait that all bridges must implement.
//! `ChannelAgentBridgeImpl` is the production implementation that delegates
//! to `AgentLoopCore`.
//!
//! Channels should only import from this module, not from `agent::loop_`.

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
