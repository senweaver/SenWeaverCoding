// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Agent Supervisor — lifecycle management, health monitoring, and auto-recovery.
//!
//! The supervisor watches over all registered agents and provides:
//! - **Health monitoring** via periodic heartbeat checks
//! - **Automatic restart** of failed agents (with backoff)
//! - **Graceful shutdown** coordination
//! - **Resource limits** enforcement (max agents, per-capability limits)
//! - **Load balancing** feedback to the task queue

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info};

use super::registry::{AgentId, AgentInfo, AgentRegistryHandle, AgentState};

/// Configuration for the supervisor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorConfig {
    /// Interval between health checks (seconds).
    #[serde(default = "default_health_interval")]
    pub health_check_interval_secs: u64,
    /// Maximum time without heartbeat before agent is considered failed (seconds).
    #[serde(default = "default_heartbeat_timeout")]
    pub heartbeat_timeout_secs: u64,
    /// Maximum number of restart attempts before giving up.
    #[serde(default = "default_max_restarts")]
    pub max_restarts: u32,
    /// Base backoff delay between restarts (seconds). Doubles each attempt.
    #[serde(default = "default_restart_backoff")]
    pub restart_backoff_base_secs: u64,
    /// Maximum number of concurrent agents.
    #[serde(default = "default_max_agents")]
    pub max_agents: usize,
    /// Per-capability agent limits (0 = unlimited).
    #[serde(default)]
    pub capability_limits: HashMap<String, usize>,
}

fn default_health_interval() -> u64 {
    30
}
fn default_heartbeat_timeout() -> u64 {
    60
}
fn default_max_restarts() -> u32 {
    3
}
fn default_restart_backoff() -> u64 {
    5
}
fn default_max_agents() -> usize {
    50
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self {
            health_check_interval_secs: default_health_interval(),
            heartbeat_timeout_secs: default_heartbeat_timeout(),
            max_restarts: default_max_restarts(),
            restart_backoff_base_secs: default_restart_backoff(),
            max_agents: default_max_agents(),
            capability_limits: HashMap::new(),
        }
    }
}

/// Tracks restart history for an agent.
#[derive(Debug, Clone)]
struct RestartRecord {
    /// Number of restarts so far.
    count: u32,
    /// When the last restart was attempted.
    last_restart: Instant,
    /// Current backoff duration.
    backoff: Duration,
}

/// A supervisor event (emitted for monitoring / hooks).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorEvent {
    /// Event type.
    pub kind: SupervisorEventKind,
    /// Agent involved.
    pub agent_id: AgentId,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
    /// Human-readable detail.
    pub detail: String,
}

/// Kinds of supervisor events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorEventKind {
    /// Agent detected as unhealthy.
    Unhealthy,
    /// Agent restart initiated.
    RestartInitiated,
    /// Agent restart succeeded.
    RestartSucceeded,
    /// Agent restart failed (giving up).
    RestartFailed,
    /// Agent gracefully shut down.
    ShutDown,
    /// Agent registration denied (limit reached).
    RegistrationDenied,
    /// Agent recovered from failure.
    Recovered,
}

/// Callback for restarting an agent. Returns true if restart succeeded.
pub type RestartCallback = Box<dyn Fn(&AgentInfo) -> bool + Send + Sync>;

/// The agent supervisor.
pub struct Supervisor {
    config: SupervisorConfig,
    registry: AgentRegistryHandle,
    /// Restart history per agent ID.
    restart_history: RwLock<HashMap<AgentId, RestartRecord>>,
    /// Event log (bounded).
    events: RwLock<Vec<SupervisorEvent>>,
    /// Maximum event log size.
    max_event_log: usize,
    /// Optional restart callback - if set, called to actually restart agents.
    restart_callback: RwLock<Option<RestartCallback>>,
}

impl Supervisor {
    /// Create a new supervisor with the given config and registry.
    pub fn new(config: SupervisorConfig, registry: AgentRegistryHandle) -> Self {
        Self {
            config,
            registry,
            restart_history: RwLock::new(HashMap::new()),
            events: RwLock::new(Vec::new()),
            max_event_log: 1000,
            restart_callback: RwLock::new(None),
        }
    }

    /// Set the restart callback. This enables actual agent restarts.
    pub fn set_restart_callback(&self, callback: RestartCallback) {
        *self.restart_callback.write() = Some(callback);
        info!("Supervisor: restart callback registered");
    }

    /// Get a reference to the registry.
    pub fn registry(&self) -> &AgentRegistryHandle {
        &self.registry
    }

