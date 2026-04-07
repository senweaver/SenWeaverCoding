// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Compact service — mirrors claude-code-typescript-src`services/compact/`.
// Handles conversation compaction when the context window is nearly full.

use serde::{Deserialize, Serialize};

/// Strategy for compaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactStrategy {
    /// Summarise old turns, keep recent ones verbatim.
    Summarize,
    /// Drop old turns entirely, keep recent ones.
    Truncate,
    /// Combine: summarise oldest, truncate middle, keep recent.
    Hybrid,
    /// Cache-aware microcompact — strip thinking from old turns.
    Microcompact,
}

/// Options for a compaction run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactOptions {
    pub strategy: CompactStrategy,
    /// Fraction of context window to target after compaction (0.0–1.0).
    pub target_utilization: f64,
    /// Number of recent turns to preserve verbatim.
    pub preserve_recent_turns: usize,
    /// Whether to preserve skill invocation context.
    pub preserve_skills: bool,
    /// Custom summary prompt (overrides default).
    pub summary_prompt: Option<String>,
}

impl Default for CompactOptions {
    fn default() -> Self {
        Self {
            strategy: CompactStrategy::Hybrid,
            target_utilization: 0.5,
            preserve_recent_turns: 4,
            preserve_skills: true,
            summary_prompt: None,
        }
    }
}

/// Result of a compaction operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactResult {
    pub messages_before: usize,
    pub messages_after: usize,
    pub tokens_before: u64,
    pub tokens_after: u64,
    pub summary: Option<String>,
    pub strategy_used: CompactStrategy,
}

/// Service that performs context compaction.
pub struct CompactService;

impl CompactService {
    /// Determine whether compaction is needed given current utilization.
    pub fn should_compact(utilization: f64, threshold: f64) -> bool {
        utilization > threshold
    }

    /// Choose the best compaction strategy based on context state.
    pub fn choose_strategy(utilization: f64, turn_count: usize) -> CompactStrategy {
        if utilization > 0.95 {
            // Emergency — aggressive truncation
            CompactStrategy::Truncate
        } else if turn_count < 10 {
            // Few turns — microcompact (strip thinking)
            CompactStrategy::Microcompact
        } else if utilization > 0.8 {
            CompactStrategy::Hybrid
        } else {
            CompactStrategy::Summarize
        }
    }

    /// Build the default summary prompt for the compaction model call.
    pub fn default_summary_prompt() -> &'static str {
        "Summarize the conversation so far in a concise way that preserves \
         all important context, decisions made, file paths mentioned, code \
         changes performed, and any pending tasks. Focus on information the \
         assistant will need to continue helping effectively."
    }
}
