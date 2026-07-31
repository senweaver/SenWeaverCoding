// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::agent::scheduler::core::{SchedulableTask, TaskScheduler};
use crate::agent::scheduler::runtime::{
    SchedulerSpanContext, TaskExecutor, TaskSchedulerRuntime,
};
use crate::memory::blackboard::Blackboard;
use crate::providers::Provider;

const DEFAULT_STAGE_TIMEOUT: Duration = Duration::from_secs(180);

pub const PIPELINE_NAMESPACE: &str = "role_pipeline";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleStage {

    pub id: String,

    pub label: String,

    pub system_prompt: String,

    pub depends_on: Vec<String>,

    pub temperature: Option<f64>,

    pub stage_timeout: Option<Duration>,
}

impl RoleStage {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        system_prompt: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            system_prompt: system_prompt.into(),
            depends_on: Vec::new(),
            temperature: None,
            stage_timeout: None,
        }
    }

    pub fn with_dependency(mut self, parent_id: impl Into<String>) -> Self {
        self.depends_on.push(parent_id.into());
        self
    }

    pub fn with_dependencies<I>(mut self, parents: I) -> Self
    where
        I: IntoIterator<Item = String>,
    {
        self.depends_on.extend(parents);
        self
    }

    pub fn with_temperature(mut self, temperature: f64) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.stage_timeout = Some(timeout);
        self
    }
}

#[derive(Debug, Clone)]
pub struct PipelineParams {

    pub run_id: Option<String>,

    pub provider_name: String,

    pub model: String,

    pub temperature: f64,

    pub stage_timeout: Duration,
}

