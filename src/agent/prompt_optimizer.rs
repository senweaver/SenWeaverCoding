// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Adaptive prompt optimization based on feedback history.
//!
//! Analyzes feedback patterns and generates prompt adjustments that
//! improve response quality over time.

use parking_lot::RwLock;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// Configuration for the adaptive prompt optimizer.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PromptOptimizerConfig {
    /// Enable adaptive prompt optimization.
    #[serde(default)]
    pub enabled: bool,
    /// Minimum feedback entries before optimization kicks in.
    #[serde(default = "default_min_samples")]
    pub min_samples: usize,
    /// Reward threshold below which a category triggers optimization.
    #[serde(default = "default_optimization_threshold")]
    pub optimization_threshold: f64,
    /// Maximum prompt additions to inject.
    #[serde(default = "default_max_additions")]
    pub max_additions: usize,
    /// Maximum characters for optimized prompt injection.
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

/// Tracks per-category performance for prompt optimization.
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

/// A generated prompt adjustment.
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

/// Adaptive prompt optimizer that learns from feedback patterns.
#[derive(Clone)]
pub struct PromptOptimizer {
    config: PromptOptimizerConfig,
    categories: Arc<RwLock<HashMap<String, CategoryPerformance>>>,
    adjustments: Arc<RwLock<Vec<PromptAdjustment>>>,
}

impl PromptOptimizer {
    pub fn new(config: &PromptOptimizerConfig) -> Self {
        Self {
            config: config.clone(),
            categories: Arc::new(RwLock::new(HashMap::new())),
            adjustments: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Record a turn's reward for a given category and update adjustments.
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

    /// Re-compute prompt adjustments based on category performance.
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

    /// Generate prompt injection from current adjustments.
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

    /// Get current category performance data.
    pub fn performance_summary(&self) -> Vec<CategoryPerformance> {
        let cats = self.categories.read();
        cats.values().cloned().collect()
    }

    /// Get current active adjustments.
    pub fn active_adjustments(&self) -> Vec<PromptAdjustment> {
        self.adjustments.read().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_track_performance() {
        let config = PromptOptimizerConfig {
            enabled: true,
            min_samples: 2,
            ..Default::default()
        };
        let optimizer = PromptOptimizer::new(&config);

        optimizer.record_turn("code", 0.8, None, Some("structured output"));
        optimizer.record_turn("code", 0.7, None, None);
        optimizer.record_turn("code", 0.9, None, None);

        let summary = optimizer.performance_summary();
        assert_eq!(summary.len(), 1);
        assert!(summary[0].avg_reward() > 0.7);
    }

    #[test]
    fn low_performance_generates_adjustment() {
        let config = PromptOptimizerConfig {
            enabled: true,
            min_samples: 2,
            optimization_threshold: 0.5,
            ..Default::default()
        };
        let optimizer = PromptOptimizer::new(&config);

        optimizer.record_turn("search", -0.5, Some("empty results"), None);
        optimizer.record_turn("search", -0.3, Some("timeout"), None);
        optimizer.record_turn("search", 0.1, None, None);

        let adj = optimizer.active_adjustments();
        assert!(!adj.is_empty());
    }

    #[test]
    fn prompt_injection_disabled() {
        let config = PromptOptimizerConfig::default();
        let optimizer = PromptOptimizer::new(&config);
        assert!(optimizer.prompt_injection().is_none());
    }

    #[test]
    fn prompt_injection_with_adjustments() {
        let config = PromptOptimizerConfig {
            enabled: true,
            min_samples: 1,
            optimization_threshold: 0.5,
            ..Default::default()
        };
        let optimizer = PromptOptimizer::new(&config);

        optimizer.record_turn("general", -0.2, Some("too vague"), None);
        optimizer.record_turn("general", 0.1, None, None);

        let injection = optimizer.prompt_injection();
        assert!(injection.is_some());
        let text = injection.unwrap();
        assert!(text.contains("<adaptive_optimization>"));
    }

    #[test]
    fn category_improvement_detection() {
        let perf = CategoryPerformance {
            category: "test".into(),
            total_turns: 8,
            total_reward: 2.0,
            recent_rewards: vec![-0.5, -0.3, -0.1, 0.0, 0.2, 0.4, 0.6, 0.8].into(),
            common_failures: VecDeque::new(),
            successful_patterns: VecDeque::new(),
        };
        assert!(perf.is_improving());
    }

    #[test]
    fn category_not_improving() {
        let perf = CategoryPerformance {
            category: "test".into(),
            total_turns: 4,
            total_reward: 0.0,
            recent_rewards: vec![0.8, 0.6, -0.1, -0.3].into(),
            common_failures: VecDeque::new(),
            successful_patterns: VecDeque::new(),
        };
        assert!(!perf.is_improving());
    }
}
