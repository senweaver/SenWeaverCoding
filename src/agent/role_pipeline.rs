// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Role-pipeline DAG runner — first-class multi-role orchestration.
//!
//! This module sits one layer above [`crate::tools::delegate_parallel`]:
//! instead of letting an agent script its own fan-out via tool calls,
//! it lets the operator (CLI / SDK / gateway) describe a *fixed* role
//! graph and run it end-to-end with shared intermediate state.
//!
//! ## Default DAG
//!
//! `default_pipeline` returns the canonical four-role graph documented
//! in the project plan:
//!
//! ```text
//!                    +-----------+
//!                    |  planner  |
//!                    +-----+-----+
//!                          |
//!              +-----------+-----------+
//!              v                       v
//!        +-----------+           +-------------+
//!        |   coder   |           |  researcher |
//!        +-----+-----+           +------+------+
//!              \                       /
//!               +---------+----------+
//!                         v
//!                    +----------+
//!                    | reviewer |
//!                    +----+-----+
//!                         |
//!                         v
//!                    +---------+
//!                    |  final  |
//!                    +---------+
//! ```
//!
//! Stages on the same depth level (here `coder` ∥ `researcher`) execute
//! concurrently inside the same level barrier so the total wall-clock
//! is `depth * max_stage_latency` rather than `Σ stage_latency`.
//!
//! ## Shared state
//!
//! Every stage's textual answer is published to a shared
//! [`crate::memory::blackboard::Blackboard`] under the `role_pipeline`
//! namespace, keyed by `role_pipeline/<run_id>/<stage_id>`.  Dependent
//! stages read those entries when assembling their prompt so artifacts
//! propagate forwards along edges of the DAG without any extra
//! coordination boilerplate.
//!
//! ## Provider model
//!
//! Each stage is executed via [`crate::providers::Provider::chat_with_system`]
//! against the workspace's default provider/model.  This intentionally
//! keeps the first cut tool-free so the pipeline is portable across
//! every backend (OpenAI, Anthropic, OpenRouter, Gemini, OpenAI-compatible,
//! local FIM, …) without bespoke tool wiring.  Future iterations can
//! upgrade individual stages to full
//! [`crate::agent::loop_::run_tool_call_loop`] runs without changing
//! the public DAG surface.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

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
            model: "claude-sonnet-4-20250514".to_string(),
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
            for stage in remaining.into_iter() {
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
        provider: &dyn Provider,
        blackboard: Arc<Blackboard>,
        params: PipelineParams,
    ) -> Result<PipelineReport, String> {
        self.validate()?;

        let levels = self
            .topological_levels()
            .ok_or_else(|| "pipeline contains a dependency cycle".to_string())?;

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
            levels = levels.len(),
            "role-pipeline run starting"
        );

        let mut artifacts: HashMap<String, String> = HashMap::new();
        let mut outcomes: Vec<StageOutcome> = Vec::new();

        let mut has_child: HashSet<String> = HashSet::new();
        for stage in &self.stages {
            for dep in &stage.depends_on {
                has_child.insert(dep.clone());
            }
        }

        for (level_idx, level_stages) in levels.into_iter().enumerate() {
            debug!(
                target: "agent.role_pipeline",
                run_id = %run_id,
                level = level_idx,
                size = level_stages.len(),
                "role-pipeline level dispatching"
            );

            let mut prompts: Vec<(RoleStage, String)> =
                Vec::with_capacity(level_stages.len());
            for stage in level_stages {
                let prompt = build_stage_prompt(goal, &stage, &artifacts);
                prompts.push((stage, prompt));
            }

            let provider_ref = provider;
            let mut futs = Vec::with_capacity(prompts.len());
            for (stage, prompt) in &prompts {
                let timeout =
                    stage.stage_timeout.unwrap_or(default_timeout);
                let temperature =
                    stage.temperature.unwrap_or(default_temp);
                let model = params.model.clone();
                let stage_id = stage.id.clone();
                let stage_label = stage.label.clone();
                let sys = stage.system_prompt.clone();
                let prompt = prompt.clone();
                futs.push(async move {
                    let started = std::time::Instant::now();
                    let chat_fut = provider_ref.chat_with_system(
                        Some(&sys),
                        &prompt,
                        &model,
                        temperature,
                    );
                    let r = tokio::time::timeout(timeout, chat_fut).await;
                    let elapsed_ms = started.elapsed().as_millis();
                    match r {
                        Ok(Ok(answer)) => StageOutcome {
                            stage_id,
                            label: stage_label,
                            success: true,
                            elapsed_ms,
                            answer,
                            error: None,
                        },
                        Ok(Err(e)) => StageOutcome {
                            stage_id,
                            label: stage_label,
                            success: false,
                            elapsed_ms,
                            answer: String::new(),
                            error: Some(format!("provider error: {e}")),
                        },
                        Err(_) => StageOutcome {
                            stage_id,
                            label: stage_label,
                            success: false,
                            elapsed_ms,
                            answer: String::new(),
                            error: Some(format!(
                                "stage timed out after {:?}",
                                timeout
                            )),
                        },
                    }
                });
            }

            let level_outcomes = futures_util::future::join_all(futs).await;

            for outcome in level_outcomes {
                let key = format!(
                    "{}/{}/{}",
                    PIPELINE_NAMESPACE, run_id, outcome.stage_id
                );
                blackboard.write(
                    key,
                    serde_json::json!({
                        "run_id": &run_id,
                        "pipeline": &self.name,
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
                outcomes.push(outcome);
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
                .filter(|o| o.success)
                .last()
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
    artifacts: &HashMap<String, String>,
) -> String {
    let mut out = String::new();
    out.push_str("## Goal\n");
    out.push_str(goal.trim());
    out.push_str("\n\n");

    if !stage.depends_on.is_empty() {
        out.push_str("## Prior artifacts\n");
        for dep in &stage.depends_on {
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
