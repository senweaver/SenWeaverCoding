// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use parking_lot::RwLock;

use crate::agent::health_signal::{HealthBroadcaster, HealthSignal};
use crate::agent::registry::AgentRegistryHandle;
use crate::error::{RegistryError, SenError};

pub type HealthPenaltyMap = Arc<RwLock<HashMap<(String, String), f64>>>;

pub const SUCCESS_WINDOW_CAP: usize = 500;

#[derive(Debug, Clone, Default)]
pub struct CapabilityGroups {
    groups: HashMap<String, HashSet<String>>,
}

impl CapabilityGroups {

    pub fn defaults() -> Self {
        let mut groups: HashMap<String, HashSet<String>> = HashMap::new();
        groups.insert(
            "code_modification".into(),
            ["file_edit", "patch_apply", "multi_edit", "glob_edit"]
                .into_iter()
                .map(String::from)
                .collect(),
        );
        groups.insert(
            "rust_refactor".into(),
            [
                "rust_edit",
                "rust_analyze",
                "rust_format",
                "rust_lint",
                "code_modification",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        );
        groups.insert(
            "web_search".into(),
            ["web_search", "url_fetch", "html_extract"]
                .into_iter()
                .map(String::from)
                .collect(),
        );
        groups.insert(
            "shell_exec".into(),
            ["shell", "run_command", "pwsh", "bash"]
                .into_iter()
                .map(String::from)
                .collect(),
        );
        Self { groups }
    }

    pub fn with_group<I, S>(mut self, name: impl Into<String>, members: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let name = name.into();
        let set: HashSet<String> = members.into_iter().map(Into::into).collect();
        self.groups.insert(name, set);
        self
    }

    pub fn members(&self, group: &str) -> Option<&HashSet<String>> {
        self.groups.get(group)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &HashSet<String>)> {
        self.groups.iter()
    }
}

#[derive(Debug, Clone)]
pub struct RoutingDecision {

    pub agent_id: String,

    pub confidence: f64,

    pub reason: String,

    pub estimated_load: LoadEstimate,

    pub all_candidates: Vec<CandidateScore>,
}

#[derive(Debug, Clone)]
pub struct CandidateScore {
    pub agent_id: String,
    pub score: f64,
    pub capability_match: f64,
    pub load_score: f64,
    pub affinity_score: f64,

    pub success_rate: f64,

    pub health_multiplier: f64,
}

#[derive(Debug, Clone, Default)]
pub struct LoadEstimate {
    pub active_tasks: usize,
    pub max_tasks: usize,
    pub queue_depth: usize,
}

impl LoadEstimate {

