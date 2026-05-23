// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::VecDeque;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone)]
pub struct LoopDetectorConfig {

    pub enabled: bool,

    pub window_size: usize,

    pub max_repeats: usize,
}

impl Default for LoopDetectorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            window_size: 20,
            max_repeats: 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopDetectionResult {

    Ok,

    Warning(String),

    Block(String),

    Break(String),
}

#[derive(Debug, Clone)]
struct ToolCallRecord {

    name: String,

    args_hash: u64,

    result_hash: u64,
}

fn hash_value(value: &serde_json::Value) -> u64 {
    let mut hasher = DefaultHasher::new();
    let canonical = serde_json::to_string(&canonicalise(value)).unwrap_or_default();
    canonical.hash(&mut hasher);
    hasher.finish()
}

pub(crate) fn canonicalise_args_string(value: &serde_json::Value) -> String {
    serde_json::to_string(&canonicalise(value)).unwrap_or_default()
}

fn canonicalise(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut sorted: Vec<(&String, &serde_json::Value)> = map.iter().collect();
            sorted.sort_by_key(|(k, _)| *k);
            let new_map: serde_json::Map<String, serde_json::Value> = sorted
                .into_iter()
                .map(|(k, v)| (k.clone(), canonicalise(v)))
                .collect();
            serde_json::Value::Object(new_map)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(canonicalise).collect())
        }
        other => other.clone(),
    }
}

