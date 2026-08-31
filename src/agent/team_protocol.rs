// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::time::Instant;

use serde_json::Value;
use tokio::sync::broadcast;
use tracing::info;

use crate::error::{CoordinatorError, SenError};

pub type TeamId = String;
pub type AgentId = String;
pub type MessageId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Orchestrator,
    Specialist,
    Reviewer,
    Mediator,
}

impl Default for Role {
    fn default() -> Self {
        Role::Specialist
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::Orchestrator => write!(f, "Orchestrator"),
            Role::Specialist => write!(f, "Specialist"),
            Role::Reviewer => write!(f, "Reviewer"),
            Role::Mediator => write!(f, "Mediator"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelType {

    Broadcast,

    Direct,

    Group,
}

#[derive(Debug, Clone)]
pub struct TeamMessage {
    pub id: MessageId,
    pub sender: AgentId,
    pub channel: ChannelType,
    pub recipients: Vec<AgentId>,
    pub payload: MessagePayload,
    pub timestamp: Instant,
}

impl TeamMessage {
    pub fn broadcast(id: MessageId, sender: AgentId, payload: MessagePayload) -> Self {
        Self {
            id,
            sender,
            channel: ChannelType::Broadcast,
            recipients: Vec::new(),
            payload,
            timestamp: Instant::now(),
        }
    }

    pub fn direct(id: MessageId, sender: AgentId, to: AgentId, payload: MessagePayload) -> Self {
        Self {
            id,
            sender,
            channel: ChannelType::Direct,
            recipients: vec![to],
            payload,
            timestamp: Instant::now(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum MessagePayload {

    Text(String),

    Data(Value),
}

#[derive(Debug)]
pub struct Team {
    pub id: TeamId,
    pub name: String,
    pub members: HashMap<AgentId, Role>,
    pub message_tx: broadcast::Sender<TeamMessage>,
    pub config: TeamConfig,
}

impl Team {
    pub fn new(id: TeamId, name: String, config: TeamConfig) -> Self {
        let (message_tx, _) = broadcast::channel(config.message_channel_size);
        Self {
            id,
            name,
            members: HashMap::new(),
            message_tx,
            config,
        }
    }

    pub fn add_member(&mut self, agent_id: AgentId, role: Role) -> Result<(), SenError> {
        if !self.members.contains_key(&agent_id)
            && self.members.len() >= self.config.max_team_size.max(1)
        {
            return Err(SenError::Coordinator(CoordinatorError::BarrierTimeout(
                format!(
                    "team '{}' is full ({} members, max_team_size={})",
                    self.id,
                    self.members.len(),
                    self.config.max_team_size
                ),
            )));
        }
        self.members.insert(agent_id.clone(), role);
        info!("Team {}: {} joined as {}", self.id, agent_id, role);
        Ok(())
    }

    pub fn remove_member(&mut self, agent_id: &AgentId) -> Option<Role> {
        let role = self.members.remove(agent_id);
        info!("Team {}: {} left the team", self.id, agent_id);
        role
    }

    pub fn get_role(&self, agent_id: &AgentId) -> Option<Role> {
        self.members.get(agent_id).copied()
    }

    pub fn agents_with_role(&self, role: Role) -> Vec<AgentId> {
        self.members
            .iter()
            .filter(|(_, r)| **r == role)
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn broadcast(
        &self,
        message_id: MessageId,
        sender: &AgentId,
        payload: MessagePayload,
    ) -> Result<(), SenError> {
        let msg = TeamMessage::broadcast(message_id, sender.clone(), payload);
        self.message_tx.send(msg).map_err(|_| {
            SenError::Coordinator(CoordinatorError::BarrierTimeout(
                "broadcast channel closed".into(),
            ))
        })?;
        Ok(())
    }

    pub fn send_direct(
        &self,
        message_id: MessageId,
        sender: &AgentId,
        to: &AgentId,
        payload: MessagePayload,
    ) -> Result<(), SenError> {
        let msg = TeamMessage::direct(message_id, sender.clone(), to.clone(), payload);
        self.message_tx.send(msg).map_err(|_| {
            SenError::Coordinator(CoordinatorError::BarrierTimeout(
                "broadcast channel closed".into(),
            ))
        })?;
        Ok(())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TeamMessage> {
        self.message_tx.subscribe()
    }
}

#[derive(Debug, Clone)]
pub struct TeamConfig {
    pub message_channel_size: usize,
    pub max_team_size: usize,
}

impl Default for TeamConfig {
    fn default() -> Self {
        Self {
            message_channel_size: 256,
            max_team_size: 20,
        }
    }
}
