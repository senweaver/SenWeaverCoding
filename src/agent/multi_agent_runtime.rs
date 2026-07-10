// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::{Arc, LazyLock};

use parking_lot::RwLock;
use thiserror::Error;
use tracing::{debug, info, warn};

use super::coordination::{Coordinator, CoordinatorHandle};
use super::registry::{AgentRegistry, AgentRegistryHandle};
use super::scheduler::{SchedulableTask, TaskOutcome, TaskScheduler};
use super::scheduler::runtime::{SchedulerSpanContext, TaskExecutor, TaskSchedulerRuntime};
use super::subagent_limiter::{SubagentLimitConfig, SubagentLimiter};
use super::supervisor::{Supervisor, SupervisorConfig, SupervisorHandle};
use super::task_orchestrator::queue::{TaskQueue, TaskQueueHandle};
use super::task_orchestrator::router::{RoutingDecision, Task, TaskRouter, TaskRouterConfig};
use crate::error::SenError;
use crate::memory::blackboard::{Blackboard, BlackboardHandle};

#[derive(Clone)]
pub struct MultiAgentRuntime {
    pub registry: AgentRegistryHandle,
    pub supervisor: SupervisorHandle,
    pub task_queue: TaskQueueHandle,
    pub coordinator: CoordinatorHandle,
    pub blackboard: BlackboardHandle,
    pub task_router: Arc<TaskRouter>,

    pub subagent_limiter: Arc<SubagentLimiter>,
}

impl MultiAgentRuntime {

    pub fn new() -> Self {
        Self::with_config(SupervisorConfig::default())
    }

    pub fn with_config(supervisor_config: SupervisorConfig) -> Self {
        Self::with_config_and_persistence(supervisor_config, None, "default")
    }

    pub fn with_config_and_persistence(
        supervisor_config: SupervisorConfig,
        journal_dir: Option<std::path::PathBuf>,
        session_id: impl AsRef<str>,
    ) -> Self {
        let registry = AgentRegistryHandle::new(AgentRegistry::new());
        let supervisor =
            SupervisorHandle::new(Supervisor::new(supervisor_config, registry.clone()));
        let task_queue = TaskQueueHandle::new(TaskQueue::new());
        let coordinator = CoordinatorHandle::new(Coordinator::new());
        let blackboard = BlackboardHandle::new(Blackboard::with_persistence(
            journal_dir,
            session_id.as_ref(),
        ));
        let task_router = Arc::new(TaskRouter::new(
            registry.clone(),
            TaskRouterConfig::default(),
        ));
        let subagent_limiter = Arc::new(SubagentLimiter::new(
            &SubagentLimitConfig::default(),
        ));

        if tokio::runtime::Handle::try_current().is_ok() {
            if let Some(svc) = crate::services::try_get_services() {
                let _ = supervisor
                    .inner()
                    .spawn_health_subscriber(&svc.health_broadcaster);
                let _ = task_router.spawn_health_subscriber(&svc.health_broadcaster);
                debug!("Multi-agent runtime: provider health subscribers attached");
            } else {
                debug!(
                    "Multi-agent runtime: services not initialized; provider health subscribers not attached"
                );
            }
            Self::spawn_event_registry_bridge(registry.clone());
            coordinator.spawn_event_subscriber();
        }

        info!("Multi-agent runtime initialized");

        Self {
            registry,
            supervisor,
            task_queue,
            coordinator,
            blackboard,
            task_router,
            subagent_limiter,
        }
    }

    fn spawn_event_registry_bridge(registry: AgentRegistryHandle) {
        crate::runtime::task_manager::spawn_supervised(
            "multi_agent.event_registry_bridge",
            async move {
                let mut rx = loop {
                    if let Some(bus) = crate::event_bus::integration::global_bus() {
                        break bus.subscribe_all();
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                };

                loop {
                    let event = match rx.recv().await {
                        Ok(event) => event,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                            debug!(
                                skipped,
                                "event registry bridge lagged behind event bus; continuing"
                            );
                            continue;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            match crate::event_bus::integration::global_bus() {
                                Some(bus) => {
                                    rx = bus.subscribe_all();
                                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                    continue;
                                }
                                None => break,
                            }
                        }
                    };

                    match &event.payload {
                        crate::event_bus::types::EventPayload::AgentRequest {
                            request_id, ..
                        } => {
                            if let crate::event_bus::types::EventTarget::Agent(agent_id) =
                                &event.target
                                && let Err(e) = registry.assign_task(agent_id, request_id)
                            {
                                warn!(
                                    agent_id = %agent_id,
                                    request_id = %request_id,
                                    error = %e,
                                    "event bridge failed to assign task to agent"
                                );
                            }
                        }
                        crate::event_bus::types::EventPayload::AgentResponse {
                            success, ..
                        } => {
                            registry.complete_task(&event.source, *success);
                        }
                        crate::event_bus::types::EventPayload::TaskDelegation { .. }
                        | crate::event_bus::types::EventPayload::Coordination { .. } => {
                            let _ = registry.heartbeat(&event.source);
                        }
                        _ => {}
                    }
                }
            },
        );
    }