    /// Check if a new agent can be registered (within limits).
    pub fn can_register(&self, info: &AgentInfo) -> Result<(), String> {
        let current_count = self.registry.all().len();
        if current_count >= self.config.max_agents {
            return Err(format!(
                "Maximum agent limit ({}) reached",
                self.config.max_agents
            ));
        }

        // Check per-capability limits
        for cap in &info.capabilities {
            if let Some(&limit) = self.config.capability_limits.get(&cap.name) {
                if limit == 0 {
                    continue;
                }
                let current = self.registry.inner().find_by_capability(&cap.name).len();
                if current >= limit {
                    return Err(format!(
                        "Capability '{}' agent limit ({}) reached",
                        cap.name, limit
                    ));
                }
            }
        }

        Ok(())
    }

    /// Register an agent through the supervisor (with limit checks).
    pub fn register_agent(&self, info: AgentInfo) -> Result<(), String> {
        self.can_register(&info)?;
        if self.registry.register(info.clone()) {
            info!(agent_id = %info.id, "Supervisor: agent registered");
            Ok(())
        } else {
            Err(format!("Agent '{}' already registered", info.id))
        }
    }

    /// Run one cycle of health checks.
    ///
    /// Returns a list of supervisor events generated during the check.
    pub fn health_check(&self) -> Vec<SupervisorEvent> {
        let mut events = Vec::new();

        let stale_ids = self.registry.inner().check_stale();

        for agent_id in &stale_ids {
            let event = SupervisorEvent {
                kind: SupervisorEventKind::Unhealthy,
                agent_id: agent_id.clone(),
                timestamp: Utc::now(),
                detail: "Heartbeat timeout exceeded".to_string(),
            };
            events.push(event.clone());
            self.record_event(event);

            // Mark as failed
            self.registry.set_state(agent_id, AgentState::Failed);

            // Attempt restart
            if self.should_restart(agent_id) {
                self.initiate_restart(agent_id, &mut events);
            } else {
                let give_up = SupervisorEvent {
                    kind: SupervisorEventKind::RestartFailed,
                    agent_id: agent_id.clone(),
                    timestamp: Utc::now(),
                    detail: "Max restart attempts exceeded".to_string(),
                };
                events.push(give_up.clone());
                self.record_event(give_up);
                error!(agent_id = %agent_id, "Supervisor: giving up on agent restart");
            }
        }

        events
    }

    /// Check if an agent should be restarted.
    fn should_restart(&self, agent_id: &str) -> bool {
        let history = self.restart_history.read();
        if let Some(record) = history.get(agent_id) {
            if record.count >= self.config.max_restarts {
                return false;
            }
            // Check backoff: don't restart too soon
            if record.last_restart.elapsed() < record.backoff {
                return false;
            }
        }
        true
    }

    /// Initiate a restart for an agent.
    /// If a restart callback is registered, it will be called to actually restart the agent.
    /// Otherwise, the agent state is just reset to Idle for manual restart.
    fn initiate_restart(&self, agent_id: &str, events: &mut Vec<SupervisorEvent>) {
        let mut history = self.restart_history.write();
        let record = history
            .entry(agent_id.to_string())
            .or_insert_with(|| RestartRecord {
                count: 0,
                last_restart: Instant::now(),
                backoff: Duration::from_secs(self.config.restart_backoff_base_secs),
            });

        record.count += 1;
        record.last_restart = Instant::now();
        // Exponential backoff
        record.backoff = Duration::from_secs(
            self.config.restart_backoff_base_secs * 2u64.pow(record.count.min(6)),
        );

        let event = SupervisorEvent {
            kind: SupervisorEventKind::RestartInitiated,
            agent_id: agent_id.to_string(),
            timestamp: Utc::now(),
            detail: format!(
                "Restart attempt {}/ {}",
                record.count, self.config.max_restarts
            ),
        };
        events.push(event.clone());
        self.record_event(event);

        // Get agent info before restart
        let agent_info = self.registry.get(agent_id);

        // Mark as restarting
        self.registry.set_state(agent_id, AgentState::Restarting);

        // Try to actually restart if callback is set
        let callback = self.restart_callback.read();
        let restart_succeeded = if let (Some(cb), Some(info)) = (callback.as_ref(), agent_info) {
            cb(&info)
        } else {
            // No callback or agent not found - just reset to Idle for manual handling
            self.registry.set_state(agent_id, AgentState::Idle);
            true // Consider this a "success" for state tracking
        };

        if restart_succeeded {
            let success_event = SupervisorEvent {
                kind: SupervisorEventKind::RestartSucceeded,
                agent_id: agent_id.to_string(),
                timestamp: Utc::now(),
                detail: format!("Agent restarted successfully (attempt {})", record.count),
            };
            events.push(success_event.clone());
            self.record_event(success_event);
            info!(
                agent_id = %agent_id,
                attempt = record.count,
                "Supervisor: agent restarted successfully"
            );
        } else {
            // Restart failed - mark as Failed
            self.registry.set_state(agent_id, AgentState::Failed);
            let fail_event = SupervisorEvent {
                kind: SupervisorEventKind::RestartFailed,
                agent_id: agent_id.to_string(),
                timestamp: Utc::now(),
                detail: format!("Restart callback failed (attempt {})", record.count),
            };
            events.push(fail_event.clone());
            self.record_event(fail_event);
            error!(
                agent_id = %agent_id,
                attempt = record.count,
                "Supervisor: agent restart failed"
            );
        }
    }

