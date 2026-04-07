// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Reinforcement signal computation and policy adjustment.
//!
//! The central orchestrator for the self-evolution system. Aggregates signals
//! from all feedback sources and produces actionable policy adjustments
//! for the agent runtime.

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Configuration for the reinforcement engine.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReinforcementConfig {
    /// Enable the reinforcement learning engine.
    #[serde(default)]
    pub enabled: bool,
    /// Learning rate for policy adjustments (0.0-1.0, lower = more conservative).
    #[serde(default = "default_learning_rate")]
    pub learning_rate: f64,
    /// Discount factor for future rewards (gamma, 0.0-1.0).
    #[serde(default = "default_discount_factor")]
    pub discount_factor: f64,
    /// Window size for computing rolling statistics.
    #[serde(default = "default_window_size")]
    pub window_size: usize,
    /// Minimum turns before policy adjustments begin.
    #[serde(default = "default_warmup_turns")]
    pub warmup_turns: usize,
    /// Enable model routing adaptation based on rewards.
    #[serde(default)]
    pub adaptive_routing: bool,
    /// Enable temperature adaptation based on performance.
    #[serde(default)]
    pub adaptive_temperature: bool,
    /// Base temperature for the LLM (adapted by the engine).
    #[serde(default = "default_base_temperature")]
    pub base_temperature: f64,
}

fn default_learning_rate() -> f64 {
    0.1
}
fn default_discount_factor() -> f64 {
    0.95
}
fn default_window_size() -> usize {
    20
}
fn default_warmup_turns() -> usize {
    10
}
fn default_base_temperature() -> f64 {
    0.7
}

impl Default for ReinforcementConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            learning_rate: default_learning_rate(),
            discount_factor: default_discount_factor(),
            window_size: default_window_size(),
            warmup_turns: default_warmup_turns(),
            adaptive_routing: false,
            adaptive_temperature: false,
            base_temperature: default_base_temperature(),
        }
    }
}

/// A turn-level reinforcement record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnRecord {
    pub turn_index: usize,
    pub timestamp: DateTime<Utc>,
    pub reward: f64,
    pub model_used: String,
    pub temperature_used: f64,
    pub query_category: String,
    pub tools_used: Vec<String>,
    pub response_length: usize,
}

/// Policy adjustment recommendations from the reinforcement engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyAdjustment {
    /// Recommended temperature adjustment (-0.3 to +0.3).
    pub temperature_delta: f64,
    /// Recommended model routing hint (if adaptive routing enabled).
    pub model_hint: Option<String>,
    /// Per-category strategy adjustments.
    pub category_strategies: HashMap<String, CategoryStrategy>,
    /// Overall performance trend.
    pub trend: PerformanceTrend,
    /// Confidence in the adjustment (0.0-1.0).
    pub confidence: f64,
}

/// Strategy for a specific query category.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryStrategy {
    pub preferred_model_hint: Option<String>,
    pub temperature_override: Option<f64>,
    pub tool_preferences: Vec<String>,
    pub avoid_tools: Vec<String>,
    pub avg_reward: f64,
    pub sample_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PerformanceTrend {
    StrongImprovement,
    SlightImprovement,
    Stable,
    SlightDegradation,
    StrongDegradation,
    InsufficientData,
}

/// The reinforcement engine that computes advantages and policy adjustments.
#[derive(Clone)]
pub struct ReinforcementEngine {
    config: ReinforcementConfig,
    history: Arc<RwLock<Vec<TurnRecord>>>,
    baseline_rewards: Arc<RwLock<HashMap<String, f64>>>,
}

