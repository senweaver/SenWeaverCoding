// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Post-turn self-reflection loop.
//!
//! The agent assesses its own performance after each turn and generates
//! self-improvement insights that are fed back into the system prompt
//! for subsequent turns.

use crate::providers::traits::{ChatMessage, Provider};
use parking_lot::RwLock;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;

/// Configuration for the self-reflection system.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SelfReflectionConfig {
    /// Enable self-reflection after each turn.
    #[serde(default)]
    pub enabled: bool,
    /// Reflect every N turns (1 = every turn, 3 = every 3rd turn).
    #[serde(default = "default_interval")]
    pub reflect_interval: u32,
    /// Maximum insights to retain for prompt injection.
    #[serde(default = "default_max_insights")]
    pub max_insights: usize,
    /// Use LLM for deep reflection (costs tokens) vs heuristic-only.
    #[serde(default)]
    pub llm_reflection: bool,
    /// Maximum characters for reflection injection into system prompt.
    #[serde(default = "default_max_inject_chars")]
    pub max_inject_chars: usize,
}

fn default_interval() -> u32 {
    1
}
fn default_max_insights() -> usize {
    10
}
fn default_max_inject_chars() -> usize {
    1500
}

impl Default for SelfReflectionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            reflect_interval: default_interval(),
            max_insights: default_max_insights(),
            llm_reflection: false,
            max_inject_chars: default_max_inject_chars(),
        }
    }
}

/// A single reflection insight.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Insight {
    /// What was learned from this interaction.
    pub observation: String,
    /// Behavioral adjustment to make.
    pub adjustment: String,
    /// Confidence in this insight (0.0-1.0).
    pub confidence: f64,
    /// How many turns this insight has been active.
    pub age_turns: u32,
    /// Category of insight for targeted application.
    pub category: InsightCategory,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InsightCategory {
    ResponseQuality,
    ToolUsage,
    UserPreference,
    ErrorRecovery,
    Communication,
}

const REFLECTION_PROMPT: &str = r#"You are a self-improvement engine. Analyze the conversation turn and generate insights.

Evaluate:
1. Did the response fully address the user's needs?
2. Were tools used effectively?
3. Could the response have been better structured?
4. Were there any errors or misunderstandings?

Respond ONLY with valid JSON:
{"observations":[{"observation":"...","adjustment":"...","confidence":0.0,"category":"ResponseQuality|ToolUsage|UserPreference|ErrorRecovery|Communication"}]}"#;

#[derive(Debug, Deserialize)]
struct ReflectionResponse {
    #[serde(default)]
    observations: Vec<RawInsight>,
}

#[derive(Debug, Deserialize)]
struct RawInsight {
    #[serde(default)]
    observation: String,
    #[serde(default)]
    adjustment: String,
    #[serde(default)]
    confidence: f64,
    #[serde(default)]
    category: String,
}

/// Self-reflection engine that accumulates insights over time.
#[derive(Clone)]
pub struct ReflectionEngine {
    config: SelfReflectionConfig,
    insights: Arc<RwLock<VecDeque<Insight>>>,
    turn_counter: Arc<std::sync::atomic::AtomicU32>,
}

impl ReflectionEngine {
    pub fn new(config: &SelfReflectionConfig) -> Self {
        Self {
            config: config.clone(),
            insights: Arc::new(RwLock::new(VecDeque::with_capacity(config.max_insights))),
            turn_counter: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        }
    }

