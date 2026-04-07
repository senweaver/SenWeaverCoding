// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Suggestions Engine - contextual next-action suggestions for agent sessions.
//!
//! Analyzes conversation history and available tools to suggest
//! relevant follow-up actions the user might want to take.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Suggestions configuration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SuggestionsConfig {
    /// Enable suggestions generation. Default: true.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Maximum number of suggestions to generate. Default: 4.
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

/// A single suggestion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    /// Short label for the suggestion.
    pub label: String,
    /// Full prompt text to execute if selected.
    pub prompt: String,
    /// Category of the suggestion.
    pub category: SuggestionCategory,
    /// Relevance score (0.0 - 1.0).
    pub relevance: f64,
}

/// Categories of suggestions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionCategory {
    /// Follow-up question.
    FollowUp,
    /// Deeper exploration.
    Explore,
    /// Related task.
    Action,
    /// Correction or refinement.
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

/// Generate suggestions based on conversation context.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_suggestions() {
        let suggestions = generate_rule_based_suggestions(
            "fix the bug",
            "I found an error in the code: undefined variable",
            &[],
            &SuggestionsConfig::default(),
        );
        assert!(!suggestions.is_empty());
        assert!(suggestions.iter().any(|s| s.label.contains("Debug")));
    }

    #[test]
    fn test_memory_suggestions() {
        let long_msg = "a".repeat(200);
        let suggestions = generate_rule_based_suggestions(
            &long_msg,
            "Here is the result",
            &["memory_store".to_string(), "memory_recall".to_string()],
            &SuggestionsConfig::default(),
        );
        assert!(suggestions.iter().any(|s| s.label.contains("memory")));
    }

    #[test]
    fn test_disabled() {
        let config = SuggestionsConfig {
            enabled: false,
            ..Default::default()
        };
        let suggestions = generate_rule_based_suggestions("hello", "world", &[], &config);
        assert!(suggestions.is_empty());
    }

    #[test]
    fn test_max_suggestions() {
        let config = SuggestionsConfig {
            max_suggestions: 1,
            ..Default::default()
        };
        let suggestions = generate_rule_based_suggestions(
            "search for files",
            "I found an error in the file and created a new function with code",
            &["memory_store".to_string()],
            &config,
        );
        assert!(suggestions.len() <= 1);
    }
}