    pub fn spawn_task_worker(
        &self,
        capabilities: Vec<String>,
        poll_interval: std::time::Duration,
        executor: super::task_orchestrator::worker::TaskWorkerExecutor,
    ) -> crate::runtime::task_manager::TaskHandle {
        let worker = super::task_orchestrator::worker::TaskQueueWorker::new(
            Arc::clone(self.task_queue.inner_arc()),
            capabilities,
            executor,
        )
        .with_blackboard(self.blackboard.clone())
        .with_poll_interval(poll_interval);

        info!(agent_id = %worker.agent_id(), "spawning task queue worker");
        worker.spawn()
    }

    pub fn cancel_subtree(&self, agent_id: &str) -> usize {
        self.subagent_limiter.cancel_descendants(agent_id)
    }

    pub fn cancel_subtree_inclusive(&self, agent_id: &str) -> usize {
        self.subagent_limiter.cancel_subtree(agent_id)
    }

    pub async fn route_task(&self, task: Task) -> Result<RoutingDecision, String> {
        match self.task_router.route(&task).await {
            Ok(decision) => {
                tracing::debug!(
                    agent_id = %decision.agent_id,
                    confidence = decision.confidence,
                    "Task {} routed to agent",
                    task.id
                );
                Ok(decision)
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Task routing failed for task {}",
                    task.id
                );
                Err(format!("Task routing failed: {}", e))
            }
        }
    }

    pub const STALE_RUNNING_TASK_MAX: std::time::Duration =
        std::time::Duration::from_secs(30 * 60);

    pub fn maintenance(&self) -> MaintenanceReport {
        let supervisor_events = self.supervisor.health_check();
        let expired_tasks = self.task_queue.inner().expire_overdue();
        let reclaimed_tasks = self
            .task_queue
            .inner()
            .reclaim_stale_running(Self::STALE_RUNNING_TASK_MAX);
        let expired_entries = self.blackboard.inner().evict_expired();
        let (expired_locks, expired_barriers, expired_votes) = self.coordinator.maintenance();

        if !supervisor_events.is_empty()
            || expired_tasks > 0
            || reclaimed_tasks > 0
            || expired_entries > 0
            || expired_locks > 0
        {
            debug!(
                supervisor_events = supervisor_events.len(),
                expired_tasks,
                reclaimed_tasks,
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
            reclaimed_tasks,
            expired_entries,
            expired_locks,
            expired_barriers,
            expired_votes,
        }
    }

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

    pub fn shutdown(&self) {
        info!("Multi-agent runtime shutting down");
        self.supervisor.shutdown_all();
    }

    pub async fn run_parallel<F, Fut>(
        &self,
        tasks: Vec<F>,
        strategy: crate::agent::parallel_executor::AggregationStrategy,
        max_concurrent: usize,
        timeout_secs: Option<u64>,
    ) -> Result<Vec<crate::agent::parallel_executor::TaskOutput>, SenError>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = crate::agent::parallel_executor::TaskOutput>
            + Send
            + 'static,
    {
        use crate::agent::parallel_executor::{ExecutorConfig, ParallelExecutor, Priority};

        if tasks.is_empty() {
            return Ok(Vec::new());
        }

        let executor = ParallelExecutor::new(ExecutorConfig {
            max_concurrent: max_concurrent.max(1),
            queue_capacity: tasks.len().max(8) * 2,
            default_timeout_secs: timeout_secs,
            fairness_enabled: true,
        });

        let mut handles = Vec::with_capacity(tasks.len());
        for task in tasks {
            let h = executor
                .submit(task, Priority::NORMAL, timeout_secs)
                .await?;
            handles.push(h);
        }

        let agg = executor
            .aggregate_results(handles, strategy, timeout_secs)
            .await?;
        Ok(agg.succeeded)
    }

    pub async fn submit_task_graph(
        &self,
        tasks: Vec<SchedulableTask>,
        max_parallel: usize,
        executor: TaskExecutor,
    ) -> Result<Vec<TaskOutcome>, String> {
        self.submit_task_graph_with_context(tasks, max_parallel, executor, None)
            .await
    }

    pub async fn submit_task_graph_with_context(
        &self,
        tasks: Vec<SchedulableTask>,
        max_parallel: usize,
        executor: TaskExecutor,
        parent_agent_id: Option<String>,
    ) -> Result<Vec<TaskOutcome>, String> {
        if tasks.is_empty() {
            return Err("submit_task_graph: task list is empty".into());
        }

        let delegation_id = format!(
            "deleg-{}-{}",
            chrono::Utc::now().timestamp_millis(),
            &uuid::Uuid::new_v4().to_string()[..8]
        );

        let mut scheduler = TaskScheduler::new(max_parallel.max(1));
        scheduler.add_tasks(tasks.clone())?;

        for t in &tasks {
            self.blackboard.inner().write(
                format!("delegation/{}", t.id),
                serde_json::json!({
                    "task_id": &t.id,
                    "delegation_id": &delegation_id,
                    "parent_agent_id": parent_agent_id.as_deref(),
                    "status": "queued",
                    "capability": &t.required_capability,
                    "description": &t.description,
                    "depends_on": &t.depends_on,
                    "submitted_at": chrono::Utc::now().to_rfc3339(),
                }),
                "multi_agent_runtime",
                "delegations",
            );
        }

        let mut span_ctx = SchedulerSpanContext::new().with_delegation(delegation_id.clone());
        if let Some(pid) = parent_agent_id.as_ref() {
            span_ctx = span_ctx.with_parent_agent(pid.clone());
        }

        let runtime = TaskSchedulerRuntime::new(scheduler);
        let outcomes = runtime.run_with_context(executor, span_ctx).await;

        for outcome in &outcomes {
            self.blackboard.inner().write(
                format!("result/{}", outcome.task_id),
                serde_json::json!({
                    "task_id": &outcome.task_id,
                    "delegation_id": &delegation_id,
                    "parent_agent_id": parent_agent_id.as_deref(),
                    "assigned_agent": outcome.assigned_agent.as_deref(),
                    "status": if outcome.success { "completed" } else { "failed" },
                    "result_preview": outcome.result.chars().take(200).collect::<String>(),
                    "completed_at": chrono::Utc::now().to_rfc3339(),
                }),
                "multi_agent_runtime",
                "task_results",
            );

            if let Some(agent_id) = outcome.assigned_agent.as_deref() {
                self.task_router.record_outcome(agent_id, outcome.success);
            }
        }

        Ok(outcomes)
    }
}