fn hash_str(s: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

pub struct LoopDetector {
    config: LoopDetectorConfig,
    window: VecDeque<ToolCallRecord>,
}

impl LoopDetector {
    pub fn new(config: LoopDetectorConfig) -> Self {
        Self {
            window: VecDeque::with_capacity(config.window_size),
            config,
        }
    }

    pub fn record(
        &mut self,
        name: &str,
        args: &serde_json::Value,
        result: &str,
    ) -> LoopDetectionResult {
        if !self.config.enabled {
            return LoopDetectionResult::Ok;
        }

        let record = ToolCallRecord {
            name: name.to_string(),
            args_hash: hash_value(args),
            result_hash: hash_str(result),
        };

        if self.window.len() >= self.config.window_size {
            self.window.pop_front();
        }
        self.window.push_back(record);

        if let Some(result) = self.detect_exact_repeat() {
            return result;
        }
        if let Some(result) = self.detect_ping_pong() {
            return result;
        }
        if let Some(result) = self.detect_no_progress() {
            return result;
        }

        LoopDetectionResult::Ok
    }

    fn detect_exact_repeat(&self) -> Option<LoopDetectionResult> {
        let max = self.config.max_repeats;
        if self.window.len() < max {
            return None;
        }

        let last = self.window.back()?;
        let consecutive_records: Vec<&ToolCallRecord> = self
            .window
            .iter()
            .rev()
            .take_while(|r| r.name == last.name && r.args_hash == last.args_hash)
            .collect();
        let consecutive = consecutive_records.len();

        let unique_results: std::collections::HashSet<u64> = consecutive_records
            .iter()
            .map(|r| r.result_hash)
            .collect();
        let result_is_evolving = unique_results.len() > 1;

        if consecutive >= max + 2 {
            if result_is_evolving {
                Some(LoopDetectionResult::Warning(format!(
                    "Warning: tool '{}' called {} times consecutively with identical arguments, \
                     but the result is still changing — likely polling for state. Continuing.",
                    last.name, consecutive
                )))
            } else {
                Some(LoopDetectionResult::Break(format!(
                    "Circuit breaker: tool '{}' called {} times consecutively with identical arguments and identical results",
                    last.name, consecutive
                )))
            }
        } else if consecutive > max {
            if result_is_evolving {
                Some(LoopDetectionResult::Warning(format!(
                    "Warning: tool '{}' called {} times consecutively with identical arguments \
                     (result still changing).",
                    last.name, consecutive
                )))
            } else {
                Some(LoopDetectionResult::Block(format!(
                    "Blocked: tool '{}' called {} times consecutively with identical arguments and identical results",
                    last.name, consecutive
                )))
            }
        } else if consecutive >= max {
            Some(LoopDetectionResult::Warning(format!(
                "Warning: tool '{}' has been called {} times consecutively with identical arguments. \
                 Try a different approach.",
                last.name, consecutive
            )))
        } else {
            None
        }
    }

    fn detect_ping_pong(&self) -> Option<LoopDetectionResult> {
        const MIN_CYCLES: usize = 4;
        let needed = MIN_CYCLES * 2;

        if self.window.len() < needed {
            return None;
        }

        let tail: Vec<&ToolCallRecord> = self.window.iter().rev().take(needed).collect();

        let a_name = &tail[0].name;
        let a_args_hash = tail[0].args_hash;
        let b_name = &tail[1].name;
        let b_args_hash = tail[1].args_hash;

        if a_name == b_name {
            return None;
        }

        let is_ping_pong = tail.iter().enumerate().all(|(i, r)| {
            if i % 2 == 0 {
                &r.name == a_name && r.args_hash == a_args_hash
            } else {
                &r.name == b_name && r.args_hash == b_args_hash
            }
        });

        if !is_ping_pong {
            return None;
        }

        let mut cycles = MIN_CYCLES;
        let extended: Vec<&ToolCallRecord> = self.window.iter().rev().collect();
        for extra_pair in extended.chunks(2).skip(MIN_CYCLES) {
            if extra_pair.len() == 2
                && &extra_pair[0].name == a_name
                && extra_pair[0].args_hash == a_args_hash
                && &extra_pair[1].name == b_name
                && extra_pair[1].args_hash == b_args_hash
            {
                cycles += 1;
            } else {
                break;
            }
        }

        if cycles >= MIN_CYCLES + 2 {
            Some(LoopDetectionResult::Break(format!(
                "Circuit breaker: tools '{}' and '{}' have been alternating with identical arguments for {} cycles",
                a_name, b_name, cycles
            )))
        } else if cycles > MIN_CYCLES {
            Some(LoopDetectionResult::Block(format!(
                "Blocked: tools '{}' and '{}' have been alternating with identical arguments for {} cycles",
                a_name, b_name, cycles
            )))
        } else {
            Some(LoopDetectionResult::Warning(format!(
                "Warning: tools '{}' and '{}' appear to be alternating with identical arguments ({} cycles). \
                 Consider a different strategy.",
                a_name, b_name, cycles
            )))
        }
    }

    fn detect_no_progress(&self) -> Option<LoopDetectionResult> {
        const MIN_CALLS: usize = 5;

        if self.window.len() < MIN_CALLS {
            return None;
        }

        let last = self.window.back()?;
        let same_tool_same_result: Vec<&ToolCallRecord> = self
            .window
            .iter()
            .rev()
            .take_while(|r| r.name == last.name && r.result_hash == last.result_hash)
            .collect();

        let count = same_tool_same_result.len();
        if count < MIN_CALLS {
            return None;
        }

        let unique_args: std::collections::HashSet<u64> =
            same_tool_same_result.iter().map(|r| r.args_hash).collect();
        if unique_args.len() < 2 {

            return None;
        }

        if count >= MIN_CALLS + 2 {
            Some(LoopDetectionResult::Break(format!(
                "Circuit breaker: tool '{}' called {} times with different arguments but identical results — no progress",
                last.name, count
            )))
        } else if count > MIN_CALLS {
            Some(LoopDetectionResult::Block(format!(
                "Blocked: tool '{}' called {} times with different arguments but identical results",
                last.name, count
            )))
        } else {
            Some(LoopDetectionResult::Warning(format!(
                "Warning: tool '{}' called {} times with different arguments but identical results. \
                 The current approach may not be making progress.",
                last.name, count
            )))
        }
    }
}
