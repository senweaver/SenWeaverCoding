// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Experience replay memory for scored interaction storage.
//!
//! Stores scored interaction experiences that can be recalled for adaptive behavior.
//! Experiences are ranked by reward and can be sampled for prompt injection
//! (few-shot learning from past successes/failures).

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;

/// Configuration for the experience replay system.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExperienceConfig {
    /// Enable experience replay collection.
    #[serde(default)]
    pub enabled: bool,
    /// Maximum experiences to retain in memory.
    #[serde(default = "default_capacity")]
    pub capacity: usize,
    /// Minimum reward threshold to retain an experience (-1.0 to 1.0).
    /// Experiences below this are discarded (keeps only learning-worthy ones).
    #[serde(default = "default_min_reward")]
    pub min_retain_reward: f64,
    /// Number of top experiences to inject as few-shot examples.
    #[serde(default = "default_few_shot_count")]
    pub few_shot_count: usize,
    /// Number of worst experiences to inject as anti-examples.
    #[serde(default = "default_anti_example_count")]
    pub anti_example_count: usize,
    /// Maximum characters per experience in prompt injection.
    #[serde(default = "default_max_inject_chars")]
    pub max_inject_chars: usize,
}

fn default_capacity() -> usize {
    500
}
fn default_min_reward() -> f64 {
    -0.8
}
fn default_few_shot_count() -> usize {
    2
}
fn default_anti_example_count() -> usize {
    1
}
fn default_max_inject_chars() -> usize {
    2000
}

impl Default for ExperienceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            capacity: default_capacity(),
            min_retain_reward: default_min_reward(),
            few_shot_count: default_few_shot_count(),
            anti_example_count: default_anti_example_count(),
            max_inject_chars: default_max_inject_chars(),
        }
    }
}

/// A stored interaction experience with quality score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Experience {
    pub id: String,
    pub session_id: String,
    pub timestamp: DateTime<Utc>,
    pub user_query: String,
    pub assistant_response: String,
    pub tools_used: Vec<String>,
    pub model: String,
    /// Combined reward signal (-1.0 to 1.0).
    pub reward: f64,
    /// Query category/type for similarity matching.
    pub query_category: String,
    /// Number of times this experience has been replayed as a few-shot example.
    pub replay_count: u32,
}

/// Thread-safe experience replay buffer.
#[derive(Clone)]
pub struct ExperienceReplay {
    config: ExperienceConfig,
    buffer: Arc<RwLock<VecDeque<Experience>>>,
}

impl ExperienceReplay {
    pub fn new(config: &ExperienceConfig) -> Self {
        Self {
            config: config.clone(),
            buffer: Arc::new(RwLock::new(VecDeque::with_capacity(config.capacity))),
        }
    }

    /// Whether experience collection (store) and prompt injection are enabled.
    #[must_use]
    pub fn collection_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Store a new experience. Discards if below minimum reward threshold.
    pub fn store(&self, experience: Experience) -> bool {
        if experience.reward < self.config.min_retain_reward {
            return false;
        }

        let mut buf = self.buffer.write();
        if buf.len() >= self.config.capacity {
            let min_idx = buf
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    a.reward
                        .partial_cmp(&b.reward)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| i);

            if let Some(idx) = min_idx {
                if buf[idx].reward < experience.reward {
                    buf.remove(idx);
                } else {
                    return false;
                }
            }
        }

