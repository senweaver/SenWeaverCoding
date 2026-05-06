// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//!
//! Team collaboration protocol for multi-agent coordination.
//!
//! ## Overview
//!
//! A `Team` is a group of agents working toward shared goals under a
//! defined role structure.  The team protocol provides:
//!
//! - **Role assignment**: Orchestrator, Specialist, Reviewer, Mediator
//! - **Communication channels**: Broadcast, Direct, Group, Hierarchical
//! - **Goal tracking**: decomposition, progress, alerts
//!
//! ## Roles
//!
//! | Role         | Responsibilities                          |
//! |--------------|------------------------------------------|
//! | Orchestrator | Task decomposition and assignment         |
//! | Specialist   | Focused execution in a domain             |
//! | Reviewer     | Validates other agents' outputs          |
//! | Mediator     | Resolves conflicts between agents         |
//!
//! ## Usage
//!
//! ```ignore
//! let team = Team::new("coding-team", TeamConfig::default());
//! team.add_member("orchestrator".into(), Role::Orchestrator);
//! team.add_goal(Goal::new("g1".into(), "implement auth".into()))?;
//! team.broadcast(&"orchestrator".into(), MessagePayload::GoalAnnounced { goal_id: "g1".into() })?;
//! ```

use std::collections::HashMap;
use std::time::Instant;

use serde_json::Value;
use tokio::sync::broadcast;
use tracing::info;

use crate::error::{CoordinatorError, SenError};

