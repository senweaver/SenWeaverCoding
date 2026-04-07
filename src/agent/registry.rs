// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Agent Registry — centralized tracking of all agent instances.
//!
//! Provides runtime registration, state tracking, capability discovery,
//! and health monitoring for all active agents in the system.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

/// Unique agent identifier.
pub type AgentId = String;

/// Agent operational state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    /// Registered but not yet started.
    Idle,
    /// Currently processing a task.
    Active,
    /// Paused / suspended.
    Suspended,
    /// Gracefully shutting down.
    ShuttingDown,
    /// Terminated (terminal state).
    Terminated,
    /// Crashed / unhealthy.
    Failed,
    /// Currently restarting (transient state).
    Restarting,
}

impl std::fmt::Display for AgentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// A capability that an agent can perform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCapability {
    /// Capability name (e.g. "code_review", "summarization", "data_analysis").
    pub name: String,
    /// Description of the capability.
    pub description: String,
    /// Priority / proficiency score (0.0 – 1.0).
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

/// Metadata for a registered agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    /// Unique agent ID.
    pub id: AgentId,
    /// Human-readable name.
    pub name: String,
    /// Agent role / description.
    pub role: String,
    /// Current operational state.
    pub state: AgentState,
    /// Capabilities this agent advertises.
    pub capabilities: Vec<AgentCapability>,
    /// Provider + model identifier.
    pub model: String,
    /// When the agent was registered.
    pub registered_at: DateTime<Utc>,
    /// Last heartbeat timestamp.
    pub last_heartbeat: DateTime<Utc>,
    /// Number of tasks completed.
    pub tasks_completed: u64,
    /// Number of tasks failed.
    pub tasks_failed: u64,
    /// Current task ID (if active).
    pub current_task: Option<String>,
    /// Tags for filtering and grouping.
    pub tags: HashSet<String>,
    /// Maximum concurrent tasks this agent supports.
    pub max_concurrency: u32,
    /// Current load (number of in-flight tasks).
    pub current_load: u32,
}

impl AgentInfo {
    /// Create a new agent info with sensible defaults.
    pub fn new(id: impl Into<String>, name: impl Into<String>, role: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: id.into(),
            name: name.into(),
            role: role.into(),
            state: AgentState::Idle,
            capabilities: Vec::new(),
            model: String::new(),
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

    /// Whether the agent is available for new work.
    pub fn is_available(&self) -> bool {
        self.state == AgentState::Idle
            || (self.state == AgentState::Active && self.current_load < self.max_concurrency)
    }

    /// Whether the agent has a specific capability.
    pub fn has_capability(&self, name: &str) -> bool {
        self.capabilities.iter().any(|c| c.name == name)
    }

    /// Get proficiency for a capability (0.0 if not found).
    pub fn proficiency_for(&self, capability: &str) -> f64 {
        self.capabilities
            .iter()
            .find(|c| c.name == capability)
            .map(|c| c.proficiency)
            .unwrap_or(0.0)
    }
}

/// Central agent registry.
///
/// Thread-safe registry that tracks all agent instances, their states,
/// capabilities, and health. Supports capability-based discovery and
/// load-aware routing.
pub struct AgentRegistry {
    agents: RwLock<HashMap<AgentId, AgentInfo>>,
    /// Heartbeat timeout: agents not heard from within this window are marked stale.
    heartbeat_timeout: Duration,
}

impl AgentRegistry {
    /// Create a new registry with default heartbeat timeout (60s).
    pub fn new() -> Self {
        Self {
            agents: RwLock::new(HashMap::new()),
            heartbeat_timeout: Duration::from_secs(60),
        }
    }

    /// Create a registry with a custom heartbeat timeout.
    pub fn with_heartbeat_timeout(timeout: Duration) -> Self {
        Self {
            agents: RwLock::new(HashMap::new()),
            heartbeat_timeout: timeout,
        }
    }

    /// Register a new agent. Returns false if ID already exists.
    pub fn register(&self, info: AgentInfo) -> bool {
        let mut agents = self.agents.write();
        if agents.contains_key(&info.id) {
            warn!(agent_id = %info.id, "Agent already registered");
            return false;
        }
        info!(agent_id = %info.id, name = %info.name, "Agent registered");
        agents.insert(info.id.clone(), info);
        true
    }

    /// Deregister an agent. Returns the removed info if it existed.
    pub fn deregister(&self, agent_id: &str) -> Option<AgentInfo> {
        let mut agents = self.agents.write();
        let removed = agents.remove(agent_id);
        if removed.is_some() {
            info!(agent_id = %agent_id, "Agent deregistered");
        }
        removed
    }

    /// Update agent state.
    pub fn set_state(&self, agent_id: &str, state: AgentState) -> bool {
        let mut agents = self.agents.write();
        if let Some(agent) = agents.get_mut(agent_id) {
            debug!(agent_id = %agent_id, old = %agent.state, new = %state, "Agent state change");
            agent.state = state;
            agent.last_heartbeat = Utc::now();
            true
        } else {
            false
        }
    }