impl Default for MultiAgentRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Default)]
pub struct MaintenanceReport {
    pub supervisor_events_count: usize,
    pub expired_tasks: usize,
    pub reclaimed_tasks: usize,
    pub expired_entries: usize,
    pub expired_locks: usize,
    pub expired_barriers: usize,
    pub expired_votes: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RuntimeHealthSummary {
    pub total_agents: usize,
    pub healthy_agents: usize,
    pub unhealthy_agents: usize,
    pub pending_tasks: usize,
    pub running_tasks: usize,
    pub blackboard_entries: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MultiAgentRuntimeConfig {
    pub supervisor_config: SupervisorConfig,

    pub sub_agent_identities: Vec<(String, String)>,

    #[serde(default)]
    pub allow_shared_identity: bool,
}

impl Default for MultiAgentRuntimeConfig {
    fn default() -> Self {
        Self {
            supervisor_config: SupervisorConfig::default(),
            sub_agent_identities: Vec::new(),
            allow_shared_identity: false,
        }
    }
}

#[derive(Debug, Error)]
pub enum MultiAgentRuntimeManagerError {
    #[error("runtime is not initialized")]
    NotInitialized,

    #[error("runtime is already initialized")]
    AlreadyInitialized,

    #[error("runtime shutdown failed: {0}")]
    ShutdownFailed(String),

    #[error(
        "sub-agents '{agent_a}' and '{agent_b}' share CallerIdentity user_id '{user_id}'; \
         per-agent RBAC workspace isolation cannot be enforced. Assign distinct identities, \
         or set allow_shared_identity=true in the multi-agent runtime config to permit this explicitly"
    )]
    SharedIdentityConflict {
        agent_a: String,
        agent_b: String,
        user_id: String,
    },
}

