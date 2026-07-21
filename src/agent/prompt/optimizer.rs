// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use parking_lot::RwLock;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PromptOptimizerConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_min_samples")]
    pub min_samples: usize,

    #[serde(default = "default_optimization_threshold")]
    pub optimization_threshold: f64,

    #[serde(default = "default_max_additions")]
    pub max_additions: usize,

    #[serde(default = "default_max_chars")]
    pub max_chars: usize,
}

fn default_min_samples() -> usize {
    5
}
fn default_optimization_threshold() -> f64 {
    0.3
}
fn default_max_additions() -> usize {
    5
}
fn default_max_chars() -> usize {
    1200
}

impl Default for PromptOptimizerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_samples: default_min_samples(),
            optimization_threshold: default_optimization_threshold(),
            max_additions: default_max_additions(),
            max_chars: default_max_chars(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryPerformance {
    pub category: String,
    pub total_turns: u32,
    pub total_reward: f64,
    pub recent_rewards: VecDeque<f64>,
    pub common_failures: VecDeque<String>,
    pub successful_patterns: VecDeque<String>,
}

impl CategoryPerformance {
    pub fn avg_reward(&self) -> f64 {
        if self.total_turns == 0 {
            0.0
        } else {
            self.total_reward / self.total_turns as f64
        }
    }

    pub fn recent_avg(&self) -> f64 {
        if self.recent_rewards.is_empty() {
            0.0
        } else {
            self.recent_rewards.iter().sum::<f64>() / self.recent_rewards.len() as f64
        }
    }

    pub fn is_improving(&self) -> bool {
        if self.recent_rewards.len() < 4 {
            return false;
        }
        let mid = self.recent_rewards.len() / 2;
        let first_half: f64 = self.recent_rewards.iter().take(mid).sum::<f64>() / mid as f64;
        let second_half: f64 = self.recent_rewards.iter().skip(mid).sum::<f64>()
            / (self.recent_rewards.len() - mid) as f64;
        second_half > first_half
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptAdjustment {
    pub category: String,
    pub instruction: String,
    pub priority: f64,
    pub source: AdjustmentSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AdjustmentSource {
    FailurePattern,
    SuccessReinforcement,
    PerformanceDip,
    UserPreference,
}

#[derive(Clone)]
pub struct PromptOptimizer {
    config: PromptOptimizerConfig,
    categories: Arc<RwLock<HashMap<String, CategoryPerformance>>>,
    adjustments: Arc<RwLock<Vec<PromptAdjustment>>>,
}

static GLOBAL_PROMPT_OPTIMIZER: std::sync::OnceLock<PromptOptimizer> =
    std::sync::OnceLock::new();

pub fn ensure_global_optimizer(config: &PromptOptimizerConfig) -> &'static PromptOptimizer {
    GLOBAL_PROMPT_OPTIMIZER.get_or_init(|| PromptOptimizer::new(config))
}

pub fn global_optimizer() -> &'static PromptOptimizer {
    GLOBAL_PROMPT_OPTIMIZER.get_or_init(|| {
        let config = crate::services::try_get_services()
            .map(|svc| svc.config().prompt_optimizer.clone())
            .unwrap_or_default();
        PromptOptimizer::new(&config)
    })
}

impl PromptOptimizer {
    pub fn new(config: &PromptOptimizerConfig) -> Self {
        Self {
            config: config.clone(),
            categories: Arc::new(RwLock::new(HashMap::new())),
            adjustments: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn record_turn(
        &self,
        category: &str,
        reward: f64,
        failure_reason: Option<&str>,
        successful_pattern: Option<&str>,
    ) {
        let mut cats = self.categories.write();
        let perf = cats
            .entry(category.to_string())
            .or_insert_with(|| CategoryPerformance {
                category: category.to_string(),
                total_turns: 0,
                total_reward: 0.0,
                recent_rewards: VecDeque::new(),
                common_failures: VecDeque::new(),
                successful_patterns: VecDeque::new(),
            });

        perf.total_turns += 1;
        perf.total_reward += reward;
        perf.recent_rewards.push_back(reward);
        if perf.recent_rewards.len() > 20 {
            perf.recent_rewards.pop_front();
        }

        if let Some(reason) = failure_reason {
            if !perf.common_failures.contains(&reason.to_string()) {
                perf.common_failures.push_back(reason.to_string());
                if perf.common_failures.len() > 10 {
                    perf.common_failures.pop_front();
                }
            }
        }

        if let Some(pattern) = successful_pattern {
            if !perf.successful_patterns.contains(&pattern.to_string()) {
                perf.successful_patterns.push_back(pattern.to_string());
                if perf.successful_patterns.len() > 10 {
                    perf.successful_patterns.pop_front();
                }
            }
        }

        drop(cats);
        self.update_adjustments();
    }

    fn update_adjustments(&self) {
        let cats = self.categories.read();
        let mut new_adjustments = Vec::new();

        for (_, perf) in cats.iter() {
            if (perf.total_turns as usize) < self.config.min_samples {
                continue;
            }

            let avg = perf.recent_avg();

            if avg < self.config.optimization_threshold {
                let instruction = if !perf.common_failures.is_empty() {
                    format!(
                        "For '{}' queries, avoid these pitfalls: {}",
                        perf.category,
                        perf.common_failures
                            .iter()
                            .take(3)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("; ")
                    )
                } else {
                    format!(
                        "For '{}' queries, focus on accuracy and completeness (recent quality: {:.1}%).",
                        perf.category,
                        (avg + 1.0) * 50.0
                    )
                };

                new_adjustments.push(PromptAdjustment {
                    category: perf.category.clone(),
                    instruction,
                    priority: (1.0 - avg).clamp(0.0, 1.0),
                    source: AdjustmentSource::PerformanceDip,
                });
            }

            if !perf.successful_patterns.is_empty() && avg > 0.5 {
                let instruction = format!(
                    "For '{}' queries, continue using: {}",
                    perf.category,
                    perf.successful_patterns
                        .iter()
                        .take(2)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("; ")
                );

                new_adjustments.push(PromptAdjustment {
                    category: perf.category.clone(),
                    instruction,
                    priority: 0.3,
                    source: AdjustmentSource::SuccessReinforcement,
                });
            }
        }

        new_adjustments.sort_by(|a, b| {
            b.priority
                .partial_cmp(&a.priority)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        new_adjustments.truncate(self.config.max_additions);

        let mut adj = self.adjustments.write();
        *adj = new_adjustments;
    }

    pub fn prompt_injection(&self) -> Option<String> {
        if !self.config.enabled {
            return None;
        }

        let adj = self.adjustments.read();
        if adj.is_empty() {
            return None;
        }

        let mut output = String::from("<adaptive_optimization>\n");
        output.push_str("Based on your interaction history, apply these learned behaviors:\n");

        for adjustment in adj.iter() {
            let entry = format!("- {}\n", adjustment.instruction);
            if output.len() + entry.len() > self.config.max_chars {
                break;
            }
            output.push_str(&entry);
        }

        output.push_str("</adaptive_optimization>");
        Some(output)
    }

    pub fn performance_summary(&self) -> Vec<CategoryPerformance> {
        let cats = self.categories.read();
        cats.values().cloned().collect()
    }

    pub fn active_adjustments(&self) -> Vec<PromptAdjustment> {
        self.adjustments.read().clone()
    }
}