    /// Record a heartbeat from an agent.
    pub fn heartbeat(&self, agent_id: &str) -> bool {
        let mut agents = self.agents.write();
        if let Some(agent) = agents.get_mut(agent_id) {
            agent.last_heartbeat = Utc::now();
            true
        } else {
            false
        }
    }

    /// Assign a task to an agent.
    pub fn assign_task(&self, agent_id: &str, task_id: &str) -> bool {
        let mut agents = self.agents.write();
        if let Some(agent) = agents.get_mut(agent_id) {
            if !agent.is_available() {
                return false;
            }
            agent.current_task = Some(task_id.to_string());
            agent.current_load += 1;
            if agent.state == AgentState::Idle {
                agent.state = AgentState::Active;
            }
            agent.last_heartbeat = Utc::now();
            true
        } else {
            false
        }
    }

    /// Complete a task on an agent.
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
                agent.state = AgentState::Idle;
            }
            agent.last_heartbeat = Utc::now();
        }
    }

    /// Get a snapshot of an agent's info.
    pub fn get(&self, agent_id: &str) -> Option<AgentInfo> {
        self.agents.read().get(agent_id).cloned()
    }

    /// Get all registered agents.
    pub fn all(&self) -> Vec<AgentInfo> {
        self.agents.read().values().cloned().collect()
    }

    /// Get agents in a specific state.
    pub fn by_state(&self, state: AgentState) -> Vec<AgentInfo> {
        self.agents
            .read()
            .values()
            .filter(|a| a.state == state)
            .cloned()
            .collect()
    }

    /// Find agents with a specific capability, sorted by proficiency descending.
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

    /// Find the best available agent for a capability (highest proficiency, lowest load).
    pub fn find_best_available(&self, capability: &str) -> Option<AgentInfo> {
        let agents = self.agents.read();
        agents
            .values()
            .filter(|a| a.is_available() && a.has_capability(capability))
            .max_by(|a, b| {
                let score_a = a.proficiency_for(capability)
                    * (1.0 - a.current_load as f64 / a.max_concurrency.max(1) as f64);
                let score_b = b.proficiency_for(capability)
                    * (1.0 - b.current_load as f64 / b.max_concurrency.max(1) as f64);
                score_a
                    .partial_cmp(&score_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned()
    }

    /// Find agents by tag.
    pub fn find_by_tag(&self, tag: &str) -> Vec<AgentInfo> {
        self.agents
            .read()
            .values()
            .filter(|a| a.tags.contains(tag))
            .cloned()
            .collect()
    }

    /// Get count of active agents.
    pub fn active_count(&self) -> usize {
        self.agents
            .read()
            .values()
            .filter(|a| a.state == AgentState::Active)
            .count()
    }

    /// Get total agent count.
    pub fn count(&self) -> usize {
        self.agents.read().len()
    }

    /// Check for stale agents (no heartbeat within timeout).
    /// Returns IDs of stale agents.
    pub fn check_stale(&self) -> Vec<AgentId> {
        let now = Utc::now();
        let timeout_secs = self.heartbeat_timeout.as_secs() as i64;
        self.agents
            .read()
            .iter()
            .filter(|(_, a)| {
                a.state != AgentState::Terminated
                    && (now - a.last_heartbeat).num_seconds() > timeout_secs
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Mark stale agents as Failed. Returns the number of agents marked.
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

    /// Update agent capabilities.
    pub fn update_capabilities(&self, agent_id: &str, capabilities: Vec<AgentCapability>) -> bool {
        let mut agents = self.agents.write();
        if let Some(agent) = agents.get_mut(agent_id) {
            agent.capabilities = capabilities;
            true
        } else {
            false
        }
    }

    /// Get a summary of all agent states.
    pub fn state_summary(&self) -> HashMap<String, usize> {
        let mut summary = HashMap::new();
        for agent in self.agents.read().values() {
            *summary.entry(agent.state.to_string()).or_insert(0) += 1;
        }
        summary
    }

    /// Get all unique capabilities across all agents.
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

/// Thread-safe handle to the registry.
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

    pub fn register(&self, info: AgentInfo) -> bool {
        self.inner.register(info)
    }

    pub fn deregister(&self, agent_id: &str) -> Option<AgentInfo> {
        self.inner.deregister(agent_id)
    }

    pub fn set_state(&self, agent_id: &str, state: AgentState) -> bool {
        self.inner.set_state(agent_id, state)
    }

    pub fn heartbeat(&self, agent_id: &str) -> bool {
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

    pub fn assign_task(&self, agent_id: &str, task_id: &str) -> bool {
        self.inner.assign_task(agent_id, task_id)
    }

    pub fn complete_task(&self, agent_id: &str, success: bool) {
        self.inner.complete_task(agent_id, success)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_agent(id: &str, name: &str) -> AgentInfo {
        let mut info = AgentInfo::new(id, name, "test role");
        info.capabilities.push(AgentCapability {
            name: "code_review".to_string(),
            description: "Reviews code".to_string(),
            proficiency: 0.9,
        });
        info
    }

    #[test]
    fn register_and_get() {
        let registry = AgentRegistry::new();
        let agent = test_agent("a1", "Agent 1");
        assert!(registry.register(agent));
        assert!(registry.get("a1").is_some());
        assert_eq!(registry.count(), 1);
    }

    #[test]
    fn register_duplicate_fails() {
        let registry = AgentRegistry::new();
        assert!(registry.register(test_agent("a1", "Agent 1")));
        assert!(!registry.register(test_agent("a1", "Agent 1 dup")));
    }

    #[test]
    fn deregister() {
        let registry = AgentRegistry::new();
        registry.register(test_agent("a1", "Agent 1"));
        assert!(registry.deregister("a1").is_some());
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn state_transitions() {
        let registry = AgentRegistry::new();
        registry.register(test_agent("a1", "Agent 1"));

        assert!(registry.set_state("a1", AgentState::Active));
        assert_eq!(registry.get("a1").unwrap().state, AgentState::Active);

        assert!(registry.set_state("a1", AgentState::Terminated));
        assert_eq!(registry.get("a1").unwrap().state, AgentState::Terminated);
    }

    #[test]
    fn find_by_capability() {
        let registry = AgentRegistry::new();
        let mut a1 = test_agent("a1", "Agent 1");
        a1.capabilities[0].proficiency = 0.9;
        let mut a2 = AgentInfo::new("a2", "Agent 2", "test");
        a2.capabilities.push(AgentCapability {
            name: "code_review".to_string(),
            description: "".to_string(),
            proficiency: 0.7,
        });

        registry.register(a1);
        registry.register(a2);

        let results = registry.find_by_capability("code_review");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "a1"); // higher proficiency first
    }

    #[test]
    fn find_best_available() {
        let registry = AgentRegistry::new();
        let a1 = test_agent("a1", "Agent 1");
        registry.register(a1);

        let best = registry.find_best_available("code_review");
        assert!(best.is_some());
        assert_eq!(best.unwrap().id, "a1");

        // Make agent busy
        registry.set_state("a1", AgentState::Suspended);
        let best = registry.find_best_available("code_review");
        assert!(best.is_none());
    }

    #[test]
    fn task_assignment() {
        let registry = AgentRegistry::new();
        registry.register(test_agent("a1", "Agent 1"));

        assert!(registry.assign_task("a1", "task-1"));
        assert_eq!(registry.get("a1").unwrap().state, AgentState::Active);
        assert_eq!(registry.get("a1").unwrap().current_load, 1);

        registry.complete_task("a1", true);
        assert_eq!(registry.get("a1").unwrap().state, AgentState::Idle);
        assert_eq!(registry.get("a1").unwrap().tasks_completed, 1);
    }

    #[test]
    fn by_state_filter() {
        let registry = AgentRegistry::new();
        registry.register(test_agent("a1", "Agent 1"));
        registry.register(test_agent("a2", "Agent 2"));
        registry.set_state("a2", AgentState::Active);

        let idle = registry.by_state(AgentState::Idle);
        assert_eq!(idle.len(), 1);
        assert_eq!(idle[0].id, "a1");
    }

    #[test]
    fn state_summary() {
        let registry = AgentRegistry::new();
        registry.register(test_agent("a1", "Agent 1"));
        registry.register(test_agent("a2", "Agent 2"));
        registry.set_state("a2", AgentState::Active);

        let summary = registry.state_summary();
        assert_eq!(summary.get("Idle"), Some(&1));
        assert_eq!(summary.get("Active"), Some(&1));
    }

    #[test]
    fn find_by_tag() {
        let registry = AgentRegistry::new();
        let mut a1 = test_agent("a1", "Agent 1");
        a1.tags.insert("fast".to_string());
        registry.register(a1);
        registry.register(test_agent("a2", "Agent 2"));

        let tagged = registry.find_by_tag("fast");
        assert_eq!(tagged.len(), 1);
        assert_eq!(tagged[0].id, "a1");
    }

    #[test]
    fn all_capabilities() {
        let registry = AgentRegistry::new();
        let mut a1 = test_agent("a1", "Agent 1");
        a1.capabilities.push(AgentCapability {
            name: "summarize".to_string(),
            description: "".to_string(),
            proficiency: 0.8,
        });
        registry.register(a1);

        let caps = registry.all_capabilities();
        assert!(caps.contains("code_review"));
        assert!(caps.contains("summarize"));
    }

    #[test]
    fn handle_operations() {
        let handle = AgentRegistryHandle::new(AgentRegistry::new());
        assert!(handle.register(test_agent("a1", "Agent 1")));
        assert!(handle.get("a1").is_some());
        assert_eq!(handle.all().len(), 1);
    }
}
