// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Compact service — mirrors claude-code-typescript-src`services/compact/`.
// Handles conversation compaction when the context window is nearly full.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactStrategy {

    Summarize,

    Truncate,

    Hybrid,

    Microcompact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactOptions {
    pub strategy: CompactStrategy,

    pub target_utilization: f64,

    pub preserve_recent_turns: usize,

    pub preserve_skills: bool,

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactResult {
    pub messages_before: usize,
    pub messages_after: usize,
    pub tokens_before: u64,
    pub tokens_after: u64,
    pub summary: Option<String>,
    pub strategy_used: CompactStrategy,
}

pub struct CompactService;

impl CompactService {

    pub fn should_compact(utilization: f64, threshold: f64) -> bool {
        utilization > threshold
    }

    pub fn choose_strategy(utilization: f64, turn_count: usize) -> CompactStrategy {
        if utilization > 0.95 {

            CompactStrategy::Truncate
        } else if turn_count < 10 {

            CompactStrategy::Microcompact
        } else if utilization > 0.8 {
            CompactStrategy::Hybrid
        } else {
            CompactStrategy::Summarize
        }
    }

    pub fn default_summary_prompt() -> &'static str {
        "Summarize the conversation so far in a concise way that preserves \
         all important context, decisions made, file paths mentioned, code \
         changes performed, and any pending tasks. Focus on information the \
         assistant will need to continue helping effectively."
    }
}