pub struct MultiAgentRuntimeManager {
    runtime: RwLock<Option<Arc<MultiAgentRuntime>>>,
    config: RwLock<Option<MultiAgentRuntimeConfig>>,
}

impl MultiAgentRuntimeManager {

    pub fn new() -> Self {
        Self {
            runtime: RwLock::new(None),
            config: RwLock::new(None),
        }
    }

    pub fn get_or_init(&self, config: MultiAgentRuntimeConfig) -> Arc<MultiAgentRuntime> {

        if let Some(runtime) = self.runtime.read().as_ref() {
            let runtime = runtime.clone();
            self.merge_config(config);
            return runtime;
        }

        let mut guard = self.runtime.write();

        if let Some(runtime) = guard.as_ref() {
            let runtime = runtime.clone();
            drop(guard);
            self.merge_config(config);
            return runtime;
        }

        let runtime = Arc::new(MultiAgentRuntime::with_config(
            config.supervisor_config.clone(),
        ));
        *guard = Some(runtime.clone());

        *self.config.write() = Some(config);

        info!("Multi-agent runtime initialized via manager");
        runtime
    }

    fn merge_config(&self, incoming: MultiAgentRuntimeConfig) {
        let mut stored = self.config.write();
        match stored.as_mut() {
            Some(existing) => {
                let mut changed = false;
                for (agent_id, caller_user_id) in incoming.sub_agent_identities {
                    if let Some(slot) = existing
                        .sub_agent_identities
                        .iter_mut()
                        .find(|(id, _)| *id == agent_id)
                    {
                        if slot.1 != caller_user_id {
                            slot.1 = caller_user_id;
                            changed = true;
                        }
                    } else {
                        existing.sub_agent_identities.push((agent_id, caller_user_id));
                        changed = true;
                    }
                }
                if incoming.allow_shared_identity {
                    existing.allow_shared_identity = true;
                }
                if changed {
                    info!("Multi-agent runtime config updated with new sub-agent identities");
                }
            }
            None => {
                *stored = Some(incoming);
            }
        }
    }

    pub fn get(&self) -> Option<Arc<MultiAgentRuntime>> {
        self.runtime.read().clone()
    }

    pub fn shutdown(&self) -> Result<(), SenError> {
        let runtime = self.runtime.write().take();
        match runtime {
            Some(rt) => {
                rt.shutdown();
                *self.config.write() = None;
                info!("Multi-agent runtime shut down via manager");
                Ok(())
            }
            None => Err(SenError::Agent(crate::error::AgentError::from(
                "runtime not initialized",
            ))),
        }
    }

    pub fn is_initialized(&self) -> bool {
        self.runtime.read().is_some()
    }

    pub fn config(&self) -> Option<MultiAgentRuntimeConfig> {
        self.config.read().clone()
    }
}

impl Default for MultiAgentRuntimeManager {
    fn default() -> Self {
        Self::new()
    }
}

impl From<MultiAgentRuntimeManagerError> for SenError {
    fn from(err: MultiAgentRuntimeManagerError) -> Self {
        SenError::Agent(crate::error::AgentError::from(err.to_string()))
    }
}

#[derive(Clone)]
pub struct MultiAgentRuntimeHandle {
    inner: Arc<MultiAgentRuntime>,
}

impl MultiAgentRuntimeHandle {

    pub fn new(rt: Arc<MultiAgentRuntime>) -> Self {
        Self { inner: rt }
    }

    pub fn runtime(&self) -> &Arc<MultiAgentRuntime> {
        &self.inner
    }

    pub fn into_inner(self) -> Arc<MultiAgentRuntime> {
        self.inner
    }
}

impl std::ops::Deref for MultiAgentRuntimeHandle {
    type Target = MultiAgentRuntime;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl From<Arc<MultiAgentRuntime>> for MultiAgentRuntimeHandle {
    fn from(rt: Arc<MultiAgentRuntime>) -> Self {
        Self::new(rt)
    }
}

pub struct MultiAgentRuntimeBuilder {
    config: MultiAgentRuntimeConfig,
}

impl Default for MultiAgentRuntimeBuilder {
    fn default() -> Self {
        Self {
            config: MultiAgentRuntimeConfig::default(),
        }
    }
}

impl MultiAgentRuntimeBuilder {

    pub fn with_config(mut self, cfg: MultiAgentRuntimeConfig) -> Self {
        self.config = cfg;
        self
    }

    pub fn build(self) -> Arc<MultiAgentRuntime> {
        Arc::new(MultiAgentRuntime::with_config(
            self.config.supervisor_config,
        ))
    }

