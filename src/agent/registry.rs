// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::error::RegistryError;

pub type AgentId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {

    Idle,

    Active,

    Suspended,

    ShuttingDown,

    Terminated,

    Failed,

    Restarting,
}

impl std::fmt::Display for AgentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCapability {

    pub name: String,

    pub description: String,

    pub proficiency: f64,
}

impl PartialEq for AgentCapability {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.description == other.description
    }
}

impl Eq for AgentCapability {}

impl std::hash::Hash for AgentCapability {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "AgentInfoRaw")]
pub struct AgentInfo {

    pub id: AgentId,

    pub name: String,

    pub role: String,

    pub state: AgentState,

    pub capabilities: Vec<AgentCapability>,

    pub model: String,

    #[serde(default)]
    pub provider: String,

    pub registered_at: DateTime<Utc>,

    pub last_heartbeat: DateTime<Utc>,

    pub tasks_completed: u64,

    pub tasks_failed: u64,

    pub current_task: Option<String>,

    pub tags: HashSet<String>,

    pub max_concurrency: u32,

    pub current_load: u32,
}

#[derive(Deserialize)]
struct AgentInfoRaw {
    pub id: AgentId,
    pub name: String,
    pub role: String,
    pub state: AgentState,
    #[serde(default)]
    pub capabilities: Vec<AgentCapability>,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub provider: String,
    pub registered_at: DateTime<Utc>,
    pub last_heartbeat: DateTime<Utc>,
    #[serde(default)]
    pub tasks_completed: u64,
    #[serde(default)]
    pub tasks_failed: u64,
    #[serde(default)]
    pub current_task: Option<String>,
    #[serde(default)]
    pub tags: HashSet<String>,
    #[serde(default = "default_max_concurrency")]
    pub max_concurrency: u32,
    #[serde(default)]
    pub current_load: u32,
}

fn default_max_concurrency() -> u32 {
    1
}

impl From<AgentInfoRaw> for AgentInfo {
    fn from(raw: AgentInfoRaw) -> Self {
        let provider = if raw.provider.is_empty() {
            parse_provider_from_model(&raw.model)
        } else {
            raw.provider
        };
        AgentInfo {
            id: raw.id,
            name: raw.name,
            role: raw.role,
            state: raw.state,
            capabilities: raw.capabilities,
            model: raw.model,
            provider,
            registered_at: raw.registered_at,
            last_heartbeat: raw.last_heartbeat,
            tasks_completed: raw.tasks_completed,
            tasks_failed: raw.tasks_failed,
            current_task: raw.current_task,
            tags: raw.tags,
            max_concurrency: raw.max_concurrency,
            current_load: raw.current_load,
        }
    }
}

fn parse_provider_from_model(model: &str) -> String {
    if let Some(idx) = model.find('/') {
        return model[..idx].to_string();
    }
    if let Some(idx) = model.find(':') {
        return model[..idx].to_string();
    }
    String::new()
}

impl AgentInfo {

    pub fn new(id: impl Into<String>, name: impl Into<String>, role: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: id.into(),
            name: name.into(),
            role: role.into(),
            state: AgentState::Idle,
            capabilities: Vec::new(),
            model: String::new(),
            provider: String::new(),
            registered_at: now,
            last_heartbeat: now,
            tasks_completed: 0,
            tasks_failed: 0,
            current_task: None,
            tags: HashSet::new(),
            max_concurrency: 1,
            current_load: 0,
        }
    }

    pub fn is_available(&self) -> bool {
        self.state == AgentState::Idle
            || (self.state == AgentState::Active && self.current_load < self.max_concurrency)
    }

    pub fn has_capability(&self, name: &str) -> bool {
        self.capabilities.iter().any(|c| c.name == name)
    }

    pub fn proficiency_for(&self, capability: &str) -> f64 {
        self.capabilities
            .iter()
            .find(|c| c.name == capability)
            .map(|c| c.proficiency)
            .unwrap_or(0.0)
    }
}

pub struct AgentRegistry {
    agents: RwLock<HashMap<AgentId, AgentInfo>>,

    heartbeat_timeout: Duration,

    availability: tokio::sync::Notify,
}

