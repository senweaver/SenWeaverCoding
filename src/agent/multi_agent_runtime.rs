// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Multi-Agent Runtime — unified entry point for all collaboration infrastructure.
//!
//! Bundles the Agent Registry, Task Queue, Supervisor, Coordinator, and Blackboard
//! into a single runtime that is initialized once at system startup (gateway/daemon)
//! and shared across the system via a global handle.

use std::sync::LazyLock;

use parking_lot::RwLock;
use tracing::{debug, info};

use super::coordination::{Coordinator, CoordinatorHandle};
use super::registry::{AgentRegistry, AgentRegistryHandle};
use super::supervisor::{Supervisor, SupervisorConfig, SupervisorHandle};
use super::task_queue::{TaskQueue, TaskQueueHandle};
use crate::memory::blackboard::{Blackboard, BlackboardHandle};

/// The multi-agent runtime containing all collaboration primitives.
#[derive(Clone)]
pub struct MultiAgentRuntime {
    pub registry: AgentRegistryHandle,
    pub supervisor: SupervisorHandle,
    pub task_queue: TaskQueueHandle,
    pub coordinator: CoordinatorHandle,
    pub blackboard: BlackboardHandle,
}

impl MultiAgentRuntime {
    /// Create a new runtime with default configuration.
    pub fn new() -> Self {
        Self::with_config(SupervisorConfig::default())
    }

    /// Create a new runtime with custom supervisor configuration.
    pub fn with_config(supervisor_config: SupervisorConfig) -> Self {
        let registry = AgentRegistryHandle::new(AgentRegistry::new());
        let supervisor =
            SupervisorHandle::new(Supervisor::new(supervisor_config, registry.clone()));
        let task_queue = TaskQueueHandle::new(TaskQueue::new());
        let coordinator = CoordinatorHandle::new(Coordinator::new());
        let blackboard = BlackboardHandle::new(Blackboard::new());

        info!("Multi-agent runtime initialized");

        Self {
            registry,
            supervisor,
            task_queue,
            coordinator,
            blackboard,
        }
    }

    /// Run periodic maintenance across all subsystems.
    ///
    /// Should be called on a timer (e.g. every 30s) to:
    /// - Run supervisor health checks (detect stale agents, auto-restart)
    /// - Expire overdue tasks in the queue
    /// - Evict expired blackboard entries
    /// - Evict expired locks, barriers, and voting sessions
    pub fn maintenance(&self) -> MaintenanceReport {
        let supervisor_events = self.supervisor.health_check();
        let expired_tasks = self.task_queue.inner().expire_overdue();
        let expired_entries = self.blackboard.inner().evict_expired();
        let (expired_locks, expired_barriers, expired_votes) = self.coordinator.maintenance();

        if !supervisor_events.is_empty()
            || expired_tasks > 0
            || expired_entries > 0
            || expired_locks > 0
        {
            debug!(
                supervisor_events = supervisor_events.len(),
                expired_tasks,
                expired_entries,
                expired_locks,
                expired_barriers,
                expired_votes,
                "Multi-agent runtime maintenance cycle"
            );
        }

        MaintenanceReport {
            supervisor_events_count: supervisor_events.len(),
            expired_tasks,
            expired_entries,
            expired_locks,
            expired_barriers,
            expired_votes,
        }
    }

    /// Get a health summary of the entire multi-agent runtime.
    pub fn health_summary(&self) -> RuntimeHealthSummary {
        let supervisor_report = self.supervisor.health_report();
        let pending_tasks = self.task_queue.pending_count();
        let running_tasks = self.task_queue.running_count();
        let blackboard_entries = self.blackboard.inner().len();

        RuntimeHealthSummary {
            total_agents: supervisor_report.total_agents,
            healthy_agents: supervisor_report.healthy,
            unhealthy_agents: supervisor_report.unhealthy,
            pending_tasks,
            running_tasks,
            blackboard_entries,
        }
    }

    /// Graceful shutdown of all agents.
    pub fn shutdown(&self) {
        info!("Multi-agent runtime shutting down");
        self.supervisor.shutdown_all();
    }
}