    pub fn build_handle(self) -> MultiAgentRuntimeHandle {
        MultiAgentRuntimeHandle::new(self.build())
    }
}

impl MultiAgentRuntime {

    pub fn builder() -> MultiAgentRuntimeBuilder {
        MultiAgentRuntimeBuilder::default()
    }
}

static MANAGER: LazyLock<MultiAgentRuntimeManager> = LazyLock::new(MultiAgentRuntimeManager::new);

pub fn init_global_runtime() -> Arc<MultiAgentRuntime> {
    match init_global_runtime_with_config(MultiAgentRuntimeConfig::default()) {
        Ok(runtime) => runtime,
        Err(e) => {
            tracing::error!(
                error = %e,
                "global multi-agent runtime init rejected the configured identities; \
                 starting with a safe single-identity default instead"
            );
            MANAGER.get_or_init(MultiAgentRuntimeConfig::default())
        }
    }
}

pub fn init_global_runtime_with_config(
    config: MultiAgentRuntimeConfig,
) -> Result<Arc<MultiAgentRuntime>, MultiAgentRuntimeManagerError> {

    if config.sub_agent_identities.len() > 1 && !config.allow_shared_identity {
        let mut seen: std::collections::HashMap<&str, &str> =
            std::collections::HashMap::with_capacity(config.sub_agent_identities.len());
        for (agent_id, caller_user_id) in &config.sub_agent_identities {
            if let Some(prior_agent) = seen.insert(caller_user_id.as_str(), agent_id.as_str()) {
                tracing::error!(
                    agent_id = %agent_id,
                    conflicting_agent_id = %prior_agent,
                    caller_user_id = %caller_user_id,
                    "Multi-agent runtime: sub-agents share the same CallerIdentity \
                     user_id  -  refusing to initialize. Assign each sub-agent a distinct \
                     CallerIdentity, or set allow_shared_identity=true to permit explicitly."
                );
                return Err(MultiAgentRuntimeManagerError::SharedIdentityConflict {
                    agent_a: prior_agent.to_string(),
                    agent_b: agent_id.clone(),
                    user_id: caller_user_id.clone(),
                });
            }
        }
    }

    let runtime = MANAGER.get_or_init(config.clone());
    info!("Global multi-agent runtime initialized");
    Ok(runtime)
}

pub fn global_runtime() -> Option<Arc<MultiAgentRuntime>> {
    MANAGER.get()
}

pub fn session_scoped_key(key: &str) -> String {
    match crate::session::current_session_context() {
        Some(ctx) if !ctx.session_id.is_empty() => format!("{}::{}", ctx.session_id, key),
        _ => format!("__global__::{key}"),
    }
}

pub fn session_scoped_namespace(namespace: &str) -> String {
    match crate::session::current_session_context() {
        Some(ctx) if !ctx.session_id.is_empty() => format!("{namespace}:{}", ctx.session_id),
        _ => namespace.to_string(),
    }
}

pub fn register_configured_agents(rt: &MultiAgentRuntime, config: &crate::config::Config) {
    use crate::agent::registry::{AgentCapability, AgentInfo};

    if rt.registry.get("primary").is_none() {
        let mut primary = AgentInfo::new("primary", "Primary Agent", "coder");
        primary.capabilities.push(AgentCapability {
            name: "coding".into(),
            description: "Default single-agent session".into(),
            proficiency: 1.0,
        });
        primary.capabilities.push(AgentCapability {
            name: "general".into(),
            description: "General purpose assistant".into(),
            proficiency: 0.9,
        });
        let _ = rt.supervisor.register_agent(primary);
    }

    for (swarm_name, swarm_cfg) in &config.swarms {
        for agent_name in &swarm_cfg.agents {
            let id = format!("{swarm_name}/{agent_name}");
            if rt.registry.get(&id).is_some() {
                continue;
            }
            let mut info = AgentInfo::new(&id, agent_name.as_str(), swarm_name.as_str());
            info.capabilities.push(AgentCapability {
                name: agent_name.clone(),
                description: format!("Swarm member of '{swarm_name}'"),
                proficiency: 0.9,
            });
            info.capabilities.push(AgentCapability {
                name: "general".into(),
                description: "General fallback capability".into(),
                proficiency: 0.6,
            });
            let _ = rt.supervisor.register_agent(info);
        }
    }
}

pub fn global_manager() -> &'static MultiAgentRuntimeManager {
    &MANAGER
}
