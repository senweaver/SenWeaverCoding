// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReinforcementConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_learning_rate")]
    pub learning_rate: f64,

    #[serde(default = "default_discount_factor")]
    pub discount_factor: f64,

    #[serde(default = "default_window_size")]
    pub window_size: usize,

    #[serde(default = "default_warmup_turns")]
    pub warmup_turns: usize,

    #[serde(default)]
    pub adaptive_routing: bool,

    #[serde(default)]
    pub adaptive_temperature: bool,

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyAdjustment {

    pub temperature_delta: f64,

    pub model_hint: Option<String>,

    pub category_strategies: HashMap<String, CategoryStrategy>,

    pub trend: PerformanceTrend,

    pub confidence: f64,
}

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

    fn compute_advantage(&self, record: &TurnRecord) -> f64 {
        let baselines = self.baseline_rewards.read();
        let baseline = baselines
            .get(&record.query_category)
            .copied()
            .unwrap_or(0.0);

        record.reward - baseline
    }

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

    pub fn recommended_temperature(&self, category: &str) -> f64 {
        let adjustment = self.get_policy_adjustment();

        if let Some(strategy) = adjustment.category_strategies.get(category) {
            if let Some(temp) = strategy.temperature_override {
                return temp;
            }
        }

        (self.config.base_temperature + adjustment.temperature_delta).clamp(0.1, 2.0)
    }

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

    pub fn total_turns(&self) -> usize {
        self.history.read().len()
    }

    pub fn baselines(&self) -> HashMap<String, f64> {
        self.baseline_rewards.read().clone()
    }
}

static GLOBAL_REINFORCEMENT: std::sync::OnceLock<Option<ReinforcementEngine>> =
    std::sync::OnceLock::new();

pub fn global_reinforcement_engine() -> Option<&'static ReinforcementEngine> {
    if let Some(cached) = GLOBAL_REINFORCEMENT.get() {
        return cached.as_ref();
    }
    let config = crate::services::try_get_services().map(|svc| svc.config().reinforcement.clone())?;
    GLOBAL_REINFORCEMENT
        .get_or_init(|| config.enabled.then(|| ReinforcementEngine::new(&config)))
        .as_ref()
}