    /// Mark an agent as successfully recovered (resets restart counter).
    pub fn mark_recovered(&self, agent_id: &str) {
        self.restart_history.write().remove(agent_id);
        let event = SupervisorEvent {
            kind: SupervisorEventKind::Recovered,
            agent_id: agent_id.to_string(),
            timestamp: Utc::now(),
            detail: "Agent recovered, restart counter reset".to_string(),
        };
        self.record_event(event);
        debug!(agent_id = %agent_id, "Supervisor: agent recovered");
    }

    /// Initiate graceful shutdown of an agent.
    pub fn shutdown_agent(&self, agent_id: &str) -> bool {
        if self.registry.set_state(agent_id, AgentState::ShuttingDown) {
            let event = SupervisorEvent {
                kind: SupervisorEventKind::ShutDown,
                agent_id: agent_id.to_string(),
                timestamp: Utc::now(),
                detail: "Graceful shutdown initiated".to_string(),
            };
            self.record_event(event);
            info!(agent_id = %agent_id, "Supervisor: shutdown initiated");
            true
        } else {
            false
        }
    }

    /// Initiate graceful shutdown of all agents.
    pub fn shutdown_all(&self) {
        let agents = self.registry.all();
        for agent in agents {
            if agent.state != AgentState::Terminated {
                self.shutdown_agent(&agent.id);
            }
        }
        info!("Supervisor: shutdown all agents");
    }

    /// Get recent supervisor events.
    pub fn recent_events(&self, limit: usize) -> Vec<SupervisorEvent> {
        let events = self.events.read();
        events.iter().rev().take(limit).cloned().collect()
    }

    /// Get restart count for an agent.
    pub fn restart_count(&self, agent_id: &str) -> u32 {
        self.restart_history
            .read()
            .get(agent_id)
            .map(|r| r.count)
            .unwrap_or(0)
    }

    /// Get a health report for all agents.
    pub fn health_report(&self) -> SupervisorHealthReport {
        let agents = self.registry.all();
        let total = agents.len();
        let healthy = agents
            .iter()
            .filter(|a| matches!(a.state, AgentState::Idle | AgentState::Active))
            .count();
        let unhealthy = agents
            .iter()
            .filter(|a| a.state == AgentState::Failed)
            .count();
        let shutting_down = agents
            .iter()
            .filter(|a| a.state == AgentState::ShuttingDown)
            .count();

        SupervisorHealthReport {
            total_agents: total,
            healthy,
            unhealthy,
            shutting_down,
            state_summary: self.registry.inner().state_summary(),
            timestamp: Utc::now(),
        }
    }

    fn record_event(&self, event: SupervisorEvent) {
        let mut events = self.events.write();
        if events.len() >= self.max_event_log {
            let half = events.len() / 2;
            events.drain(0..half);
        }
        events.push(event);
    }
}

/// Health report from the supervisor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorHealthReport {
    pub total_agents: usize,
    pub healthy: usize,
    pub unhealthy: usize,
    pub shutting_down: usize,
    pub state_summary: HashMap<String, usize>,
    pub timestamp: DateTime<Utc>,
}

/// Thread-safe handle to the supervisor.
#[derive(Clone)]
pub struct SupervisorHandle {
    inner: Arc<Supervisor>,
}

impl SupervisorHandle {
    pub fn new(supervisor: Supervisor) -> Self {
        Self {
            inner: Arc::new(supervisor),
        }
    }

    pub fn inner(&self) -> &Supervisor {
        &self.inner
    }

    pub fn register_agent(&self, info: AgentInfo) -> Result<(), String> {
        self.inner.register_agent(info)
    }

    pub fn health_check(&self) -> Vec<SupervisorEvent> {
        self.inner.health_check()
    }

    pub fn shutdown_agent(&self, agent_id: &str) -> bool {
        self.inner.shutdown_agent(agent_id)
    }

    pub fn shutdown_all(&self) {
        self.inner.shutdown_all()
    }

    pub fn health_report(&self) -> SupervisorHealthReport {
        self.inner.health_report()
    }

    pub fn registry(&self) -> &AgentRegistryHandle {
        self.inner.registry()
    }

    pub fn set_restart_callback(&self, callback: RestartCallback) {
        self.inner.set_restart_callback(callback);
    }
}