impl Default for MultiAgentRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// Report from a maintenance cycle.
#[derive(Debug, Clone, Default)]
pub struct MaintenanceReport {
    pub supervisor_events_count: usize,
    pub expired_tasks: usize,
    pub expired_entries: usize,
    pub expired_locks: usize,
    pub expired_barriers: usize,
    pub expired_votes: usize,
}

/// Health summary of the runtime.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RuntimeHealthSummary {
    pub total_agents: usize,
    pub healthy_agents: usize,
    pub unhealthy_agents: usize,
    pub pending_tasks: usize,
    pub running_tasks: usize,
    pub blackboard_entries: usize,
}

// ── Global Runtime Singleton ────────────────────────────────────────

static GLOBAL_RUNTIME: LazyLock<RwLock<Option<MultiAgentRuntime>>> =
    LazyLock::new(|| RwLock::new(None));

/// Initialize the global multi-agent runtime. Call once at startup.
/// Returns the runtime handle for local use.
pub fn init_global_runtime() -> MultiAgentRuntime {
    init_global_runtime_with_config(SupervisorConfig::default())
}

/// Initialize the global multi-agent runtime with custom config.
pub fn init_global_runtime_with_config(config: SupervisorConfig) -> MultiAgentRuntime {
    let runtime = MultiAgentRuntime::with_config(config);
    *GLOBAL_RUNTIME.write() = Some(runtime.clone());
    info!("Global multi-agent runtime initialized");
    runtime
}

/// Get a reference to the global multi-agent runtime, if initialized.
pub fn global_runtime() -> Option<MultiAgentRuntime> {
    GLOBAL_RUNTIME.read().clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::registry::{AgentCapability, AgentInfo};
    use crate::agent::task_queue::{Task, TaskPriority};

    #[test]
    fn runtime_initialization() {
        let rt = MultiAgentRuntime::new();
        let summary = rt.health_summary();
        assert_eq!(summary.total_agents, 0);
        assert_eq!(summary.pending_tasks, 0);
    }

    #[test]
    fn full_workflow() {
        let rt = MultiAgentRuntime::new();

        // Register agents
        let mut agent = AgentInfo::new("worker-1", "Code Worker", "coder");
        agent.capabilities.push(AgentCapability {
            name: "coding".into(),
            description: "Writes code".into(),
            proficiency: 0.9,
        });
        assert!(rt.supervisor.register_agent(agent).is_ok());

        // Submit task
        let task = Task::new("task-1", "Write unit tests", "coding", "user")
            .with_priority(TaskPriority::High);
        rt.task_queue.submit(task);

        // Agent claims task
        let claimed = rt.task_queue.claim("worker-1", "coding");
        assert!(claimed.is_some());
        let task = claimed.unwrap();

        // Track in registry
        assert!(rt.registry.assign_task("worker-1", &task.id));

        // Write shared state
        rt.blackboard.inner().write(
            "progress",
            serde_json::json!({"step": 1}),
            "worker-1",
            "project",
        );

        // Complete task
        rt.task_queue.complete(&task.id, "Tests written");
        rt.registry.complete_task("worker-1", true);

        let summary = rt.health_summary();
        assert_eq!(summary.total_agents, 1);
        assert_eq!(summary.healthy_agents, 1);
        assert_eq!(summary.running_tasks, 0);
        assert_eq!(summary.blackboard_entries, 1);
    }

    #[test]
    fn maintenance_cycle() {
        let rt = MultiAgentRuntime::new();
        let report = rt.maintenance();
        assert_eq!(report.supervisor_events_count, 0);
        assert_eq!(report.expired_tasks, 0);
    }

    #[test]
    fn global_runtime_init() {
        let rt = init_global_runtime();
        assert!(global_runtime().is_some());
        let summary = rt.health_summary();
        assert_eq!(summary.total_agents, 0);
    }

    #[test]
    fn coordinator_integration() {
        let rt = MultiAgentRuntime::new();

        // Test lock acquisition
        let result = rt
            .coordinator
            .locks()
            .acquire("resource-1", "agent-1", "editing");
        assert!(matches!(
            result,
            crate::agent::coordination::LockResult::Acquired
        ));

        // Test release (returns bool, not LockResult)
        let released = rt.coordinator.locks().release("resource-1", "agent-1");
        assert!(released);
    }
}
