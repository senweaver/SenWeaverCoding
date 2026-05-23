// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::providers::traits::{ChatMessage, Provider};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SelfEvalConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_eval_votes")]
    pub eval_votes: u32,

    #[serde(default = "default_judge_temperature")]
    pub judge_temperature: f64,

    #[serde(default = "default_accept_threshold")]
    pub accept_threshold: f64,

    #[serde(default = "default_judge_max_tokens")]
    pub judge_max_tokens: u32,

    #[serde(default = "default_true")]
    pub persist_scores: bool,
}

fn default_eval_votes() -> u32 {
    3
}
fn default_judge_temperature() -> f64 {
    0.3
}
fn default_accept_threshold() -> f64 {
    0.6
}
fn default_judge_max_tokens() -> u32 {
    256
}
fn default_true() -> bool {
    true
}

impl Default for SelfEvalConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            eval_votes: default_eval_votes(),
            judge_temperature: default_judge_temperature(),
            accept_threshold: default_accept_threshold(),
            judge_max_tokens: default_judge_max_tokens(),
            persist_scores: default_true(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JudgeVerdict {

    pub score: f64,

    pub rationale: String,

    pub suggestions: Vec<String>,

    pub should_retry: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalDimensions {
    pub relevance: f64,
    pub completeness: f64,
    pub accuracy: f64,
    pub clarity: f64,
    pub helpfulness: f64,
}

impl EvalDimensions {
    pub fn aggregate(&self) -> f64 {
        self.relevance * 0.25
            + self.completeness * 0.20
            + self.accuracy * 0.25
            + self.clarity * 0.15
            + self.helpfulness * 0.15
    }
}

const JUDGE_SYSTEM_PROMPT: &str = r#"You are a response quality judge. Evaluate the assistant's response to the user's query.

Score each dimension from 0.0 to 1.0:
- relevance: Does the response address the user's actual question?
- completeness: Does it cover all aspects of the query?
- accuracy: Is the information correct and precise?
- clarity: Is it well-structured and easy to understand?
- helpfulness: Does it provide actionable, useful information?

Also provide:
- overall_score: -1.0 to 1.0 (-1 = harmful/wrong, 0 = neutral, 1 = excellent)
- rationale: Brief explanation (1-2 sentences)
- suggestions: List of specific improvements (empty if score > 0.6)
- should_retry: true if response is unacceptable

Respond ONLY with valid JSON:
{"relevance":0.0,"completeness":0.0,"accuracy":0.0,"clarity":0.0,"helpfulness":0.0,"overall_score":0.0,"rationale":"...","suggestions":[],"should_retry":false}"#;

#[derive(Debug, Deserialize)]
struct JudgeResponse {
    #[serde(default)]
    relevance: f64,
    #[serde(default)]
    completeness: f64,
    #[serde(default)]
    accuracy: f64,
    #[serde(default)]
    clarity: f64,
    #[serde(default)]
    helpfulness: f64,
    #[serde(default)]
    overall_score: f64,
    #[serde(default)]
    rationale: String,
    #[serde(default)]
    suggestions: Vec<String>,
    #[serde(default)]
    should_retry: bool,
}

pub async fn judge_response(
    provider: &dyn Provider,
    model: &str,
    user_query: &str,
    assistant_response: &str,
    next_user_message: Option<&str>,
    config: &SelfEvalConfig,
) -> Option<JudgeVerdict> {
    if !config.enabled {
        return None;
    }

    let mut scores: Vec<f64> = Vec::new();
    let mut all_suggestions: Vec<String> = Vec::new();
    let mut last_rationale = String::new();
    let mut retry_votes = 0u32;

    let context = if let Some(next_msg) = next_user_message {
        format!(
            "User query:\n{}\n\nAssistant response:\n{}\n\nUser's follow-up (evidence of quality):\n{}",
            truncate(user_query, 1500),
            truncate(assistant_response, 2000),
            truncate(next_msg, 500)
        )
    } else {
        format!(
            "User query:\n{}\n\nAssistant response:\n{}",
            truncate(user_query, 1500),
            truncate(assistant_response, 2000)
        )
    };

    let messages = vec![
        ChatMessage::system(JUDGE_SYSTEM_PROMPT),
        ChatMessage::user(context),
    ];

    for _ in 0..config.eval_votes {
        let result = provider
            .chat_with_history(&messages, model, config.judge_temperature as f64)
            .await;
        if let Ok(response) = result {
            if let Ok(parsed) = serde_json::from_str::<JudgeResponse>(&response) {
                scores.push(parsed.overall_score);
                if parsed.should_retry {
                    retry_votes += 1;
                }
                for s in &parsed.suggestions {
                    if !all_suggestions.contains(s) {
                        all_suggestions.push(s.clone());
                    }
                }
                last_rationale = parsed.rationale;
            }
        }
    }

    if scores.is_empty() {
        return None;
    }

    let avg_score = scores.iter().sum::<f64>() / scores.len() as f64;
    let majority_retry = retry_votes > config.eval_votes / 2;

    Some(JudgeVerdict {
        score: avg_score.clamp(-1.0, 1.0),
        rationale: last_rationale,
        suggestions: all_suggestions,
        should_retry: majority_retry,
    })
}

pub fn heuristic_eval(
    user_query: &str,
    assistant_response: &str,
    tool_results: &[(&str, bool)],
) -> EvalDimensions {
    let resp_lower = assistant_response.to_lowercase();
    let query_lower = user_query.to_lowercase();

    let relevance = if assistant_response.is_empty() {
        0.0
    } else {
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();
        let overlap = query_words
            .iter()
            .filter(|w| w.len() > 3 && resp_lower.contains(**w))
            .count();
        let ratio = if query_words.is_empty() {
            0.5
        } else {
            overlap as f64 / query_words.len() as f64
        };
        (0.3 + ratio * 0.7).min(1.0)
    };

    let completeness = {
        let len = assistant_response.len();
        let query_len = user_query.len();
        let ratio = len as f64 / (query_len as f64 * 3.0).max(50.0);
        ratio.clamp(0.1, 1.0)
    };

    let accuracy = {
        let cop_out_phrases = ["i don't know", "i'm not sure", "i cannot", "as an ai"];
        let is_cop_out = cop_out_phrases.iter().any(|p| resp_lower.starts_with(p));
        let tool_success_rate = if tool_results.is_empty() {
            1.0
        } else {
            let successes = tool_results.iter().filter(|(_, ok)| *ok).count();
            successes as f64 / tool_results.len() as f64
        };
        let base = if is_cop_out { 0.3 } else { 0.7 };
        (base * 0.5 + tool_success_rate * 0.5).min(1.0)
    };

    let clarity: f64 = {
        let has_structure = assistant_response.contains('\n')
            || assistant_response.contains("- ")
            || assistant_response.contains("```");
        let base: f64 = if has_structure { 0.8 } else { 0.6 };
        let len_penalty: f64 = if assistant_response.len() > 5000 {
            0.1
        } else {
            0.0
        };
        (base - len_penalty).max(0.1)
    };

    let helpfulness: f64 = {
        let has_code = assistant_response.contains("```");
        let has_links = assistant_response.contains("http");
        let has_steps = assistant_response.contains("1.") || assistant_response.contains("Step ");
        let base: f64 = 0.5;
        let bonus: f64 = if has_code { 0.15 } else { 0.0 }
            + if has_links { 0.1 } else { 0.0 }
            + if has_steps { 0.1 } else { 0.0 };
        (base + bonus).min(1.0)
    };

    EvalDimensions {
        relevance,
        completeness,
        accuracy,
        clarity,
        helpfulness,
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