        buf.push_back(experience);
        true
    }

    /// Get top-N experiences by reward, optionally filtered by category.
    pub fn top_experiences(&self, n: usize, category: Option<&str>) -> Vec<Experience> {
        let buf = self.buffer.read();
        let mut filtered: Vec<&Experience> = buf
            .iter()
            .filter(|e| category.map_or(true, |c| e.query_category == c))
            .collect();

        filtered.sort_by(|a, b| {
            b.reward
                .partial_cmp(&a.reward)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        filtered.into_iter().take(n).cloned().collect()
    }

    /// Get worst-N experiences by reward (anti-examples for learning).
    pub fn worst_experiences(&self, n: usize, category: Option<&str>) -> Vec<Experience> {
        let buf = self.buffer.read();
        let mut filtered: Vec<&Experience> = buf
            .iter()
            .filter(|e| category.map_or(true, |c| e.query_category == c) && e.reward < 0.0)
            .collect();

        filtered.sort_by(|a, b| {
            a.reward
                .partial_cmp(&b.reward)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        filtered.into_iter().take(n).cloned().collect()
    }

    /// Generate a prompt injection block from experience replay.
    ///
    /// Includes top successes as positive examples and worst failures
    /// as anti-examples to steer future behavior.
    pub fn prompt_injection(&self, query_category: Option<&str>) -> Option<String> {
        if !self.config.enabled {
            return None;
        }

        let good = self.top_experiences(self.config.few_shot_count, query_category);
        let bad = self.worst_experiences(self.config.anti_example_count, query_category);

        if good.is_empty() && bad.is_empty() {
            return None;
        }

        let mut output = String::from("<experience_replay>\n");
        let max = self.config.max_inject_chars;

        if !good.is_empty() {
            output.push_str("<successful_patterns>\n");
            for (i, exp) in good.iter().enumerate() {
                let entry = format!(
                    "Example {}: Query type '{}', reward {:.2}\n\
                     User: {}\nAssistant approach: {}\n\n",
                    i + 1,
                    exp.query_category,
                    exp.reward,
                    truncate_str(&exp.user_query, 200),
                    truncate_str(&exp.assistant_response, 300),
                );
                if output.len() + entry.len() > max {
                    break;
                }
                output.push_str(&entry);
            }
            output.push_str("</successful_patterns>\n");
        }

        if !bad.is_empty() {
            output.push_str("<avoid_patterns>\n");
            for (i, exp) in bad.iter().enumerate() {
                let entry = format!(
                    "Anti-example {}: Query type '{}', reward {:.2}\n\
                     Avoid this approach for similar queries.\n\n",
                    i + 1,
                    exp.query_category,
                    exp.reward,
                );
                if output.len() + entry.len() > max {
                    break;
                }
                output.push_str(&entry);
            }
            output.push_str("</avoid_patterns>\n");
        }

        output.push_str("</experience_replay>");
        Some(output)
    }

    /// Get statistics about the experience buffer.
    pub fn stats(&self) -> ExperienceStats {
        let buf = self.buffer.read();
        let count = buf.len();

        if count == 0 {
            return ExperienceStats {
                total: 0,
                avg_reward: 0.0,
                positive_count: 0,
                negative_count: 0,
                top_categories: Vec::new(),
            };
        }

        let avg = buf.iter().map(|e| e.reward).sum::<f64>() / count as f64;
        let positive = buf.iter().filter(|e| e.reward > 0.0).count();
        let negative = buf.iter().filter(|e| e.reward < 0.0).count();

        let mut cat_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for exp in buf.iter() {
            *cat_counts.entry(exp.query_category.clone()).or_default() += 1;
        }
        let mut top_cats: Vec<(String, usize)> = cat_counts.into_iter().collect();
        top_cats.sort_by(|a, b| b.1.cmp(&a.1));
        top_cats.truncate(5);

        ExperienceStats {
            total: count,
            avg_reward: avg,
            positive_count: positive,
            negative_count: negative,
            top_categories: top_cats,
        }
    }

    pub fn len(&self) -> usize {
        self.buffer.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.read().is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperienceStats {
    pub total: usize,
    pub avg_reward: f64,
    pub positive_count: usize,
    pub negative_count: usize,
    pub top_categories: Vec<(String, usize)>,
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let end = s
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i <= max)
            .last()
            .unwrap_or(0);
        format!("{}...", &s[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_exp(reward: f64, category: &str) -> Experience {
        Experience {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: "test".into(),
            timestamp: Utc::now(),
            user_query: "test query".into(),
            assistant_response: "test response".into(),
            tools_used: Vec::new(),
            model: "test-model".into(),
            reward,
            query_category: category.into(),
            replay_count: 0,
        }
    }

    #[test]
    fn store_and_retrieve() {
        let config = ExperienceConfig {
            enabled: true,
            ..Default::default()
        };
        let replay = ExperienceReplay::new(&config);

        replay.store(make_exp(0.9, "code"));
        replay.store(make_exp(0.5, "code"));
        replay.store(make_exp(-0.3, "general"));

        assert_eq!(replay.len(), 3);

        let top = replay.top_experiences(2, Some("code"));
        assert_eq!(top.len(), 2);
        assert!(top[0].reward >= top[1].reward);
    }

    #[test]
    fn below_threshold_discarded() {
        let config = ExperienceConfig {
            enabled: true,
            min_retain_reward: -0.5,
            ..Default::default()
        };
        let replay = ExperienceReplay::new(&config);

        assert!(!replay.store(make_exp(-0.9, "bad")));
        assert!(replay.is_empty());
    }

    #[test]
    fn capacity_eviction() {
        let config = ExperienceConfig {
            enabled: true,
            capacity: 3,
            ..Default::default()
        };
        let replay = ExperienceReplay::new(&config);

        replay.store(make_exp(0.1, "a"));
        replay.store(make_exp(0.5, "b"));
        replay.store(make_exp(0.3, "c"));
        replay.store(make_exp(0.9, "d"));

        assert_eq!(replay.len(), 3);
        let top = replay.top_experiences(10, None);
        assert!(top[0].reward > 0.8);
    }

    #[test]
    fn prompt_injection_output() {
        let config = ExperienceConfig {
            enabled: true,
            ..Default::default()
        };
        let replay = ExperienceReplay::new(&config);

        replay.store(make_exp(0.9, "code"));
        replay.store(make_exp(-0.5, "code"));

        let injection = replay.prompt_injection(None);
        assert!(injection.is_some());
        let text = injection.unwrap();
        assert!(text.contains("<experience_replay>"));
        assert!(text.contains("<successful_patterns>"));
    }

    #[test]
    fn stats_calculation() {
        let config = ExperienceConfig {
            enabled: true,
            ..Default::default()
        };
        let replay = ExperienceReplay::new(&config);

        replay.store(make_exp(0.8, "code"));
        replay.store(make_exp(-0.3, "general"));

        let stats = replay.stats();
        assert_eq!(stats.total, 2);
        assert_eq!(stats.positive_count, 1);
        assert_eq!(stats.negative_count, 1);
    }
}