impl AgentRegistry {

    pub fn new() -> Self {
        Self {
            agents: RwLock::new(HashMap::new()),
            heartbeat_timeout: Duration::from_secs(60),
            availability: tokio::sync::Notify::new(),
        }
    }

    pub fn with_heartbeat_timeout(timeout: Duration) -> Self {
        Self {
            agents: RwLock::new(HashMap::new()),
            heartbeat_timeout: timeout,
            availability: tokio::sync::Notify::new(),
        }
    }

    pub fn availability(&self) -> &tokio::sync::Notify {
        &self.availability
    }

    pub fn register(&self, info: AgentInfo) -> Result<(), RegistryError> {
        let mut agents = self.agents.write();
        if agents.contains_key(&info.id) {
            warn!(agent_id = %info.id, "Agent already registered");
            return Err(RegistryError::AlreadyRegistered(info.id));
        }
        info!(agent_id = %info.id, name = %info.name, "Agent registered");
        agents.insert(info.id.clone(), info);
        drop(agents);
        self.availability.notify_waiters();
        Ok(())
    }

    pub fn register_bounded(
        &self,
        info: AgentInfo,
        max_agents: usize,
        capability_limits: &HashMap<String, usize>,
    ) -> Result<(), RegistryError> {
        let mut agents = self.agents.write();
        if agents.contains_key(&info.id) {
            warn!(agent_id = %info.id, "Agent already registered");
            return Err(RegistryError::AlreadyRegistered(info.id));
        }
        if max_agents > 0 && agents.len() >= max_agents {
            return Err(RegistryError::MaxAgentsLimit(max_agents));
        }
        for cap in &info.capabilities {
            if let Some(&limit) = capability_limits.get(&cap.name) {
                if limit == 0 {
                    continue;
                }
                let current = agents
                    .values()
                    .filter(|a| a.has_capability(&cap.name))
                    .count();
                if current >= limit {
                    return Err(RegistryError::CapabilityLimit(cap.name.clone(), limit));
                }
            }
        }
        info!(agent_id = %info.id, name = %info.name, "Agent registered");
        agents.insert(info.id.clone(), info);
        drop(agents);
        self.availability.notify_waiters();
        Ok(())
    }

    pub fn deregister(&self, agent_id: &str) -> Result<AgentInfo, RegistryError> {
        let mut agents = self.agents.write();
        match agents.remove(agent_id) {
            Some(removed) => {
                info!(agent_id = %agent_id, "Agent deregistered");
                Ok(removed)
            }
            None => Err(RegistryError::AgentNotFound(agent_id.to_string())),
        }
    }

    pub fn set_state(&self, agent_id: &str, state: AgentState) -> Result<(), RegistryError> {
        let mut agents = self.agents.write();
        match agents.get_mut(agent_id) {
            Some(agent) => {
                debug!(agent_id = %agent_id, old = %agent.state, new = %state, "Agent state change");
                agent.state = state;
                agent.last_heartbeat = Utc::now();
                drop(agents);
                self.availability.notify_waiters();
                Ok(())
            }
            None => Err(RegistryError::AgentNotFound(agent_id.to_string())),
        }
    }

    pub fn heartbeat(&self, agent_id: &str) -> Result<(), RegistryError> {
        let mut agents = self.agents.write();
        match agents.get_mut(agent_id) {
            Some(agent) => {
                agent.last_heartbeat = Utc::now();
                Ok(())
            }
            None => Err(RegistryError::AgentNotFound(agent_id.to_string())),
        }
    }

    pub fn assign_task(&self, agent_id: &str, task_id: &str) -> Result<(), RegistryError> {
        let mut agents = self.agents.write();
        match agents.get_mut(agent_id) {
            Some(agent) => {
                if !agent.is_available() {
                    return Err(RegistryError::AgentNotAvailable(
                        agent_id.to_string(),
                        format!("{:?}", agent.state),
                    ));
                }
                agent.current_task = Some(task_id.to_string());
                agent.current_load += 1;
                if agent.state == AgentState::Idle {
                    agent.state = AgentState::Active;
                }
                agent.last_heartbeat = Utc::now();
                Ok(())
            }
            None => Err(RegistryError::AgentNotFound(agent_id.to_string())),
        }
    }

