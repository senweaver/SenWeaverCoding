// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Shared loop control state used by both [`AgentLoopCore::run_streamed`]
//! and [`super::Agent::turn_streamed`] to ensure behavioural parity.
//!
//! ## Why shared state?
//!
//! Both the canonical `run_tool_call_loop` path and the agent's streaming
//! path need identical loop-control logic: loop detection, identical-output
//! hashing, seen-tool-signature tracking.  Previously this state was
//! duplicated in both call sites, making it easy for one path to drift
//! from the other.
//!
//! This module centralises that state so any future change to loop-control
//! policy only needs to be made in one place.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};

pub type LoopDetectionCallback = Box<dyn Fn(&str) + Send + Sync>;

pub const DEFAULT_IDENTICAL_OUTPUT_THRESHOLD: u32 = 5;

pub struct LoopControlState {

    pub loop_detector: crate::agent::loop_detector::LoopDetector,

    last_tool_output_hash: Option<u64>,

    consecutive_identical_outputs: u32,

    seen_tool_signatures: HashSet<(String, String)>,

    identical_output_threshold: u32,

    notification_callback: Option<LoopDetectionCallback>,
}

impl Default for LoopControlState {
    fn default() -> Self {
        Self::new(
            crate::agent::loop_detector::LoopDetectorConfig::default(),
            DEFAULT_IDENTICAL_OUTPUT_THRESHOLD,
        )
    }
}

impl LoopControlState {

    pub fn new(
        config: crate::agent::loop_detector::LoopDetectorConfig,
        identical_output_threshold: u32,
    ) -> Self {
        let threshold = if identical_output_threshold == 0 {
            DEFAULT_IDENTICAL_OUTPUT_THRESHOLD
        } else {
            identical_output_threshold
        };
        Self {
            loop_detector: crate::agent::loop_detector::LoopDetector::new(config),
            last_tool_output_hash: None,
            consecutive_identical_outputs: 0,
            seen_tool_signatures: HashSet::new(),
            identical_output_threshold: threshold,
            notification_callback: None,
        }
    }

    pub fn with_callback(mut self, cb: LoopDetectionCallback) -> Self {
        self.notification_callback = Some(cb);
        self
    }

    fn notify(&self, msg: &str) {
        if let Some(cb) = self.notification_callback.as_ref() {
            cb(msg);
        }
    }

    pub fn record_tool_signature(&mut self, name: &str, arguments: &str) -> bool {
        !self
            .seen_tool_signatures
            .insert((name.to_string(), arguments.to_string()))
    }

    pub fn record_tool_results_with_args(
        &mut self,
        results: &[(String, serde_json::Value, String)],
    ) -> Result<Option<String>, String> {
        let mut has_payload = false;
        let mut fingerprint_hasher = std::collections::hash_map::DefaultHasher::new();
        let mut notification: Option<String> = None;

        for (name, args, output) in results {
            has_payload = true;
            name.hash(&mut fingerprint_hasher);
            crate::agent::loop_detector::canonicalise_args_string(args).hash(&mut fingerprint_hasher);
            output.hash(&mut fingerprint_hasher);
            let det = self.loop_detector.record(name, args, output);
            match det {
                crate::agent::loop_detector::LoopDetectionResult::Warning(msg) => {
                    tracing::warn!("[Loop Detection] {msg}");
                    self.notify(&msg);
                    notification = Some(msg);
                }
                crate::agent::loop_detector::LoopDetectionResult::Block(msg) => {
                    tracing::warn!("[Loop Detection — BLOCKED] {msg}");
                    let formatted = format!("[Loop Detection — BLOCKED] {msg}");
                    self.notify(&formatted);
                    notification = Some(formatted);
                }
                crate::agent::loop_detector::LoopDetectionResult::Break(msg) => {
                    let formatted = format!("Agent loop aborted by loop detector: {msg}");
                    self.notify(&formatted);
                    return Err(formatted);
                }
                _ => {}
            }
        }

        if has_payload {
            let current_hash = fingerprint_hasher.finish();
            if self.last_tool_output_hash == Some(current_hash) {
                self.consecutive_identical_outputs += 1;
            } else {
                self.consecutive_identical_outputs = 0;
                self.last_tool_output_hash = Some(current_hash);
            }
            if self.consecutive_identical_outputs >= self.identical_output_threshold {
                return Err(format!(
                    "Agent loop aborted: identical tool call (name + arguments + output) detected {} consecutive times",
                    self.consecutive_identical_outputs
                ));
            }
        }
        Ok(notification)
    }

    pub fn record_tool_results(
        &mut self,
        results: &[(String, String)],
    ) -> Result<Option<String>, String> {
        let mut has_payload = false;
        let mut fingerprint_hasher = std::collections::hash_map::DefaultHasher::new();
        let mut notification: Option<String> = None;

        for (name, output) in results {
            has_payload = true;
            name.hash(&mut fingerprint_hasher);
            output.hash(&mut fingerprint_hasher);
            let det = self
                .loop_detector
                .record(name, &serde_json::Value::Null, output);
            match det {
                crate::agent::loop_detector::LoopDetectionResult::Warning(msg) => {
                    tracing::warn!("[Loop Detection] {msg}");
                    self.notify(&msg);
                    notification = Some(msg);
                }
                crate::agent::loop_detector::LoopDetectionResult::Block(msg) => {
                    tracing::warn!("[Loop Detection — BLOCKED] {msg}");
                    let formatted = format!("[Loop Detection — BLOCKED] {msg}");
                    self.notify(&formatted);
                    notification = Some(formatted);
                }
                crate::agent::loop_detector::LoopDetectionResult::Break(msg) => {
                    let formatted = format!("Agent loop aborted by loop detector: {msg}");
                    self.notify(&formatted);
                    return Err(formatted);
                }
                _ => {}
            }
        }

        if has_payload {
            let current_hash = fingerprint_hasher.finish();
            if self.last_tool_output_hash == Some(current_hash) {
                self.consecutive_identical_outputs += 1;
            } else {
                self.consecutive_identical_outputs = 0;
                self.last_tool_output_hash = Some(current_hash);
            }
            if self.consecutive_identical_outputs >= self.identical_output_threshold {
                return Err(format!(
                    "Agent loop aborted: identical tool call (name + output) detected {} consecutive times",
                    self.consecutive_identical_outputs
                ));
            }
        }
        Ok(notification)
    }

    pub fn reset(&mut self) {
        self.last_tool_output_hash = None;
        self.consecutive_identical_outputs = 0;
        self.seen_tool_signatures.clear();
    }
}
