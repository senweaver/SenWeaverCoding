// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::agent::executor_core::ToolLoopDedup;
use crate::agent::loop_detector::{LoopDetector, LoopDetectorConfig};

pub struct TurnLoopState {

    pub(crate) dedup: ToolLoopDedup,

    pub(crate) loop_detector: LoopDetector,

    pub(crate) last_output_hash: Option<u64>,

    pub(crate) consecutive_identical: u32,

    pub(crate) tools_used: Vec<String>,

    pub(crate) tool_results: Vec<(String, bool)>,

    pub(crate) token_counter: u64,

    pub(crate) had_file_edit: bool,
}

impl TurnLoopState {

    pub fn new() -> Self {
        Self {
            dedup: ToolLoopDedup::new(),
            loop_detector: LoopDetector::new(LoopDetectorConfig::default()),
            last_output_hash: None,
            consecutive_identical: 0,
            tools_used: Vec::new(),
            tool_results: Vec::new(),
            token_counter: 0,
            had_file_edit: false,
        }
    }

    pub(crate) fn with_loop_detector_config(cfg: LoopDetectorConfig) -> Self {
        Self {
            loop_detector: LoopDetector::new(cfg),
            ..Self::new()
        }
    }

    pub fn record_tool_outcome(&mut self, name: impl Into<String>, success: bool) {
        let name = name.into();
        self.tools_used.push(name.clone());
        self.tool_results.push((name, success));
    }

    pub fn record_output_hash(&mut self, hash: u64, threshold: u32) -> bool {
        if self.last_output_hash == Some(hash) {
            self.consecutive_identical += 1;
        } else {
            self.last_output_hash = Some(hash);
            self.consecutive_identical = 1;
        }
        self.consecutive_identical >= threshold
    }

    pub fn add_tokens(&mut self, n: u64) {
        self.token_counter = self.token_counter.saturating_add(n);
    }

    pub fn mark_file_edit(&mut self) {
        self.had_file_edit = true;
    }

    pub fn token_counter(&self) -> u64 {
        self.token_counter
    }

    pub fn had_file_edit(&self) -> bool {
        self.had_file_edit
    }

    pub fn tools_used(&self) -> &[String] {
        &self.tools_used
    }

    pub fn tool_results(&self) -> &[(String, bool)] {
        &self.tool_results
    }
}

impl Default for TurnLoopState {
    fn default() -> Self {
        Self::new()
    }
}