impl ReinforcementEngine {
    pub fn new(config: &ReinforcementConfig) -> Self {
        Self {
            config: config.clone(),
            history: Arc::new(RwLock::new(Vec::new())),
            baseline_rewards: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Record a turn and compute the advantage relative to the category baseline.
    pub fn record_turn(&self, record: TurnRecord) -> f64 {
        let advantage = self.compute_advantage(&record);

        let mut history = self.history.write();
        history.push(record.clone());
        if history.len() > self.config.window_size * 10 {
            let keep_from = history.len() - self.config.window_size * 5;
            history.drain(..keep_from);
        }

        let mut baselines = self.baseline_rewards.write();
        let baseline = baselines
            .entry(record.query_category.clone())
            .or_insert(0.0);
        *baseline = *baseline * (1.0 - self.config.learning_rate)
            + record.reward * self.config.learning_rate;

        advantage
    }

    /// Compute advantage: how much better/worse this turn was vs baseline.
    fn compute_advantage(&self, record: &TurnRecord) -> f64 {
        let baselines = self.baseline_rewards.read();
        let baseline = baselines
            .get(&record.query_category)
            .copied()
            .unwrap_or(0.0);

        record.reward - baseline
    }

    /// Get the current policy adjustment recommendation.
    pub fn get_policy_adjustment(&self) -> PolicyAdjustment {
        let history = self.history.read();

        if history.len() < self.config.warmup_turns {
            return PolicyAdjustment {
                temperature_delta: 0.0,
                model_hint: None,
                category_strategies: HashMap::new(),
                trend: PerformanceTrend::InsufficientData,
                confidence: 0.0,
            };
        }

        let recent: Vec<&TurnRecord> = history.iter().rev().take(self.config.window_size).collect();

        let recent_avg = recent.iter().map(|r| r.reward).sum::<f64>() / recent.len() as f64;
        let overall_avg = history.iter().map(|r| r.reward).sum::<f64>() / history.len() as f64;

        let temperature_delta = if self.config.adaptive_temperature {
            if recent_avg < -0.2 {
                -0.1 * self.config.learning_rate
            } else if recent_avg > 0.5 {
                0.05 * self.config.learning_rate
            } else {
                0.0
            }
        } else {
            0.0
        };

        let model_hint = if self.config.adaptive_routing {
            self.compute_model_hint(&recent)
        } else {
            None
        };

        let category_strategies = self.compute_category_strategies(&history);

        let trend = {
            let diff = recent_avg - overall_avg;
            if diff > 0.2 {
                PerformanceTrend::StrongImprovement
            } else if diff > 0.05 {
                PerformanceTrend::SlightImprovement
            } else if diff < -0.2 {
                PerformanceTrend::StrongDegradation
            } else if diff < -0.05 {
                PerformanceTrend::SlightDegradation
            } else {
                PerformanceTrend::Stable
            }
        };

        let confidence = (history.len() as f64 / (self.config.warmup_turns as f64 * 3.0)).min(1.0);

        PolicyAdjustment {
            temperature_delta,
            model_hint,
            category_strategies,
            trend,
            confidence,
        }
    }

    fn compute_model_hint(&self, recent: &[&TurnRecord]) -> Option<String> {
        let mut model_rewards: HashMap<String, (f64, u32)> = HashMap::new();
        for record in recent {
            let entry = model_rewards
                .entry(record.model_used.clone())
                .or_insert((0.0, 0));
            entry.0 += record.reward;
            entry.1 += 1;
        }

        model_rewards
            .into_iter()
            .filter(|(_, (_, count))| *count >= 3)
            .max_by(|(_, (sum_a, cnt_a)), (_, (sum_b, cnt_b))| {
                let avg_a = sum_a / *cnt_a as f64;
                let avg_b = sum_b / *cnt_b as f64;
                avg_a
                    .partial_cmp(&avg_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(model, _)| model)
    }

    fn compute_category_strategies(
        &self,
        history: &[TurnRecord],
    ) -> HashMap<String, CategoryStrategy> {
        let mut category_data: HashMap<String, Vec<&TurnRecord>> = HashMap::new();
        for record in history {
            category_data
                .entry(record.query_category.clone())
                .or_default()
                .push(record);
        }

        let mut strategies = HashMap::new();

        for (category, records) in &category_data {
            if records.len() < 3 {
                continue;
            }

            let avg_reward = records.iter().map(|r| r.reward).sum::<f64>() / records.len() as f64;

            let mut model_perf: HashMap<String, (f64, u32)> = HashMap::new();
            for r in records {
                let entry = model_perf.entry(r.model_used.clone()).or_insert((0.0, 0));
                entry.0 += r.reward;
                entry.1 += 1;
            }
            let preferred_model = model_perf
                .iter()
                .filter(|(_, (_, count))| *count >= 2)
                .max_by(|(_, (sa, ca)), (_, (sb, cb))| {
                    let a = sa / *ca as f64;
                    let b = sb / *cb as f64;
                    a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(m, _)| m.clone());

            let mut tool_rewards: HashMap<String, (f64, u32)> = HashMap::new();
            for r in records {
                for tool in &r.tools_used {
                    let entry = tool_rewards.entry(tool.clone()).or_insert((0.0, 0));
                    entry.0 += r.reward;
                    entry.1 += 1;
                }
            }

            let mut tool_pref: Vec<(String, f64)> = tool_rewards
                .iter()
                .map(|(t, (s, c))| (t.clone(), s / *c as f64))
                .collect();
            tool_pref.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            let good_tools: Vec<String> = tool_pref
                .iter()
                .filter(|(_, avg)| *avg > 0.3)
                .take(5)
                .map(|(t, _)| t.clone())
                .collect();
            let bad_tools: Vec<String> = tool_pref
                .iter()
                .filter(|(_, avg)| *avg < -0.3)
                .map(|(t, _)| t.clone())
                .collect();

            let temperature_override = if avg_reward < -0.2 {
                Some((self.config.base_temperature - 0.1).max(0.1))
            } else if avg_reward > 0.5 {
                Some(self.config.base_temperature)
            } else {
                None
            };

            strategies.insert(
                category.clone(),
                CategoryStrategy {
                    preferred_model_hint: preferred_model,
                    temperature_override,
                    tool_preferences: good_tools,
                    avoid_tools: bad_tools,
                    avg_reward,
                    sample_count: records.len(),
                },
            );
        }

        strategies
    }

    /// Get the recommended temperature for a query category.
    pub fn recommended_temperature(&self, category: &str) -> f64 {
        let adjustment = self.get_policy_adjustment();

        if let Some(strategy) = adjustment.category_strategies.get(category) {
            if let Some(temp) = strategy.temperature_override {
                return temp;
            }
        }

        (self.config.base_temperature + adjustment.temperature_delta).clamp(0.1, 2.0)
    }

    /// Generate prompt injection summarizing the reinforcement policy state.
    pub fn prompt_injection(&self) -> Option<String> {
        if !self.config.enabled {
            return None;
        }

        {
            let history = self.history.read();
            if history.len() < self.config.warmup_turns {
                return None;
            }
        }

        let adjustment = self.get_policy_adjustment();

        let trend_str = match adjustment.trend {
            PerformanceTrend::StrongImprovement => "strongly improving",
            PerformanceTrend::SlightImprovement => "slightly improving",
            PerformanceTrend::Stable => "stable",
            PerformanceTrend::SlightDegradation => "slightly degrading",
            PerformanceTrend::StrongDegradation => "degrading - increased care needed",
            PerformanceTrend::InsufficientData => return None,
        };

        let mut output = format!(
            "<reinforcement_policy>\nPerformance trend: {}. Confidence: {:.0}%.\n",
            trend_str,
            adjustment.confidence * 100.0,
        );

        for (cat, strategy) in &adjustment.category_strategies {
            if strategy.avg_reward < 0.0 || !strategy.avoid_tools.is_empty() {
                output.push_str(&format!(
                    "- Category '{}': avg reward {:.2}",
                    cat, strategy.avg_reward
                ));
                if !strategy.avoid_tools.is_empty() {
                    output.push_str(&format!(
                        ", avoid tools: {}",
                        strategy.avoid_tools.join(", ")
                    ));
                }
                if !strategy.tool_preferences.is_empty() {
                    output.push_str(&format!(
                        ", prefer tools: {}",
                        strategy.tool_preferences.join(", ")
                    ));
                }
                output.push('\n');
            }
        }

        output.push_str("</reinforcement_policy>");
        Some(output)
    }

    /// Get the total number of recorded turns.
    pub fn total_turns(&self) -> usize {
        self.history.read().len()
    }

    /// Get a summary of current baselines.
    pub fn baselines(&self) -> HashMap<String, f64> {
        self.baseline_rewards.read().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_record(reward: f64, category: &str, model: &str) -> TurnRecord {
        TurnRecord {
            turn_index: 0,
            timestamp: Utc::now(),
            reward,
            model_used: model.to_string(),
            temperature_used: 0.7,
            query_category: category.to_string(),
            tools_used: Vec::new(),
            response_length: 100,
        }
    }

    #[test]
    fn advantage_computation() {
        let config = ReinforcementConfig {
            enabled: true,
            warmup_turns: 1,
            ..Default::default()
        };
        let engine = ReinforcementEngine::new(&config);

        let adv1 = engine.record_turn(make_record(0.5, "code", "model-a"));
        assert!((adv1 - 0.5).abs() < f64::EPSILON);

        let adv2 = engine.record_turn(make_record(0.8, "code", "model-a"));
        assert!(adv2 > 0.0);
    }

    #[test]
    fn policy_adjustment_warmup() {
        let config = ReinforcementConfig {
            enabled: true,
            warmup_turns: 5,
            ..Default::default()
        };
        let engine = ReinforcementEngine::new(&config);

        engine.record_turn(make_record(0.5, "code", "model-a"));
        let adj = engine.get_policy_adjustment();
        assert!(matches!(adj.trend, PerformanceTrend::InsufficientData));
        assert!(adj.confidence < 0.01);
    }

    #[test]
    fn policy_adjustment_after_warmup() {
        let config = ReinforcementConfig {
            enabled: true,
            warmup_turns: 3,
            window_size: 5,
            ..Default::default()
        };
        let engine = ReinforcementEngine::new(&config);

        for _ in 0..5 {
            engine.record_turn(make_record(0.7, "code", "model-a"));
        }

        let adj = engine.get_policy_adjustment();
        assert!(adj.confidence > 0.0);
    }

    #[test]
    fn baselines_update_over_time() {
        let config = ReinforcementConfig {
            enabled: true,
            learning_rate: 0.5,
            ..Default::default()
        };
        let engine = ReinforcementEngine::new(&config);

        engine.record_turn(make_record(1.0, "code", "m"));
        engine.record_turn(make_record(1.0, "code", "m"));

        let baselines = engine.baselines();
        let code_baseline = baselines.get("code").unwrap();
        assert!(*code_baseline > 0.5);
    }

    #[test]
    fn recommended_temperature_default() {
        let config = ReinforcementConfig {
            base_temperature: 0.7,
            ..Default::default()
        };
        let engine = ReinforcementEngine::new(&config);
        let temp = engine.recommended_temperature("code");
        assert!((temp - 0.7).abs() < 0.1);
    }

    #[test]
    fn category_strategy_with_tools() {
        let config = ReinforcementConfig {
            enabled: true,
            warmup_turns: 2,
            ..Default::default()
        };
        let engine = ReinforcementEngine::new(&config);

        for _ in 0..5 {
            let mut record = make_record(0.8, "code", "model-a");
            record.tools_used = vec!["file_read".into()];
            engine.record_turn(record);
        }

        let adj = engine.get_policy_adjustment();
        let code_strategy = adj.category_strategies.get("code");
        assert!(code_strategy.is_some());
    }
}
