// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Suggestions Engine - contextual next-action suggestions for agent sessions.
//!
//! Analyzes conversation history and available tools to suggest
//! relevant follow-up actions the user might want to take.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SuggestionsConfig {

    #[serde(default = "default_enabled")]
    pub enabled: bool,

    #[serde(default = "default_max_suggestions")]
    pub max_suggestions: usize,
}

fn default_enabled() -> bool {
    true
}
fn default_max_suggestions() -> usize {
    4
}

impl Default for SuggestionsConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            max_suggestions: default_max_suggestions(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {

    pub label: String,

    pub prompt: String,

    pub category: SuggestionCategory,

    pub relevance: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionCategory {

    FollowUp,

    Explore,

    Action,

    Refine,
}

impl std::fmt::Display for SuggestionCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FollowUp => write!(f, "follow_up"),
            Self::Explore => write!(f, "explore"),
            Self::Action => write!(f, "action"),
            Self::Refine => write!(f, "refine"),
        }
    }
}

pub fn generate_rule_based_suggestions(
    user_message: &str,
    assistant_response: &str,
    available_tools: &[String],
    config: &SuggestionsConfig,
) -> Vec<Suggestion> {
    if !config.enabled {
        return Vec::new();
    }

    let mut suggestions = Vec::new();
    let msg_lower = user_message.to_lowercase();
    let resp_lower = assistant_response.to_lowercase();

    if resp_lower.contains("error") || resp_lower.contains("failed") || resp_lower.contains("issue")
    {
        suggestions.push(Suggestion {
            label: "Debug further".to_string(),
            prompt: "Can you investigate the error in more detail and suggest a fix?".to_string(),
            category: SuggestionCategory::Explore,
            relevance: 0.9,
        });
    }

    if resp_lower.contains("file") || resp_lower.contains("created") || resp_lower.contains("wrote")
    {
        suggestions.push(Suggestion {
            label: "Review changes".to_string(),
            prompt: "Show me a summary of all the changes that were made.".to_string(),
            category: SuggestionCategory::FollowUp,
            relevance: 0.8,
        });
    }

    if msg_lower.contains("search") || msg_lower.contains("find") {
        suggestions.push(Suggestion {
            label: "Refine search".to_string(),
            prompt: "Can you narrow down the search with more specific criteria?".to_string(),
            category: SuggestionCategory::Refine,
            relevance: 0.7,
        });
    }

    if available_tools.iter().any(|t| t.contains("memory")) && msg_lower.len() > 100 {
        suggestions.push(Suggestion {
            label: "Save to memory".to_string(),
            prompt: "Please save the key points from this conversation to memory.".to_string(),
            category: SuggestionCategory::Action,
            relevance: 0.6,
        });
    }

    if resp_lower.contains("code") || resp_lower.contains("function") || resp_lower.contains("impl")
    {
        suggestions.push(Suggestion {
            label: "Add tests".to_string(),
            prompt: "Can you write tests for the code we just discussed?".to_string(),
            category: SuggestionCategory::Action,
            relevance: 0.7,
        });
    }

    if resp_lower.contains("todo")
        || resp_lower.contains("next step")
        || resp_lower.contains("remaining")
    {
        suggestions.push(Suggestion {
            label: "Continue work".to_string(),
            prompt: "Please continue with the next pending task.".to_string(),
            category: SuggestionCategory::FollowUp,
            relevance: 0.85,
        });
    }

    suggestions.sort_by(|a, b| {
        b.relevance
            .partial_cmp(&a.relevance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    suggestions.truncate(config.max_suggestions);
    suggestions
}
