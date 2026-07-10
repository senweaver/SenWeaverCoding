// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::providers::traits::Provider;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubTask {
    pub id: String,
    pub description: String,
    pub prompt: String,
    pub required_capability: String,
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubTaskResult {
    pub task_id: String,
    pub agent_id: String,
    pub output: String,
    pub success: bool,

    pub confidence: Option<f32>,

    #[serde(default)]
    pub degraded: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureSummary {
    pub task_id: String,
    pub agent_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergedOutput {

    pub merged: String,

    pub degraded: bool,

    pub reasons: Vec<String>,

    pub failures: Vec<FailureSummary>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeStrategy {

    First,

    All,

    Voting,

    LlmJudge,
}

impl Default for MergeStrategy {
    fn default() -> Self {
        Self::All
    }
}

pub fn merge_results(results: &[SubTaskResult], strategy: MergeStrategy) -> String {
    let successful: Vec<&SubTaskResult> = results.iter().filter(|r| r.success).collect();

    match strategy {
        MergeStrategy::First => successful
            .first()
            .map_or_else(String::new, |r| r.output.clone()),
        MergeStrategy::All => successful
            .iter()
            .map(|r| format!("# {}\n{}", r.task_id, r.output))
            .collect::<Vec<_>>()
            .join("\n\n"),
        MergeStrategy::Voting => successful
            .iter()
            .max_by(|a, b| {
                a.confidence
                    .unwrap_or(0.0)
                    .partial_cmp(&b.confidence.unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map_or_else(String::new, |r| r.output.clone()),
        MergeStrategy::LlmJudge => {

            build_judge_prompt(&successful)
        }
    }
}

pub fn build_judge_prompt(successful: &[&SubTaskResult]) -> String {
    let mut buf = String::from(
        "You are an impartial judge.  Several specialist agents have produced \
         candidate answers for sub-tasks of a larger user request.  Synthesize \
         the single best answer.  Prefer factual accuracy over verbosity.  \
         If candidates conflict, reconcile them or pick the most confident one \
         and briefly justify.  Output ONLY the final answer  -  no preamble, no \
         commentary about the judging process.\n\n",
    );
    buf.push_str("Candidates for judgment:\n\n");
    for r in successful {
        buf.push_str(&format!(
            "--- candidate '{}' (agent={}, conf={:.2}) ---\n{}\n\n",
            r.task_id,
            r.agent_id,
            r.confidence.unwrap_or(0.0),
            r.output
        ));
    }
    buf
}

pub async fn merge_results_with_judge(
    results: &[SubTaskResult],
    strategy: MergeStrategy,
    provider: Option<Arc<dyn Provider>>,
    model: &str,
    temperature: f64,
) -> String {
    if !matches!(strategy, MergeStrategy::LlmJudge) {
        return merge_results(results, strategy);
    }

    let successful: Vec<&SubTaskResult> = results.iter().filter(|r| r.success).collect();
    if successful.is_empty() {
        return String::new();
    }
    if successful.len() == 1 {

        return successful[0].output.clone();
    }

    let Some(provider) = provider else {

        tracing::debug!(
            "MergeStrategy::LlmJudge requested without a provider; falling back to Voting"
        );
        return merge_results(results, MergeStrategy::Voting);
    };

    let prompt = build_judge_prompt(&successful);
    let judge_system = "You merge multiple candidate answers into the single best final answer. \
         Be decisive and concise.";

    match provider
        .chat_with_system(Some(judge_system), &prompt, model, temperature)
        .await
    {
        Ok(synthesized) if !synthesized.trim().is_empty() => synthesized,
        Ok(_) => {
            tracing::warn!("LLM judge returned empty output; falling back to Voting");
            merge_results(results, MergeStrategy::Voting)
        }
        Err(e) => {
            tracing::warn!(error = %e, "LLM judge failed; falling back to Voting");
            merge_results(results, MergeStrategy::Voting)
        }
    }
}

pub fn merge_results_structured(
    results: &[SubTaskResult],
    strategy: MergeStrategy,
) -> MergedOutput {
    let merged = merge_results(results, strategy);
    let mut reasons: Vec<String> = Vec::new();
    let mut degraded = false;
    for r in results {
        if r.degraded {
            degraded = true;
            if let Some(reason) = r.reason.as_ref() {
                if !reasons.iter().any(|existing| existing == reason) {
                    reasons.push(reason.clone());
                }
            }
        }
    }
    let failures = results
        .iter()
        .filter(|r| !r.success)
        .map(|r| FailureSummary {
            task_id: r.task_id.clone(),
            agent_id: r.agent_id.clone(),
            error: r.output.clone(),
        })
        .collect();
    MergedOutput {
        merged,
        degraded,
        reasons,
        failures,
    }
}

pub async fn merge_results_with_judge_structured(
    results: &[SubTaskResult],
    strategy: MergeStrategy,
    provider: Option<Arc<dyn Provider>>,
    model: &str,
    temperature: f64,
) -> MergedOutput {
    let merged = merge_results_with_judge(results, strategy, provider, model, temperature).await;
    let mut reasons: Vec<String> = Vec::new();
    let mut degraded = false;
    for r in results {
        if r.degraded {
            degraded = true;
            if let Some(reason) = r.reason.as_ref() {
                if !reasons.iter().any(|existing| existing == reason) {
                    reasons.push(reason.clone());
                }
            }
        }
    }
    let failures = results
        .iter()
        .filter(|r| !r.success)
        .map(|r| FailureSummary {
            task_id: r.task_id.clone(),
            agent_id: r.agent_id.clone(),
            error: r.output.clone(),
        })
        .collect();
    MergedOutput {
        merged,
        degraded,
        reasons,
        failures,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationPlan {
    pub sub_tasks: Vec<SubTask>,
    pub merge_strategy: MergeStrategy,
}

impl DelegationPlan {
    pub fn new(sub_tasks: Vec<SubTask>, merge_strategy: MergeStrategy) -> Self {
        Self {
            sub_tasks,
            merge_strategy,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        use std::collections::{HashMap, HashSet, VecDeque};
        let ids: HashSet<&str> = self.sub_tasks.iter().map(|t| t.id.as_str()).collect();
        if ids.len() != self.sub_tasks.len() {
            return Err("duplicate sub-task IDs".into());
        }
        for t in &self.sub_tasks {
            for dep in &t.depends_on {
                if !ids.contains(dep.as_str()) {
                    return Err(format!("unknown dependency '{}' in task '{}'", dep, t.id));
                }
            }
        }

        let mut indegree: HashMap<&str, usize> = HashMap::new();
        let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
        for t in &self.sub_tasks {
            indegree.entry(t.id.as_str()).or_insert(0);
            for dep in &t.depends_on {
                *indegree.entry(t.id.as_str()).or_insert(0) += 1;
                dependents.entry(dep.as_str()).or_default().push(t.id.as_str());
            }
        }
        let mut queue: VecDeque<&str> = indegree
            .iter()
            .filter(|(_, deg)| **deg == 0)
            .map(|(id, _)| *id)
            .collect();
        let mut resolved = 0usize;
        while let Some(id) = queue.pop_front() {
            resolved += 1;
            if let Some(children) = dependents.get(id) {
                for child in children {
                    if let Some(deg) = indegree.get_mut(child) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(child);
                        }
                    }
                }
            }
        }
        if resolved != self.sub_tasks.len() {
            let mut cyclic: Vec<&str> = indegree
                .iter()
                .filter(|(_, deg)| **deg > 0)
                .map(|(id, _)| *id)
                .collect();
            cyclic.sort_unstable();
            return Err(format!(
                "dependency cycle detected involving tasks: {}",
                cyclic.join(", ")
            ));
        }
        Ok(())
    }
}