    pub fn complete_task(&self, agent_id: &str, success: bool) {
        let mut agents = self.agents.write();
        if let Some(agent) = agents.get_mut(agent_id) {
            if success {
                agent.tasks_completed += 1;
            } else {
                agent.tasks_failed += 1;
            }
            agent.current_load = agent.current_load.saturating_sub(1);
            if agent.current_load == 0 {
                agent.current_task = None;
                match agent.state {
                    AgentState::Active | AgentState::Restarting => {
                        agent.state = AgentState::Idle;
                    }
                    AgentState::ShuttingDown => {
                        agent.state = AgentState::Terminated;
                    }
                    _ => {}
                }
            }
            agent.last_heartbeat = Utc::now();
        }
        drop(agents);
        self.availability.notify_waiters();
    }

    pub fn reconcile_loads_after_event_lag(&self) {
        let mut agents = self.agents.write();
        for agent in agents.values_mut() {
            match agent.state {
                AgentState::Idle
                | AgentState::Terminated
                | AgentState::Failed
                | AgentState::Suspended
                | AgentState::ShuttingDown
                | AgentState::Restarting => {
                    agent.current_load = 0;
                    agent.current_task = None;
                }
                AgentState::Active => {
                    if agent.current_task.is_none() {
                        agent.current_load = 0;
                        agent.state = AgentState::Idle;
                    } else {
                        agent.current_load =
                            agent.current_load.clamp(1, agent.max_concurrency.max(1));
                    }
                }
            }
            agent.last_heartbeat = Utc::now();
        }
    }

    pub fn get(&self, agent_id: &str) -> Option<AgentInfo> {
        self.agents.read().get(agent_id).cloned()
    }

    pub fn all(&self) -> Vec<AgentInfo> {
        self.agents.read().values().cloned().collect()
    }

    pub fn by_state(&self, state: AgentState) -> Vec<AgentInfo> {
        self.agents
            .read()
            .values()
            .filter(|a| a.state == state)
            .cloned()
            .collect()
    }