    /// Perform reflection on a completed turn.
    ///
    /// Uses LLM or heuristics depending on config. Only fires every N turns
    /// per `reflect_interval`.
    pub async fn reflect(
        &self,
        provider: Option<&dyn Provider>,
        model: Option<&str>,
        user_query: &str,
        assistant_response: &str,
        tool_outcomes: &[(&str, bool)],
        reward: f64,
    ) {
        if !self.config.enabled {
            return;
        }

        let turn = self
            .turn_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if self.config.reflect_interval > 0 && turn % self.config.reflect_interval != 0 {
            return;
        }

        let new_insights = if self.config.llm_reflection {
            if let (Some(prov), Some(m)) = (provider, model) {
                self.llm_reflect(
                    prov,
                    m,
                    user_query,
                    assistant_response,
                    tool_outcomes,
                    reward,
                )
                .await
            } else {
                self.heuristic_reflect(user_query, assistant_response, tool_outcomes, reward)
            }
        } else {
            self.heuristic_reflect(user_query, assistant_response, tool_outcomes, reward)
        };

        let mut insights = self.insights.write();
        for ins in &mut *insights {
            ins.age_turns += 1;
        }

        for insight in new_insights {
            if insight.confidence < 0.3 {
                continue;
            }
            let duplicate = insights.iter().any(|existing| {
                existing.category == insight.category && existing.adjustment == insight.adjustment
            });
            if !duplicate {
                if insights.len() >= self.config.max_insights {
                    let oldest_idx = insights
                        .iter()
                        .enumerate()
                        .max_by_key(|(_, i)| i.age_turns)
                        .map(|(idx, _)| idx);
                    if let Some(idx) = oldest_idx {
                        insights.remove(idx);
                    }
                }
                insights.push_back(insight);
            }
        }
    }

    async fn llm_reflect(
        &self,
        provider: &dyn Provider,
        model: &str,
        user_query: &str,
        assistant_response: &str,
        tool_outcomes: &[(&str, bool)],
        reward: f64,
    ) -> Vec<Insight> {
        let tools_summary = tool_outcomes
            .iter()
            .map(|(name, ok)| format!("  {}: {}", name, if *ok { "success" } else { "failed" }))
            .collect::<Vec<_>>()
            .join("\n");

        let context = format!(
            "User: {}\n\nAssistant: {}\n\nTools used:\n{}\n\nReward score: {:.2}",
            truncate(user_query, 800),
            truncate(assistant_response, 1200),
            if tools_summary.is_empty() {
                "  (none)".to_string()
            } else {
                tools_summary
            },
            reward,
        );

        let messages = vec![
            ChatMessage::system(REFLECTION_PROMPT),
            ChatMessage::user(context),
        ];

        let result = provider.chat_with_history(&messages, model, 0.3).await;
        match result {
            Ok(response) => {
                if let Ok(parsed) = serde_json::from_str::<ReflectionResponse>(&response) {
                    parsed
                        .observations
                        .into_iter()
                        .map(|raw| Insight {
                            observation: raw.observation,
                            adjustment: raw.adjustment,
                            confidence: raw.confidence.clamp(0.0, 1.0),
                            age_turns: 0,
                            category: parse_category(&raw.category),
                        })
                        .collect()
                } else {
                    Vec::new()
                }
            }
            Err(_) => Vec::new(),
        }
    }

    /// Generate insights from heuristics without LLM call.
    fn heuristic_reflect(
        &self,
        user_query: &str,
        assistant_response: &str,
        tool_outcomes: &[(&str, bool)],
        reward: f64,
    ) -> Vec<Insight> {
        let mut insights = Vec::new();

        if reward < -0.3 {
            insights.push(Insight {
                observation: "Response received negative reward signal.".into(),
                adjustment: "Consider providing more thorough, accurate responses.".into(),
                confidence: 0.7,
                age_turns: 0,
                category: InsightCategory::ResponseQuality,
            });
        }

        let failed_tools: Vec<&str> = tool_outcomes
            .iter()
            .filter(|(_, ok)| !*ok)
            .map(|(name, _)| *name)
            .collect();
        if !failed_tools.is_empty() {
            insights.push(Insight {
                observation: format!("Tool(s) failed: {}", failed_tools.join(", ")),
                adjustment: "Validate tool inputs more carefully before execution.".into(),
                confidence: 0.8,
                age_turns: 0,
                category: InsightCategory::ToolUsage,
            });
        }

        if assistant_response.len() < user_query.len() / 2 && user_query.len() > 100 {
            insights.push(Insight {
                observation: "Response was significantly shorter than the query.".into(),
                adjustment: "Ensure responses are proportional to query complexity.".into(),
                confidence: 0.5,
                age_turns: 0,
                category: InsightCategory::ResponseQuality,
            });
        }

        let resp_lower = assistant_response.to_lowercase();
        if resp_lower.starts_with("i don't know")
            || resp_lower.starts_with("i'm not sure")
            || resp_lower.starts_with("i cannot")
        {
            insights.push(Insight {
                observation: "Response started with an uncertainty/refusal phrase.".into(),
                adjustment: "Try to provide partial answers or suggest alternatives.".into(),
                confidence: 0.6,
                age_turns: 0,
                category: InsightCategory::Communication,
            });
        }

        insights
    }