pub type TeamId = String;
pub type AgentId = String;
pub type GoalId = String;
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

    Hierarchical,
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
    pub fn broadcast(sender: AgentId, payload: MessagePayload) -> Self {
        Self {
            id: 0,
            sender,
            channel: ChannelType::Broadcast,
            recipients: Vec::new(),
            payload,
            timestamp: Instant::now(),
        }
    }

    pub fn direct(sender: AgentId, to: AgentId, payload: MessagePayload) -> Self {
        Self {
            id: 0,
            sender,
            channel: ChannelType::Direct,
            recipients: vec![to],
            payload,
            timestamp: Instant::now(),
        }
    }

    pub fn hierarchical(sender: AgentId, payload: MessagePayload) -> Self {
        Self {
            id: 0,
            sender,
            channel: ChannelType::Hierarchical,
            recipients: Vec::new(),
            payload,
            timestamp: Instant::now(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum MessagePayload {

    GoalAnnounced { goal_id: GoalId },

    GoalCompleted { goal_id: GoalId, output: Value },

    TaskAssigned {
        goal_id: GoalId,
        task: String,
        assignee: AgentId,
    },

    DecisionRequested {
        context: String,
        options: Vec<String>,
    },

    DecisionReached { consensus: String },

    ConflictReported { details: String },

    ConflictResolved { resolution: String },

    ProgressUpdate { goal_id: GoalId, percent: u8 },

    Text(String),

    Data(Value),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GoalPriority {
    Critical = 4,
    High = 3,
    Medium = 2,
    Low = 1,
}

impl Default for GoalPriority {
    fn default() -> Self {
        GoalPriority::Medium
    }
}

#[derive(Debug, Clone)]
pub struct Goal {
    pub id: GoalId,
    pub description: String,
    pub priority: GoalPriority,
    pub status: GoalStatus,
    pub sub_goals: Vec<GoalId>,
    pub assignees: Vec<AgentId>,
    pub created_at: Instant,
    pub updated_at: Instant,
}

impl Goal {
    pub fn new(id: GoalId, description: String) -> Self {
        Self {
            id,
            description,
            priority: GoalPriority::default(),
            status: GoalStatus::Pending,
            sub_goals: Vec::new(),
            assignees: Vec::new(),
            created_at: Instant::now(),
            updated_at: Instant::now(),
        }
    }

    pub fn with_priority(mut self, priority: GoalPriority) -> Self {
        self.priority = priority;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalStatus {
    Pending,
    InProgress,
    Blocked,
    Completed,
    Cancelled,
}

#[derive(Debug)]
pub struct Team {
    pub id: TeamId,
    pub name: String,
    pub members: HashMap<AgentId, Role>,
    pub goals: HashMap<GoalId, Goal>,
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
            goals: HashMap::new(),
            message_tx,
            config,
        }
    }

    pub fn add_member(&mut self, agent_id: AgentId, role: Role) {
        self.members.insert(agent_id.clone(), role);
        info!("Team {}: {} joined as {}", self.id, agent_id, role);
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

    pub fn add_goal(&mut self, goal: Goal) -> Result<(), SenError> {
        if self.goals.contains_key(&goal.id) {
            return Err(SenError::Coordinator(CoordinatorError::AgentNotFound(
                format!("goal '{}' already exists", goal.id),
            )));
        }
        info!(
            "Team {}: goal '{}' added (priority: {:?})",
            self.id, goal.id, goal.priority
        );
        self.goals.insert(goal.id.clone(), goal);
        Ok(())
    }

    pub fn update_goal_status(
        &mut self,
        goal_id: &GoalId,
        status: GoalStatus,
    ) -> Result<(), SenError> {
        let goal = self.goals.get_mut(goal_id).ok_or_else(|| {
            SenError::Coordinator(CoordinatorError::AgentNotFound(format!(
                "goal '{}' not found",
                goal_id
            )))
        })?;
        goal.status = status;
        goal.updated_at = Instant::now();
        Ok(())
    }

    pub fn assign_goal(&mut self, goal_id: &GoalId, agent_id: &AgentId) -> Result<(), SenError> {
        if !self.members.contains_key(agent_id) {
            return Err(SenError::Coordinator(CoordinatorError::AgentNotFound(
                format!("agent '{}' not in team", agent_id),
            )));
        }
        let goal = self.goals.get_mut(goal_id).ok_or_else(|| {
            SenError::Coordinator(CoordinatorError::AgentNotFound(format!(
                "goal '{}' not found",
                goal_id
            )))
        })?;
        if !goal.assignees.contains(agent_id) {
            goal.assignees.push(agent_id.clone());
        }
        if goal.status == GoalStatus::Pending {
            goal.status = GoalStatus::InProgress;
        }
        goal.updated_at = Instant::now();
        Ok(())
    }

    pub fn active_goals(&self) -> Vec<&Goal> {
        self.goals
            .values()
            .filter(|g| {
                matches!(
                    g.status,
                    GoalStatus::Pending | GoalStatus::InProgress | GoalStatus::Blocked
                )
            })
            .collect()
    }

    pub fn broadcast(&self, sender: &AgentId, payload: MessagePayload) -> Result<(), SenError> {
        let msg = TeamMessage::broadcast(sender.clone(), payload);
        self.message_tx.send(msg).map_err(|_| {
            SenError::Coordinator(CoordinatorError::BarrierTimeout(
                "broadcast channel closed".into(),
            ))
        })?;
        Ok(())
    }

    pub fn send_direct(
        &self,
        sender: &AgentId,
        to: &AgentId,
        payload: MessagePayload,
    ) -> Result<(), SenError> {
        let msg = TeamMessage::direct(sender.clone(), to.clone(), payload);
        self.message_tx.send(msg).map_err(|_| {
            SenError::Coordinator(CoordinatorError::BarrierTimeout(
                "broadcast channel closed".into(),
            ))
        })?;
        Ok(())
    }

    pub fn send_hierarchical(
        &self,
        sender: &AgentId,
        payload: MessagePayload,
    ) -> Result<(), SenError> {
        let msg = TeamMessage::hierarchical(sender.clone(), payload);
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
    pub goal_timeout_secs: Option<u64>,
}

impl Default for TeamConfig {
    fn default() -> Self {
        Self {
            message_channel_size: 256,
            max_team_size: 20,
            goal_timeout_secs: Some(3600),
        }
    }
}