impl From<Supervisor> for SupervisorHandle {
    fn from(s: Supervisor) -> Self {
        Self::new(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::registry::{AgentCapability, AgentRegistry};

    fn make_supervisor() -> (Supervisor, AgentRegistryHandle) {
        let registry = AgentRegistryHandle::new(AgentRegistry::new());
        let config = SupervisorConfig::default();
        let supervisor = Supervisor::new(config, registry.clone());
        (supervisor, registry)
    }

    fn test_agent(id: &str) -> AgentInfo {
        let mut info = AgentInfo::new(id, format!("Agent {}", id), "worker");
        info.capabilities.push(AgentCapability {
            name: "general".to_string(),
            description: "General tasks".to_string(),
            proficiency: 0.8,
        });
        info
    }

    #[test]
    fn register_through_supervisor() {
        let (supervisor, _) = make_supervisor();
        assert!(supervisor.register_agent(test_agent("a1")).is_ok());
        assert!(supervisor.register_agent(test_agent("a1")).is_err()); // duplicate
    }

    #[test]
    fn max_agents_limit() {
        let registry = AgentRegistryHandle::new(AgentRegistry::new());
        let config = SupervisorConfig {
            max_agents: 2,
            ..Default::default()
        };
        let supervisor = Supervisor::new(config, registry);

        assert!(supervisor.register_agent(test_agent("a1")).is_ok());
        assert!(supervisor.register_agent(test_agent("a2")).is_ok());
        assert!(supervisor.register_agent(test_agent("a3")).is_err());
    }

    #[test]
    fn capability_limit() {
        let registry = AgentRegistryHandle::new(AgentRegistry::new());
        let mut caps = HashMap::new();
        caps.insert("general".to_string(), 1);
        let config = SupervisorConfig {
            capability_limits: caps,
            ..Default::default()
        };
        let supervisor = Supervisor::new(config, registry);

        assert!(supervisor.register_agent(test_agent("a1")).is_ok());
        assert!(supervisor.register_agent(test_agent("a2")).is_err());
    }

    #[test]
    fn health_check_no_stale() {
        let (supervisor, _) = make_supervisor();
        supervisor.register_agent(test_agent("a1")).unwrap();
        let events = supervisor.health_check();
        assert!(events.is_empty());
    }

    #[test]
    fn shutdown_agent() {
        let (supervisor, registry) = make_supervisor();
        supervisor.register_agent(test_agent("a1")).unwrap();

        assert!(supervisor.shutdown_agent("a1"));
        assert_eq!(registry.get("a1").unwrap().state, AgentState::ShuttingDown);
    }

    #[test]
    fn shutdown_all() {
        let (supervisor, registry) = make_supervisor();
        supervisor.register_agent(test_agent("a1")).unwrap();
        supervisor.register_agent(test_agent("a2")).unwrap();

        supervisor.shutdown_all();
        assert_eq!(registry.get("a1").unwrap().state, AgentState::ShuttingDown);
        assert_eq!(registry.get("a2").unwrap().state, AgentState::ShuttingDown);
    }

    #[test]
    fn health_report() {
        let (supervisor, _) = make_supervisor();
        supervisor.register_agent(test_agent("a1")).unwrap();
        supervisor.register_agent(test_agent("a2")).unwrap();

        let report = supervisor.health_report();
        assert_eq!(report.total_agents, 2);
        assert_eq!(report.healthy, 2);
        assert_eq!(report.unhealthy, 0);
    }

    #[test]
    fn mark_recovered_resets_restarts() {
        let (supervisor, _) = make_supervisor();
        supervisor.register_agent(test_agent("a1")).unwrap();

        // Simulate some restarts
        {
            let mut history = supervisor.restart_history.write();
            history.insert(
                "a1".to_string(),
                RestartRecord {
                    count: 2,
                    last_restart: Instant::now(),
                    backoff: Duration::from_secs(10),
                },
            );
        }
        assert_eq!(supervisor.restart_count("a1"), 2);

        supervisor.mark_recovered("a1");
        assert_eq!(supervisor.restart_count("a1"), 0);
    }

    #[test]
    fn recent_events() {
        let (supervisor, _) = make_supervisor();
        supervisor.register_agent(test_agent("a1")).unwrap();
        supervisor.shutdown_agent("a1");

        let events = supervisor.recent_events(10);
        assert!(!events.is_empty());
        assert_eq!(events[0].kind, SupervisorEventKind::ShutDown);
    }

    #[test]
    fn handle_operations() {
        let (supervisor, _) = make_supervisor();
        let handle = SupervisorHandle::new(supervisor);

        assert!(handle.register_agent(test_agent("a1")).is_ok());
        let report = handle.health_report();
        assert_eq!(report.total_agents, 1);
    }
}