impl Default for PipelineParams {
    fn default() -> Self {
        Self {
            run_id: None,
            provider_name: "openrouter".to_string(),
            model: String::new(),
            temperature: 0.2,
            stage_timeout: DEFAULT_STAGE_TIMEOUT,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StageOutcome {
    pub stage_id: String,
    pub label: String,
    pub success: bool,
    pub elapsed_ms: u128,
    pub answer: String,

    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PipelineReport {
    pub run_id: String,

    pub stages: Vec<StageOutcome>,

    pub final_answer: String,
}

#[derive(Debug, Clone)]
pub struct RolePipeline {
    pub name: String,
    pub stages: Vec<RoleStage>,
}

impl RolePipeline {
    pub fn new(name: impl Into<String>, stages: Vec<RoleStage>) -> Self {
        Self {
            name: name.into(),
            stages,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut ids: HashSet<&str> = HashSet::new();
        for stage in &self.stages {
            if !ids.insert(stage.id.as_str()) {
                return Err(format!("duplicate stage id `{}`", stage.id));
            }
        }
        for stage in &self.stages {
            for dep in &stage.depends_on {
                if !ids.contains(dep.as_str()) {
                    return Err(format!(
                        "stage `{}` depends on unknown stage `{}`",
                        stage.id, dep
                    ));
                }
            }
        }
        if self.topological_levels().is_none() {
            return Err("pipeline contains a dependency cycle".to_string());
        }
        Ok(())
    }

    pub fn topological_levels(&self) -> Option<Vec<Vec<RoleStage>>> {
        let mut remaining: Vec<RoleStage> = self.stages.clone();
        let mut completed: HashSet<String> = HashSet::new();
        let mut levels: Vec<Vec<RoleStage>> = Vec::new();

        while !remaining.is_empty() {
            let mut ready: Vec<RoleStage> = Vec::new();
            let mut still_pending: Vec<RoleStage> = Vec::new();
            for stage in remaining {
                if stage.depends_on.iter().all(|d| completed.contains(d)) {
                    ready.push(stage);
                } else {
                    still_pending.push(stage);
                }
            }
            if ready.is_empty() {

                return None;
            }
            for stage in &ready {
                completed.insert(stage.id.clone());
            }
            levels.push(ready);
            remaining = still_pending;
        }
        Some(levels)
    }

    pub async fn run(
        &self,
        goal: &str,
        provider: Arc<dyn Provider>,
        blackboard: Arc<Blackboard>,
        params: PipelineParams,
    ) -> Result<PipelineReport, String> {
        self.validate()?;

        let run_id = params
            .run_id
            .clone()
            .unwrap_or_else(|| format!("team-{}", uuid::Uuid::new_v4()));
        let default_temp = params.temperature;
        let default_timeout = params.stage_timeout;

        info!(
            target: "agent.role_pipeline",
            pipeline = %self.name,
            run_id = %run_id,
            stages = self.stages.len(),
            "role-pipeline run starting"
        );

        let mut has_child: HashSet<String> = HashSet::new();
        for stage in &self.stages {
            for dep in &stage.depends_on {
                has_child.insert(dep.clone());
            }
        }

        let deps_by_id: HashMap<&str, &[String]> = self
            .stages
            .iter()
            .map(|s| (s.id.as_str(), s.depends_on.as_slice()))
            .collect();
        let declaration_order: Vec<String> =
            self.stages.iter().map(|s| s.id.clone()).collect();
        let ancestors_by_id: Arc<HashMap<String, Vec<String>>> = Arc::new(
            self.stages
                .iter()
                .map(|stage| {
                    let mut seen: HashSet<&str> = HashSet::new();
                    let mut frontier: Vec<&str> =
                        stage.depends_on.iter().map(String::as_str).collect();
                    while let Some(id) = frontier.pop() {
                        if seen.insert(id) {
                            if let Some(parents) = deps_by_id.get(id) {
                                frontier.extend(parents.iter().map(String::as_str));
                            }
                        }
                    }
                    let ordered: Vec<String> = declaration_order
                        .iter()
                        .filter(|id| seen.contains(id.as_str()))
                        .cloned()
                        .collect();
                    (stage.id.clone(), ordered)
                })
                .collect(),
        );

        let mut tasks: Vec<SchedulableTask> = Vec::with_capacity(self.stages.len());
        for stage in &self.stages {
            let mut task =
                SchedulableTask::new(stage.id.clone(), stage.label.clone(), String::new());
            for dep in &stage.depends_on {
                task = task.with_dependency(dep.clone());
            }
            tasks.push(task);
        }

        let stage_by_id: Arc<HashMap<String, RoleStage>> = Arc::new(
            self.stages
                .iter()
                .map(|s| (s.id.clone(), s.clone()))
                .collect(),
        );
        let artifacts: Arc<parking_lot::Mutex<HashMap<String, String>>> =
            Arc::new(parking_lot::Mutex::new(HashMap::new()));
        let executed: Arc<parking_lot::Mutex<Vec<StageOutcome>>> =
            Arc::new(parking_lot::Mutex::new(Vec::new()));

        let executor: TaskExecutor = {
            let goal = goal.to_string();
            let model = params.model.clone();
            let pipeline_name = self.name.clone();
            let run_id = run_id.clone();
            let blackboard = blackboard.clone();
            let stage_by_id = stage_by_id.clone();
            let ancestors_by_id = ancestors_by_id.clone();
            let artifacts = artifacts.clone();
            let executed = executed.clone();
            Arc::new(move |task, cancel| {
                let Some(stage) = stage_by_id.get(task.id.as_str()).cloned() else {
                    let missing = task.id.clone();
                    return Box::pin(async move {
                        Err(format!("unknown pipeline stage `{missing}`"))
                    });
                };
                let goal = goal.clone();
                let model = model.clone();
                let pipeline_name = pipeline_name.clone();
                let run_id = run_id.clone();
                let blackboard = blackboard.clone();
                let artifacts = artifacts.clone();
                let executed = executed.clone();
                let provider = provider.clone();
                let ancestor_ids = ancestors_by_id
                    .get(task.id.as_str())
                    .cloned()
                    .unwrap_or_default();
                Box::pin(async move {
                    let timeout = stage.stage_timeout.unwrap_or(default_timeout);
                    let temperature = stage.temperature.unwrap_or(default_temp);
                    let prompt = {
                        let snapshot = artifacts.lock();
                        build_stage_prompt(&goal, &stage, &ancestor_ids, &snapshot)
                    };
                    let started = std::time::Instant::now();
                    let chat_fut = provider.chat_with_system(
                        Some(&stage.system_prompt),
                        &prompt,
                        &model,
                        temperature,
                    );
                    let bounded = tokio::time::timeout(timeout, chat_fut);
                    let result = tokio::select! {
                        biased;
                        () = cancel.cancelled() => {
                            return Err("stage cancelled".to_string());
                        }
                        result = bounded => result,
                    };
                    let elapsed_ms = started.elapsed().as_millis();
                    let outcome = match result {
                        Ok(Ok(answer)) => StageOutcome {
                            stage_id: stage.id.clone(),
                            label: stage.label.clone(),
                            success: true,
                            elapsed_ms,
                            answer,
                            error: None,
                        },
                        Ok(Err(e)) => StageOutcome {
                            stage_id: stage.id.clone(),
                            label: stage.label.clone(),
                            success: false,
                            elapsed_ms,
                            answer: String::new(),
                            error: Some(format!("provider error: {e}")),
                        },
                        Err(_) => StageOutcome {
                            stage_id: stage.id.clone(),
                            label: stage.label.clone(),
                            success: false,
                            elapsed_ms,
                            answer: String::new(),
                            error: Some(format!("stage timed out after {timeout:?}")),
                        },
                    };

                    blackboard.write(
                        format!("{}/{}/{}", PIPELINE_NAMESPACE, run_id, outcome.stage_id),
                        serde_json::json!({
                            "run_id": &run_id,
                            "pipeline": &pipeline_name,
                            "stage_id": &outcome.stage_id,
                            "label": &outcome.label,
                            "success": outcome.success,
                            "elapsed_ms": outcome.elapsed_ms,
                            "answer": &outcome.answer,
                            "error": &outcome.error,
                        }),
                        format!("role_pipeline:{}", outcome.stage_id),
                        PIPELINE_NAMESPACE,
                    );

                    if outcome.success {
                        artifacts
                            .lock()
                            .insert(outcome.stage_id.clone(), outcome.answer.clone());
                    } else {
                        warn!(
                            target: "agent.role_pipeline",
                            run_id = %run_id,
                            stage = %outcome.stage_id,
                            error = ?outcome.error,
                            "role-pipeline stage failed"
                        );
                    }

                    let success = outcome.success;
                    let answer = outcome.answer.clone();
                    let error = outcome.error.clone();
                    executed.lock().push(outcome);
                    if success {
                        Ok(answer)
                    } else {
                        Err(error.unwrap_or_else(|| "stage failed".to_string()))
                    }
                })
            })
        };

        let mut scheduler = TaskScheduler::new(self.stages.len().max(1));
        scheduler.add_tasks(tasks)?;
        let runtime = TaskSchedulerRuntime::new(scheduler);
        let cancel_bridge = crate::providers::current_session_cancel_token().map(|parent| {
            let scheduler_token = runtime.cancellation_token();
            crate::runtime::spawn_supervised("role_pipeline.cancel_bridge", async move {
                parent.cancelled().await;
                scheduler_token.cancel();
            })
        });
        let scheduler_outcomes = runtime
            .run_with_context(
                executor,
                SchedulerSpanContext::new().with_delegation(run_id.clone()),
            )
            .await;
        if let Some(bridge) = cancel_bridge {
            bridge.abort();
        }

        let mut executed_by_id: HashMap<String, StageOutcome> = executed
            .lock()
            .drain(..)
            .map(|o| (o.stage_id.clone(), o))
            .collect();
        let scheduler_errors: HashMap<String, String> = scheduler_outcomes
            .into_iter()
            .filter(|o| !o.success)
            .map(|o| (o.task_id, o.result))
            .collect();

        let mut outcomes: Vec<StageOutcome> = Vec::with_capacity(self.stages.len());
        for stage in &self.stages {
            match executed_by_id.remove(&stage.id) {
                Some(outcome) => outcomes.push(outcome),
                None => outcomes.push(StageOutcome {
                    stage_id: stage.id.clone(),
                    label: stage.label.clone(),
                    success: false,
                    elapsed_ms: 0,
                    answer: String::new(),
                    error: Some(
                        scheduler_errors
                            .get(&stage.id)
                            .filter(|msg| !msg.is_empty())
                            .cloned()
                            .unwrap_or_else(|| {
                                "stage cancelled before execution".to_string()
                            }),
                    ),
                }),
            }
        }

        let mut final_chunks: BTreeMap<usize, String> = BTreeMap::new();
        for (idx, outcome) in outcomes.iter().enumerate() {
            if outcome.success && !has_child.contains(&outcome.stage_id) {
                final_chunks.insert(idx, outcome.answer.clone());
            }
        }
        let final_answer = if final_chunks.is_empty() {
            outcomes
                .iter()
                .rfind(|o| o.success)
                .map(|o| o.answer.clone())
                .unwrap_or_default()
        } else {
            final_chunks
                .into_values()
                .collect::<Vec<_>>()
                .join("\n\n")
        };

        info!(
            target: "agent.role_pipeline",
            pipeline = %self.name,
            run_id = %run_id,
            stages = outcomes.len(),
            "role-pipeline run finished"
        );

        Ok(PipelineReport {
            run_id,
            stages: outcomes,
            final_answer,
        })
    }
}

fn build_stage_prompt(
    goal: &str,
    stage: &RoleStage,
    ancestor_ids: &[String],
    artifacts: &HashMap<String, String>,
) -> String {
    let mut out = String::new();
    out.push_str("## Goal\n");
    out.push_str(goal.trim());
    out.push_str("\n\n");

    if !ancestor_ids.is_empty() {
        out.push_str("## Prior artifacts\n");
        for dep in ancestor_ids {
            let body = artifacts
                .get(dep)
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| "(no output recorded)".to_string());
            out.push_str(&format!("### {dep}\n{body}\n\n"));
        }
    }

    out.push_str(&format!(
        "## Your role: {} ({})\nProduce your stage output now.\n",
        stage.label, stage.id
    ));
    out
}

pub fn default_pipeline() -> RolePipeline {
    let planner = RoleStage::new(
        "planner",
        "Planner",
        "You are the Planner role.  Decompose the user's goal into an \
         ordered numbered plan covering investigation, implementation, \
         and verification.  Be concrete; cite file paths when relevant.  \
         Return only the plan.",
    );

    let coder = RoleStage::new(
        "coder",
        "Coder",
        "You are the Coder role.  Using the planner's plan as a \
         reference, draft the code-level changes (snippets, diffs, \
         module names) needed to implement the goal.  Do not invent \
         APIs that were not mentioned in the plan or the goal.",
    )
    .with_dependency("planner");

    let researcher = RoleStage::new(
        "researcher",
        "Researcher",
        "You are the Researcher role.  Using the planner's plan as a \
         reference, gather domain knowledge: relevant standards, \
         existing libraries, edge cases, and risks the implementer \
         should know about.  Cite sources where possible.",
    )
    .with_dependency("planner");

    let reviewer = RoleStage::new(
        "reviewer",
        "Reviewer",
        "You are the Reviewer role.  Inspect the coder's draft and the \
         researcher's notes against the planner's plan.  Flag missing \
         steps, integration risks, and concrete improvements.  Return \
         a numbered review with severity tags (`critical` / `major` / \
         `minor`).",
    )
    .with_dependencies(vec!["coder".to_string(), "researcher".to_string()])
    .with_temperature(0.1);

    let finalizer = RoleStage::new(
        "final",
        "Finalizer",
        "You are the Finalizer role.  Combine the planner's plan, the \
         coder's draft, the researcher's notes, and the reviewer's \
         feedback into a single final answer for the user.  Resolve \
         contradictions in favour of the reviewer; surface unresolved \
         risks explicitly.",
    )
    .with_dependency("reviewer")
    .with_temperature(0.1);

    RolePipeline::new(
        "default",
        vec![planner, coder, researcher, reviewer, finalizer],
    )
}