    /// Generate prompt injection from accumulated insights.
    pub fn prompt_injection(&self) -> Option<String> {
        if !self.config.enabled {
            return None;
        }

        let insights = self.insights.read();
        if insights.is_empty() {
            return None;
        }

        let mut output = String::from("<self_reflection_insights>\n");
        let max = self.config.max_inject_chars;

        let mut sorted: Vec<&Insight> = insights.iter().collect();
        sorted.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        for insight in sorted {
            let entry = format!(
                "- [{}] {}: {}\n",
                format_category(&insight.category),
                insight.observation,
                insight.adjustment,
            );
            if output.len() + entry.len() > max {
                break;
            }
            output.push_str(&entry);
        }

        output.push_str("</self_reflection_insights>");
        Some(output)
    }

    pub fn insight_count(&self) -> usize {
        self.insights.read().len()
    }

    pub fn clear_insights(&self) {
        self.insights.write().clear();
    }
}

fn parse_category(s: &str) -> InsightCategory {
    match s {
        "ToolUsage" => InsightCategory::ToolUsage,
        "UserPreference" => InsightCategory::UserPreference,
        "ErrorRecovery" => InsightCategory::ErrorRecovery,
        "Communication" => InsightCategory::Communication,
        _ => InsightCategory::ResponseQuality,
    }
}

fn format_category(c: &InsightCategory) -> &'static str {
    match c {
        InsightCategory::ResponseQuality => "quality",
        InsightCategory::ToolUsage => "tools",
        InsightCategory::UserPreference => "preference",
        InsightCategory::ErrorRecovery => "recovery",
        InsightCategory::Communication => "communication",
    }
}

fn truncate(s: &str, max_chars: usize) -> &str {
    if s.len() <= max_chars {
        return s;
    }
    let end = s
        .char_indices()
        .map(|(i, _)| i)
        .take_while(|&i| i <= max_chars)
        .last()
        .unwrap_or(0);
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heuristic_low_reward_generates_insight() {
        let config = SelfReflectionConfig {
            enabled: true,
            ..Default::default()
        };
        let engine = ReflectionEngine::new(&config);

        let insights = engine.heuristic_reflect("complex question", "short answer", &[], -0.5);
        assert!(!insights.is_empty());
        assert!(
            insights
                .iter()
                .any(|i| i.category == InsightCategory::ResponseQuality)
        );
    }

    #[test]
    fn heuristic_tool_failure_generates_insight() {
        let config = SelfReflectionConfig::default();
        let engine = ReflectionEngine::new(&config);

        let insights = engine.heuristic_reflect(
            "search for something",
            "Here are the results.",
            &[("web_search", false)],
            0.0,
        );
        assert!(
            insights
                .iter()
                .any(|i| i.category == InsightCategory::ToolUsage)
        );
    }

    #[test]
    fn prompt_injection_empty_when_no_insights() {
        let config = SelfReflectionConfig {
            enabled: true,
            ..Default::default()
        };
        let engine = ReflectionEngine::new(&config);
        assert!(engine.prompt_injection().is_none());
    }

    #[test]
    fn prompt_injection_with_insights() {
        let config = SelfReflectionConfig {
            enabled: true,
            ..Default::default()
        };
        let engine = ReflectionEngine::new(&config);

        {
            let mut insights = engine.insights.write();
            insights.push_back(Insight {
                observation: "test observation".into(),
                adjustment: "test adjustment".into(),
                confidence: 0.8,
                age_turns: 0,
                category: InsightCategory::ResponseQuality,
            });
        }

        let injection = engine.prompt_injection();
        assert!(injection.is_some());
        let text = injection.unwrap();
        assert!(text.contains("<self_reflection_insights>"));
        assert!(text.contains("test observation"));
    }

    #[test]
    fn default_config_values() {
        let config = SelfReflectionConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.reflect_interval, 1);
        assert_eq!(config.max_insights, 10);
    }

    #[test]
    fn category_parsing() {
        assert_eq!(parse_category("ToolUsage"), InsightCategory::ToolUsage);
        assert_eq!(parse_category("unknown"), InsightCategory::ResponseQuality);
    }
}
