// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Prompt suggestion service — mirrors claude-code-typescript-src`services/PromptSuggestion/`.
// Provides context-aware prompt suggestions based on project state,
// recent activity, and available tools.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptSuggestion {
    pub text: String,
    pub category: SuggestionCategory,
    pub relevance_score: f64,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionCategory {
    QuickAction,
    FileOperation,
    GitOperation,
    CodeReview,
    Debugging,
    Testing,
    Documentation,
    Refactoring,
    ProjectSetup,
}

pub struct PromptSuggestionService;

impl PromptSuggestionService {

    pub fn suggest(signals: &ContextSignals) -> Vec<PromptSuggestion> {
        let mut suggestions = Vec::new();

        if signals.has_uncommitted_changes {
            suggestions.push(PromptSuggestion {
                text: "Review and commit my changes".to_string(),
                category: SuggestionCategory::GitOperation,
                relevance_score: 0.9,
                description: "You have uncommitted changes".to_string(),
            });
        }
        if signals.has_merge_conflicts {
            suggestions.push(PromptSuggestion {
                text: "Help me resolve merge conflicts".to_string(),
                category: SuggestionCategory::GitOperation,
                relevance_score: 0.95,
                description: "Merge conflicts detected".to_string(),
            });
        }

        if signals.has_test_failures {
            suggestions.push(PromptSuggestion {
                text: "Fix the failing tests".to_string(),
                category: SuggestionCategory::Testing,
                relevance_score: 0.92,
                description: "Test failures detected".to_string(),
            });
        }
        if signals.has_lint_errors {
            suggestions.push(PromptSuggestion {
                text: "Fix lint errors in the project".to_string(),
                category: SuggestionCategory::Debugging,
                relevance_score: 0.85,
                description: "Lint errors found".to_string(),
            });
        }

        if signals.last_tool_was_file_edit {
            suggestions.push(PromptSuggestion {
                text: "Review the changes I just made".to_string(),
                category: SuggestionCategory::CodeReview,
                relevance_score: 0.7,
                description: "Review recent file edits".to_string(),
            });
        }

        if suggestions.is_empty() {
            suggestions.push(PromptSuggestion {
                text: "What can I help you with?".to_string(),
                category: SuggestionCategory::QuickAction,
                relevance_score: 0.5,
                description: "General assistance".to_string(),
            });
            suggestions.push(PromptSuggestion {
                text: "Explain this codebase".to_string(),
                category: SuggestionCategory::Documentation,
                relevance_score: 0.4,
                description: "Get an overview of the project".to_string(),
            });
        }

        suggestions.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        suggestions
    }
}

#[derive(Debug, Clone, Default)]
pub struct ContextSignals {
    pub has_uncommitted_changes: bool,
    pub has_merge_conflicts: bool,
    pub has_test_failures: bool,
    pub has_lint_errors: bool,
    pub last_tool_was_file_edit: bool,
    pub files_open: Vec<String>,
    pub recent_errors: Vec<String>,
}
