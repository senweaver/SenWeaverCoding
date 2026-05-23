// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SkillEvolutionConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_max_history")]
    pub max_history_per_tool: usize,

    #[serde(default = "default_decay_factor")]
    pub decay_factor: f64,

    #[serde(default = "default_min_executions")]
    pub min_executions: usize,
}

fn default_max_history() -> usize {
    100
}
fn default_decay_factor() -> f64 {
    0.95
}
fn default_min_executions() -> usize {
    3
}

impl Default for SkillEvolutionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_history_per_tool: default_max_history(),
            decay_factor: default_decay_factor(),
            min_executions: default_min_executions(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecution {
    pub timestamp: DateTime<Utc>,
    pub success: bool,
    pub duration_ms: u64,
    pub error_type: Option<String>,
    pub context_category: String,

    pub reward_contribution: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPerformance {
    pub tool_name: String,
    pub total_executions: u32,
    pub successes: u32,
    pub failures: u32,
    pub avg_duration_ms: f64,
    pub success_rate: f64,
    pub avg_reward_contribution: f64,
    pub common_errors: Vec<(String, u32)>,
    pub best_contexts: Vec<(String, f64)>,
    pub trend: PerformanceTrend,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PerformanceTrend {
    Improving,
    Stable,
    Degrading,
    InsufficientData,
}

#[derive(Clone)]
pub struct SkillEvolutionEngine {
    config: SkillEvolutionConfig,
    tools: Arc<RwLock<HashMap<String, VecDeque<ToolExecution>>>>,
}

static GLOBAL_SKILL_EVOLUTION: std::sync::OnceLock<SkillEvolutionEngine> =
    std::sync::OnceLock::new();

pub fn ensure_global_engine(config: &SkillEvolutionConfig) -> &'static SkillEvolutionEngine {
    GLOBAL_SKILL_EVOLUTION.get_or_init(|| SkillEvolutionEngine::new(config))
}

pub fn global_engine() -> &'static SkillEvolutionEngine {
    GLOBAL_SKILL_EVOLUTION
        .get_or_init(|| SkillEvolutionEngine::new(&SkillEvolutionConfig::default()))
}

impl SkillEvolutionEngine {
    pub fn new(config: &SkillEvolutionConfig) -> Self {
        Self {
            config: config.clone(),
            tools: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn record_execution(
        &self,
        tool_name: &str,
        success: bool,
        duration_ms: u64,
        error_type: Option<&str>,
        context_category: &str,
        reward_contribution: f64,
    ) {
        let execution = ToolExecution {
            timestamp: Utc::now(),
            success,
            duration_ms,
            error_type: error_type.map(String::from),
            context_category: context_category.to_string(),
            reward_contribution,
        };

        let mut tools = self.tools.write();
        let history = tools
            .entry(tool_name.to_string())
            .or_insert_with(VecDeque::new);

        if history.len() >= self.config.max_history_per_tool {
            history.pop_front();
        }
        history.push_back(execution);
    }

    pub fn tool_performance(&self, tool_name: &str) -> Option<ToolPerformance> {
        let tools = self.tools.read();
        let history = tools.get(tool_name)?;

        if history.is_empty() {
            return None;
        }

        let total = history.len() as u32;
        let successes = history.iter().filter(|e| e.success).count() as u32;
        let failures = total - successes;
        let success_rate = successes as f64 / total as f64;

        let avg_duration = history.iter().map(|e| e.duration_ms as f64).sum::<f64>() / total as f64;
        let avg_reward = history.iter().map(|e| e.reward_contribution).sum::<f64>() / total as f64;

        let mut error_counts: HashMap<String, u32> = HashMap::new();
        for exec in history.iter() {
            if let Some(ref err) = exec.error_type {
                *error_counts.entry(err.clone()).or_default() += 1;
            }
        }
        let mut common_errors: Vec<(String, u32)> = error_counts.into_iter().collect();
        common_errors.sort_by(|a, b| b.1.cmp(&a.1));
        common_errors.truncate(5);

        let mut context_rewards: HashMap<String, (f64, u32)> = HashMap::new();
        for exec in history.iter() {
            let entry = context_rewards
                .entry(exec.context_category.clone())
                .or_insert((0.0, 0));
            entry.0 += exec.reward_contribution;
            entry.1 += 1;
        }
        let mut best_contexts: Vec<(String, f64)> = context_rewards
            .into_iter()
            .map(|(ctx, (sum, count))| (ctx, sum / count as f64))
            .collect();
        best_contexts.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        best_contexts.truncate(3);

        let history_vec: Vec<_> = history.iter().cloned().collect();
        let trend = self.compute_trend(&history_vec);

        Some(ToolPerformance {
            tool_name: tool_name.to_string(),
            total_executions: total,
            successes,
            failures,
            avg_duration_ms: avg_duration,
            success_rate,
            avg_reward_contribution: avg_reward,
            common_errors,
            best_contexts,
            trend,
        })
    }

    fn compute_trend(&self, history: &[ToolExecution]) -> PerformanceTrend {
        if history.len() < 6 {
            return PerformanceTrend::InsufficientData;
        }

        let mid = history.len() / 2;
        let first_half_rate =
            history[..mid].iter().filter(|e| e.success).count() as f64 / mid as f64;
        let second_half_rate = history[mid..].iter().filter(|e| e.success).count() as f64
            / (history.len() - mid) as f64;

        let diff = second_half_rate - first_half_rate;
        if diff > 0.1 {
            PerformanceTrend::Improving
        } else if diff < -0.1 {
            PerformanceTrend::Degrading
        } else {
            PerformanceTrend::Stable
        }
    }

    pub fn recommend_tools(
        &self,
        available_tools: &[String],
        context_category: &str,
    ) -> Vec<ToolRecommendation> {
        let tools = self.tools.read();
        let mut recommendations = Vec::new();

        for tool_name in available_tools {
            if let Some(history) = tools.get(tool_name) {
                if history.len() < self.config.min_executions {
                    recommendations.push(ToolRecommendation {
                        tool_name: tool_name.clone(),
                        confidence: 0.5,
                        reason: "Insufficient data for recommendation.".into(),
                    });
                    continue;
                }

                let context_execs: Vec<&ToolExecution> = history
                    .iter()
                    .filter(|e| e.context_category == context_category)
                    .collect();

                let (success_rate, avg_reward) = if context_execs.is_empty() {
                    let rate =
                        history.iter().filter(|e| e.success).count() as f64 / history.len() as f64;
                    let avg = history.iter().map(|e| e.reward_contribution).sum::<f64>()
                        / history.len() as f64;
                    (rate, avg)
                } else {
                    let rate = context_execs.iter().filter(|e| e.success).count() as f64
                        / context_execs.len() as f64;
                    let avg = context_execs
                        .iter()
                        .map(|e| e.reward_contribution)
                        .sum::<f64>()
                        / context_execs.len() as f64;
                    (rate, avg)
                };

                let confidence = success_rate * 0.6 + f64::midpoint(avg_reward, 1.0) * 0.4;

                let reason = if success_rate > 0.8 {
                    format!(
                        "High success rate ({:.0}%) in this context.",
                        success_rate * 100.0
                    )
                } else if success_rate < 0.4 {
                    format!(
                        "Low success rate ({:.0}%), consider alternatives.",
                        success_rate * 100.0
                    )
                } else {
                    format!(
                        "Moderate performance ({:.0}% success).",
                        success_rate * 100.0
                    )
                };

                recommendations.push(ToolRecommendation {
                    tool_name: tool_name.clone(),
                    confidence: confidence.clamp(0.0, 1.0),
                    reason,
                });
            } else {
                recommendations.push(ToolRecommendation {
                    tool_name: tool_name.clone(),
                    confidence: 0.5,
                    reason: "No prior execution data.".into(),
                });
            }
        }

        recommendations.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        recommendations
    }

    pub fn prompt_injection(&self) -> Option<String> {
        if !self.config.enabled {
            return None;
        }

        let tools = self.tools.read();
        if tools.is_empty() {
            return None;
        }

        let mut output = String::from("<tool_evolution>\n");
        let mut any_content = false;

        for (name, history) in tools.iter() {
            if history.len() < self.config.min_executions {
                continue;
            }

            let history_vec: Vec<_> = history.iter().cloned().collect();
            let perf = self.tool_performance_from_history(name, &history_vec);

            if perf.success_rate < 0.5 {
                output.push_str(&format!(
                    "- Tool '{}': low success rate ({:.0}%). Common errors: {}. Use cautiously.\n",
                    name,
                    perf.success_rate * 100.0,
                    perf.common_errors
                        .iter()
                        .take(2)
                        .map(|(e, _)| e.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                ));
                any_content = true;
            } else if perf.success_rate > 0.9 && !perf.best_contexts.is_empty() {
                output.push_str(&format!(
                    "- Tool '{}': excellent performance ({:.0}% success). Best for: {}.\n",
                    name,
                    perf.success_rate * 100.0,
                    perf.best_contexts
                        .iter()
                        .take(2)
                        .map(|(c, _)| c.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                ));
                any_content = true;
            }
        }

        if !any_content {
            return None;
        }

        output.push_str("</tool_evolution>");
        Some(output)
    }

    fn tool_performance_from_history(
        &self,
        tool_name: &str,
        history: &[ToolExecution],
    ) -> ToolPerformance {
        let total = history.len() as u32;
        let successes = history.iter().filter(|e| e.success).count() as u32;
        let success_rate = successes as f64 / total as f64;

        let mut error_counts: HashMap<String, u32> = HashMap::new();
        for exec in history {
            if let Some(ref err) = exec.error_type {
                *error_counts.entry(err.clone()).or_default() += 1;
            }
        }
        let mut common_errors: Vec<(String, u32)> = error_counts.into_iter().collect();
        common_errors.sort_by(|a, b| b.1.cmp(&a.1));

        let mut context_rewards: HashMap<String, (f64, u32)> = HashMap::new();
        for exec in history {
            let entry = context_rewards
                .entry(exec.context_category.clone())
                .or_insert((0.0, 0));
            entry.0 += exec.reward_contribution;
            entry.1 += 1;
        }
        let mut best_contexts: Vec<(String, f64)> = context_rewards
            .into_iter()
            .map(|(ctx, (sum, count))| (ctx, sum / count as f64))
            .collect();
        best_contexts.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        ToolPerformance {
            tool_name: tool_name.to_string(),
            total_executions: total,
            successes,
            failures: total - successes,
            avg_duration_ms: history.iter().map(|e| e.duration_ms as f64).sum::<f64>()
                / total as f64,
            success_rate,
            avg_reward_contribution: history.iter().map(|e| e.reward_contribution).sum::<f64>()
                / total as f64,
            common_errors,
            best_contexts,
            trend: self.compute_trend(history),
        }
    }

    pub fn tracked_tools(&self) -> Vec<String> {
        self.tools.read().keys().cloned().collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRecommendation {
    pub tool_name: String,
    pub confidence: f64,
    pub reason: String,
}