    pub fn find_by_capability(&self, capability: &str) -> Vec<AgentInfo> {
        let mut matches: Vec<AgentInfo> = self
            .agents
            .read()
            .values()
            .filter(|a| a.has_capability(capability))
            .cloned()
            .collect();
        matches.sort_by(|a, b| {
            b.proficiency_for(capability)
                .partial_cmp(&a.proficiency_for(capability))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        matches
    }

    pub fn find_best_available(&self, capability: &str) -> Option<AgentInfo> {
        let agents = self.agents.read();
        agents
            .values()
            .filter(|a| a.is_available() && a.has_capability(capability))
            .max_by(|a, b| {
                let score_a = a.proficiency_for(capability)
                    * (1.0 - f64::from(a.current_load) / f64::from(a.max_concurrency.max(1)));
                let score_b = b.proficiency_for(capability)
                    * (1.0 - f64::from(b.current_load) / f64::from(b.max_concurrency.max(1)));
                score_a
                    .partial_cmp(&score_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned()
    }

    pub fn find_by_tag(&self, tag: &str) -> Vec<AgentInfo> {
        self.agents
            .read()
            .values()
            .filter(|a| a.tags.contains(tag))
            .cloned()
            .collect()
    }

    pub fn active_count(&self) -> usize {
        self.agents
            .read()
            .values()
            .filter(|a| a.state == AgentState::Active)
            .count()
    }

    pub fn count(&self) -> usize {
        self.agents.read().len()
    }

    pub fn check_stale(&self) -> Vec<AgentId> {
        let now = Utc::now();
        let timeout_secs = self.heartbeat_timeout.as_secs() as i64;
        self.agents
            .read()
            .iter()
            .filter(|(_, a)| {
                !matches!(
                    a.state,
                    AgentState::Terminated | AgentState::Idle | AgentState::ShuttingDown
                ) && (now - a.last_heartbeat).num_seconds() > timeout_secs
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn reap_stale(&self) -> usize {
        let stale = self.check_stale();
        let count = stale.len();
        let mut agents = self.agents.write();
        for id in &stale {
            if let Some(agent) = agents.get_mut(id) {
                warn!(agent_id = %id, "Marking stale agent as Failed");
                agent.state = AgentState::Failed;
            }
        }
        count
    }

    pub fn update_capabilities(
        &self,
        agent_id: &str,
        capabilities: Vec<AgentCapability>,
    ) -> Result<(), RegistryError> {
        let mut agents = self.agents.write();
        if let Some(agent) = agents.get_mut(agent_id) {
            agent.capabilities = capabilities;
            Ok(())
        } else {
            Err(RegistryError::AgentNotFound(agent_id.to_string()))
        }
    }

    pub fn state_summary(&self) -> HashMap<String, usize> {
        let mut summary = HashMap::new();
        for agent in self.agents.read().values() {
            *summary.entry(agent.state.to_string()).or_insert(0) += 1;
        }
        summary
    }

    pub fn all_capabilities(&self) -> HashSet<String> {
        self.agents
            .read()
            .values()
            .flat_map(|a| a.capabilities.iter().map(|c| c.name.clone()))
            .collect()
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct AgentRegistryHandle {
    inner: Arc<AgentRegistry>,
}

impl AgentRegistryHandle {
    pub fn new(registry: AgentRegistry) -> Self {
        Self {
            inner: Arc::new(registry),
        }
    }

    pub fn from_arc(arc: Arc<AgentRegistry>) -> Self {
        Self { inner: arc }
    }

    pub fn inner(&self) -> &AgentRegistry {
        &self.inner
    }

    pub fn into_inner(self) -> Arc<AgentRegistry> {
        self.inner
    }

    pub fn register(&self, info: AgentInfo) -> Result<(), RegistryError> {
        self.inner.register(info)
    }

    pub fn deregister(&self, agent_id: &str) -> Result<AgentInfo, RegistryError> {
        self.inner.deregister(agent_id)
    }

    pub fn set_state(&self, agent_id: &str, state: AgentState) -> Result<(), RegistryError> {
        self.inner.set_state(agent_id, state)
    }

    pub fn heartbeat(&self, agent_id: &str) -> Result<(), RegistryError> {
        self.inner.heartbeat(agent_id)
    }

    pub fn get(&self, agent_id: &str) -> Option<AgentInfo> {
        self.inner.get(agent_id)
    }

    pub fn all(&self) -> Vec<AgentInfo> {
        self.inner.all()
    }

    pub fn find_best_available(&self, capability: &str) -> Option<AgentInfo> {
        self.inner.find_best_available(capability)
    }

    pub fn assign_task(&self, agent_id: &str, task_id: &str) -> Result<(), RegistryError> {
        self.inner.assign_task(agent_id, task_id)
    }

    pub fn assign_task_guarded(
        &self,
        agent_id: &str,
        task_id: &str,
    ) -> Result<TaskAssignmentGuard, RegistryError> {
        self.inner.assign_task(agent_id, task_id)?;
        Ok(TaskAssignmentGuard {
            registry: self.clone(),
            agent_id: agent_id.to_string(),
            done: false,
        })
    }

    pub fn complete_task(&self, agent_id: &str, success: bool) {
        self.inner.complete_task(agent_id, success)
    }

    pub fn reconcile_loads_after_event_lag(&self) {
        self.inner.reconcile_loads_after_event_lag()
    }
}

pub struct TaskAssignmentGuard {
    registry: AgentRegistryHandle,
    agent_id: String,
    done: bool,
}

impl TaskAssignmentGuard {
    pub fn complete(mut self, success: bool) {
        self.registry.complete_task(&self.agent_id, success);
        self.done = true;
    }
}

impl Drop for TaskAssignmentGuard {
    fn drop(&mut self) {
        if !self.done {
            self.registry.complete_task(&self.agent_id, false);
        }
    }
}

impl From<AgentRegistry> for AgentRegistryHandle {
    fn from(registry: AgentRegistry) -> Self {
        Self::new(registry)
    }
}

impl From<Arc<AgentRegistry>> for AgentRegistryHandle {
    fn from(arc: Arc<AgentRegistry>) -> Self {
        Self::from_arc(arc)
    }
}
