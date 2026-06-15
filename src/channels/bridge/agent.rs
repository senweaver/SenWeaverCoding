// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub use crate::agent::TurnEvent;
pub use crate::agent::loop_::core::AgentLoopCore;
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
    agent: tokio::sync::Mutex<crate::agent::Agent>,
}

impl ChannelAgentBridgeImpl {

    pub fn new(agent: crate::agent::Agent) -> Self {
        Self {
            agent: tokio::sync::Mutex::new(agent),
        }
    }

    pub async fn from_config(config: &crate::config::Config) -> anyhow::Result<Self> {
        let agent = crate::agent::Agent::from_config(config, None, None).await?;
        Ok(Self::new(agent))
    }
}

impl ChannelAgentBridge for ChannelAgentBridgeImpl {
    async fn run_turn(
        &self,
        messages: &mut Vec<ChatMessage>,
        event_tx: Option<tokio::sync::mpsc::Sender<TurnEvent>>,
    ) -> anyhow::Result<String> {
        let Some(user_idx) = messages.iter().rposition(|m| m.role == "user") else {
            anyhow::bail!("run_turn requires at least one user message in the transcript");
        };
        let user_message = messages[user_idx].content.clone();
        let mut agent = self.agent.lock().await;
        agent.clear_history();
        if user_idx > 0 {
            agent.seed_history(&messages[..user_idx]);
        }
        let reply = match event_tx {
            Some(tx) => agent.turn_streamed(&user_message, tx).await?,
            None => agent.turn(&user_message).await?,
        };
        messages.push(ChatMessage::assistant(reply.clone()));
        Ok(reply)
    }

    async fn run_streamed(
        &self,
        messages: &mut Vec<ChatMessage>,
        event_tx: tokio::sync::mpsc::Sender<TurnEvent>,
    ) -> anyhow::Result<String> {
        self.run_turn(messages, Some(event_tx)).await
    }
}
