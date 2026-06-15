// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactStrategy {

    Summarize,

    Truncate,

    Hybrid,

    Microcompact,
}

pub struct CompactService;

impl CompactService {

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