    pub fn ratio(&self) -> f64 {
        if self.max_tasks == 0 {
            return 0.0;
        }
        (self.active_tasks as f64 + self.queue_depth as f64 * 0.5) / self.max_tasks as f64
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub enum RoutingStrategy {

    CapabilityFirst,

    LeastLoad,

    #[default]
    Balanced,

    AffinityFirst,
}

#[derive(Debug, Clone)]
pub struct TaskRouterConfig {
    pub strategy: RoutingStrategy,

    pub min_confidence: f64,

    pub capability_weight: f64,

    pub load_weight: f64,

    pub affinity_weight: f64,

    pub success_rate_weight: f64,
}

impl Default for TaskRouterConfig {
    fn default() -> Self {
        Self {
            strategy: RoutingStrategy::Balanced,
            min_confidence: 0.3,
            capability_weight: 0.5,
            load_weight: 0.3,
            affinity_weight: 0.2,
            success_rate_weight: 0.15,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Task {

    pub id: String,

    pub required_tools: Vec<String>,

    pub preferred_agent: Option<String>,

    pub priority: f64,

    pub context_key: Option<String>,
}

impl Task {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            required_tools: Vec::new(),
            preferred_agent: None,
            priority: 0.5,
            context_key: None,
        }
    }

    pub fn with_tools(mut self, tools: Vec<String>) -> Self {
        self.required_tools = tools;
        self
    }

    pub fn with_affinity(mut self, agent_id: String) -> Self {
        self.preferred_agent = Some(agent_id);
        self
    }
}

pub struct TaskRouter {
    registry: AgentRegistryHandle,
    config: TaskRouterConfig,

    health_penalties: HealthPenaltyMap,

    success_windows: Arc<RwLock<HashMap<String, VecDeque<bool>>>>,

    capability_groups: CapabilityGroups,

    health_subscriber_started: std::sync::atomic::AtomicBool,
}

impl TaskRouter {
    pub fn new(registry: AgentRegistryHandle, config: TaskRouterConfig) -> Self {
        Self {
            registry,
            config,
            health_penalties: Arc::new(RwLock::new(HashMap::new())),
            success_windows: Arc::new(RwLock::new(HashMap::new())),
            capability_groups: CapabilityGroups::defaults(),
            health_subscriber_started: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn with_capability_groups(mut self, groups: CapabilityGroups) -> Self {
        self.capability_groups = groups;
        self
    }

    pub fn health_penalty_for(&self, provider: &str, model: &str) -> f64 {
        self.health_penalties
            .read()
            .get(&(provider.to_string(), model.to_string()))
            .copied()
            .unwrap_or(0.0)
    }

    pub fn health_penalties(&self) -> HealthPenaltyMap {
        self.health_penalties.clone()
    }

    pub fn record_outcome(&self, agent_id: &str, success: bool) {
        let mut map = self.success_windows.write();
        let entry = map.entry(agent_id.to_string()).or_default();
        if entry.len() >= SUCCESS_WINDOW_CAP {
            entry.pop_front();
        }
        entry.push_back(success);
    }

    pub fn past_success_rate(&self, agent_id: &str) -> f64 {
        let map = self.success_windows.read();
        let Some(window) = map.get(agent_id) else {
            return 1.0;
        };
        if window.is_empty() {
            return 1.0;
        }
        let succ = window.iter().filter(|b| **b).count() as f64;
        succ / window.len() as f64
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
        let map = self.health_penalties.clone();
        let mut rx = broadcaster.subscribe();
        Some(
            crate::runtime::spawn_supervised("task_router.health_subscriber", async move {
                while let Ok(signal) = rx.recv().await {
                    apply_signal(&map, &signal);
                }
            })
            .into_inner(),
        )
    }
}

fn apply_signal(map: &HealthPenaltyMap, signal: &HealthSignal) {
    let mut guard = map.write();
    guard.insert(signal.key(), signal.health_penalty());
}

impl TaskRouter {

    pub async fn route(&self, task: &Task) -> Result<RoutingDecision, SenError> {
        let agents = self.registry.all();

        if agents.is_empty() {
            return Err(SenError::Registry(RegistryError::AgentNotFound(
                "no agents available".into(),
            )));
        }

        let mut candidates: Vec<CandidateScore> = Vec::new();
        let mut best_load: Option<LoadEstimate> = None;

        for agent in &agents {
            let load = self.estimate_load(agent);
            let score = self.score_candidate(task, agent, &load);

            if best_load.is_none()
                || candidates.last().map(|c| c.score).unwrap_or(0.0) < score.score
            {
                best_load = Some(load);
            }
            candidates.push(score);
        }

        candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let best = candidates.first().cloned().ok_or_else(|| {
            SenError::Registry(RegistryError::AgentNotFound("no candidates scored".into()))
        })?;

        if best.score < self.config.min_confidence {
            tracing::warn!(
                "Task {} routed to {} with low confidence {:.2}",
                task.id,
                best.agent_id,
                best.score
            );
        }

        let picked_load = agents
            .iter()
            .find(|a| a.id == best.agent_id)
            .map(|a| self.estimate_load(a))
            .unwrap_or_default();

        let reason = self.build_reason(&best, task);

        if let Err(e) = self.registry.assign_task(&best.agent_id, &task.id) {
            tracing::warn!(
                agent_id = %best.agent_id,
                task_id = %task.id,
                error = %e,
                "router failed to reserve agent load after routing decision"
            );
        }

        crate::event_bus::integration::publish_task_delegation_now(
            "task_router",
            &task.id,
            crate::event_bus::types::TaskDelegationAction::Assigned,
            &reason,
        );
        crate::event_bus::integration::publish_coordination_now(
            &best.agent_id,
            crate::event_bus::types::CoordinationAction::Propose,
            &task.id,
            Some(serde_json::json!({
                "agent_id": best.agent_id,
                "confidence": best.score,
                "candidates": candidates.len(),
            })),
        );

        Ok(RoutingDecision {
            agent_id: best.agent_id.clone(),
            confidence: best.score,
            reason,
            estimated_load: picked_load,
            all_candidates: candidates,
        })
    }

    fn score_candidate(
        &self,
        task: &Task,
        agent: &crate::agent::registry::AgentInfo,
        load: &LoadEstimate,
    ) -> CandidateScore {
        let capability_match = self.score_capability(task, agent);
        let load_score = 1.0 - load.ratio().min(1.0);
        let affinity_score = self.score_affinity(task, agent);
        let success_rate = self.past_success_rate(&agent.id);

        let (cap_w, load_w, aff_w, rate_w) = match self.config.strategy {
            RoutingStrategy::CapabilityFirst => (1.0, 0.0, 0.0, 0.0),
            RoutingStrategy::LeastLoad => (0.0, 1.0, 0.0, 0.0),
            RoutingStrategy::AffinityFirst => (0.0, 0.0, 1.0, 0.0),
            RoutingStrategy::Balanced => {
                let sum = self.config.capability_weight
                    + self.config.load_weight
                    + self.config.affinity_weight
                    + self.config.success_rate_weight;
                let norm = if sum > 0.0 { sum } else { 1.0 };
                (
                    self.config.capability_weight / norm,
                    self.config.load_weight / norm,
                    self.config.affinity_weight / norm,
                    self.config.success_rate_weight / norm,
                )
            }
        };

        let base = (cap_w * capability_match)
            + (load_w * load_score)
            + (aff_w * affinity_score)
            + (rate_w * success_rate);

        let (_, model_part) = split_provider_model(&agent.model);
        let penalty = self.health_penalty_for(&agent.provider, &model_part);
        let health_multiplier = (1.0 - penalty).clamp(0.0, 1.0);
        let score = base * health_multiplier;

        CandidateScore {
            agent_id: agent.id.clone(),
            score,
            capability_match,
            load_score,
            affinity_score,
            success_rate,
            health_multiplier,
        }
    }

    fn score_capability(&self, task: &Task, agent: &crate::agent::registry::AgentInfo) -> f64 {
        if task.required_tools.is_empty() {
            return 1.0;
        }

        let agent_tools: HashSet<&String> = agent.capabilities.iter().map(|c| &c.name).collect();
        let required: HashSet<&String> = task.required_tools.iter().collect();

        if required.iter().all(|t| agent_tools.contains(*t)) {
            return 1.0;
        }

        let mut group_hits = 0.0f64;
        let mut group_total = 0.0f64;
        for req in &required {
            if let Some(members) = self.capability_groups.members(req) {
                group_total += 1.0;
                if members.iter().any(|m| agent_tools.contains(m)) {
                    group_hits += 1.0;
                }
            }
        }
        if group_total > 0.0 {
            let hit_ratio = (group_hits / group_total).clamp(0.0, 1.0);

            let group_score = 0.6 + 0.3 * hit_ratio;

            if hit_ratio >= 1.0 {
                return group_score.min(0.9);
            }
        }

        let intersection = agent_tools.intersection(&required).count();
        (intersection as f64 / required.len() as f64) * 0.5
    }

    fn estimate_load(&self, agent: &crate::agent::registry::AgentInfo) -> LoadEstimate {
        LoadEstimate {
            active_tasks: agent.current_load as usize,
            max_tasks: agent.max_concurrency as usize,
            queue_depth: 0,
        }
    }

    fn score_affinity(&self, task: &Task, agent: &crate::agent::registry::AgentInfo) -> f64 {
        if task.preferred_agent.as_ref() == Some(&agent.id) {
            return 1.0;
        }

        let _ = &task.context_key;
        0.0
    }

    fn build_reason(&self, score: &CandidateScore, _task: &Task) -> String {
        if score.capability_match >= 0.9 {
            format!(
                "agent {} matches {:.0}% of required tools (success_rate={:.0}%, health={:.0}%)",
                score.agent_id,
                score.capability_match * 100.0,
                score.success_rate * 100.0,
                score.health_multiplier * 100.0
            )
        } else if score.load_score >= 0.8 {
            format!(
                "agent {} has lowest load ({:.0}% available, success_rate={:.0}%)",
                score.agent_id,
                score.load_score * 100.0,
                score.success_rate * 100.0
            )
        } else {
            format!(
                "agent {} scored {:.2} (cap={:.0}% load={:.0}% affinity={:.0}% success={:.0}% health={:.0}%)",
                score.agent_id,
                score.score,
                score.capability_match * 100.0,
                score.load_score * 100.0,
                score.affinity_score * 100.0,
                score.success_rate * 100.0,
                score.health_multiplier * 100.0
            )
        }
    }
}

fn split_provider_model(model: &str) -> (String, String) {
    if let Some(idx) = model.find('/') {
        return (model[..idx].to_string(), model[idx + 1..].to_string());
    }
    if let Some(idx) = model.find(':') {
        return (model[..idx].to_string(), model[idx + 1..].to_string());
    }
    (String::new(), model.to_string())
}
