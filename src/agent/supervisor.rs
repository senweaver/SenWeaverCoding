// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info};

use super::registry::{AgentId, AgentInfo, AgentRegistryHandle, AgentState};
use crate::agent::health_signal::{HealthBroadcaster, HealthSignal};
use crate::error::SupervisorError;

pub type UnhealthyProviderSet = Arc<RwLock<HashMap<(String, String), HealthSignal>>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorConfig {

    #[serde(default = "default_health_interval")]
    pub health_check_interval_secs: u64,

    #[serde(default = "default_heartbeat_timeout")]
    pub heartbeat_timeout_secs: u64,

    #[serde(default = "default_max_restarts")]
    pub max_restarts: u32,

    #[serde(default = "default_restart_backoff")]
    pub restart_backoff_base_secs: u64,

    #[serde(default = "default_max_agents")]
    pub max_agents: usize,

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

#[derive(Debug, Clone)]
struct RestartRecord {

    count: u32,

    last_restart: Instant,

    backoff: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorEvent {

    pub kind: SupervisorEventKind,

    pub agent_id: AgentId,

    pub timestamp: DateTime<Utc>,

    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorEventKind {

    Unhealthy,

    RestartInitiated,

    RestartSucceeded,

    RestartFailed,

    ShutDown,

    RegistrationDenied,

    Recovered,
}

pub type RestartCallback = Box<dyn Fn(&AgentInfo) -> bool + Send + Sync>;

pub struct Supervisor {
    config: SupervisorConfig,
    registry: AgentRegistryHandle,

    restart_history: RwLock<HashMap<AgentId, RestartRecord>>,

    events: RwLock<Vec<SupervisorEvent>>,

    max_event_log: usize,

    restart_callback: RwLock<Option<RestartCallback>>,

    unhealthy_providers: UnhealthyProviderSet,

    health_subscriber_started: std::sync::atomic::AtomicBool,
}

impl Supervisor {

    pub fn new(config: SupervisorConfig, registry: AgentRegistryHandle) -> Self {
        Self {
            config,
            registry,
            restart_history: RwLock::new(HashMap::new()),
            events: RwLock::new(Vec::new()),
            max_event_log: 1000,
            restart_callback: RwLock::new(None),
            unhealthy_providers: Arc::new(RwLock::new(HashMap::new())),
            health_subscriber_started: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn unhealthy_providers(&self) -> UnhealthyProviderSet {
        self.unhealthy_providers.clone()
    }

    pub fn is_provider_unhealthy(&self, provider: &str, model: &str) -> bool {
        self.unhealthy_providers
            .read()
            .contains_key(&(provider.to_string(), model.to_string()))
    }

    pub fn spawn_health_subscriber(
        &self,
        broadcaster: &HealthBroadcaster,
    ) -> Option<tokio::task::JoinHandle<()>> {
        if self
            .health_subscriber_started
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            return None;
        }
        let set = self.unhealthy_providers.clone();
        let mut rx = broadcaster.subscribe();
        Some(
            crate::runtime::spawn_supervised("supervisor.health_subscriber", async move {
                while let Ok(signal) = rx.recv().await {
                    let key = signal.key();
                    let mut guard = set.write();
                    if signal.is_unhealthy() {
                        guard.insert(key, signal);
                    } else {
                        guard.remove(&key);
                    }
                }
            })
            .into_inner(),
        )
    }

    pub fn set_restart_callback(&self, callback: RestartCallback) {
        *self.restart_callback.write() = Some(callback);
        info!("Supervisor: restart callback registered");
    }

    pub fn registry(&self) -> &AgentRegistryHandle {
        &self.registry
    }

    pub fn can_register(&self, info: &AgentInfo) -> Result<(), SupervisorError> {
        let current_count = self.registry.all().len();
        if current_count >= self.config.max_agents {
            return Err(SupervisorError::MaxAgentsLimit(self.config.max_agents));
        }

        for cap in &info.capabilities {
            if let Some(&limit) = self.config.capability_limits.get(&cap.name) {
                if limit == 0 {
                    continue;
                }
                let current = self.registry.inner().find_by_capability(&cap.name).len();
                if current >= limit {
                    return Err(SupervisorError::CapabilityLimit(cap.name.clone(), limit));
                }
            }
        }

        Ok(())
    }

    pub fn register_agent(&self, info: AgentInfo) -> Result<(), SupervisorError> {
        self.can_register(&info)?;
        if self.registry.register(info.clone()).is_ok() {
            info!(agent_id = %info.id, "Supervisor: agent registered");
            Ok(())
        } else {
            Err(SupervisorError::AlreadyRegistered(info.id))
        }
    }

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

            let _ = self.registry.set_state(agent_id, AgentState::Failed);

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

    fn should_restart(&self, agent_id: &str) -> bool {
        let history = self.restart_history.read();
        if let Some(record) = history.get(agent_id) {
            if record.count >= self.config.max_restarts {
                return false;
            }

            if record.last_restart.elapsed() < record.backoff {
                return false;
            }
        }
        true
    }

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

        let agent_info = self.registry.get(agent_id);

        let _ = self.registry.set_state(agent_id, AgentState::Restarting);

        let callback = self.restart_callback.read();
        let restart_succeeded = match (callback.as_ref(), agent_info) {
            (Some(cb), Some(info)) => cb(&info),
            (None, _) => {
                error!(
                    agent_id = %agent_id,
                    "Supervisor: no restart callback registered; cannot restart agent"
                );
                false
            }
            (Some(_), None) => {
                error!(
                    agent_id = %agent_id,
                    "Supervisor: agent missing from registry; cannot restart"
                );
                false
            }
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

            let _ = self.registry.set_state(agent_id, AgentState::Failed);
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

    pub fn shutdown_agent(&self, agent_id: &str) -> bool {
        if self
            .registry
            .set_state(agent_id, AgentState::ShuttingDown)
            .is_ok()
        {
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

    pub fn shutdown_all(&self) {
        let agents = self.registry.all();
        for agent in agents {
            if agent.state != AgentState::Terminated {
                self.shutdown_agent(&agent.id);
            }
        }
        info!("Supervisor: shutdown all agents");
    }

    pub fn recent_events(&self, limit: usize) -> Vec<SupervisorEvent> {
        let events = self.events.read();
        events.iter().rev().take(limit).cloned().collect()
    }

    pub fn restart_count(&self, agent_id: &str) -> u32 {
        self.restart_history
            .read()
            .get(agent_id)
            .map(|r| r.count)
            .unwrap_or(0)
    }

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorHealthReport {
    pub total_agents: usize,
    pub healthy: usize,
    pub unhealthy: usize,
    pub shutting_down: usize,
    pub state_summary: HashMap<String, usize>,
    pub timestamp: DateTime<Utc>,
}

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

    pub fn register_agent(&self, info: AgentInfo) -> Result<(), SupervisorError> {
        self.inner.register_agent(info)
    }

    pub fn health_check(&self) -> Vec<SupervisorEvent> {
        self.inner.health_check()
    }

    pub fn shutdown_agent(&self, agent_id: &str) -> bool {
        self.inner.shutdown_agent(agent_id)
    }

    pub fn shutdown_all(&self) {
        self.inner.shutdown_all();
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
