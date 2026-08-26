// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

pub type LoopDetectionCallback = Box<dyn Fn(&str) + Send + Sync>;

pub const DEFAULT_IDENTICAL_OUTPUT_THRESHOLD: u32 = 5;

enum SeenSignature {
    Pending,
    Completed(Option<String>),
}

pub struct LoopControlState {

    pub loop_detector: crate::agent::loop_::detector::LoopDetector,

    last_tool_output_hash: Option<u64>,

    consecutive_identical_outputs: u32,

    seen_tool_signatures: HashMap<(String, String), SeenSignature>,

    pub coverage: crate::agent::loop_::coverage::CoverageLedger,

    identical_output_threshold: u32,

    notification_callback: Option<LoopDetectionCallback>,
}

impl Default for LoopControlState {
    fn default() -> Self {
        Self::new(
            crate::agent::loop_::detector::LoopDetectorConfig::default(),
            DEFAULT_IDENTICAL_OUTPUT_THRESHOLD,
        )
    }
}

impl LoopControlState {

    pub fn new(
        config: crate::agent::loop_::detector::LoopDetectorConfig,
        identical_output_threshold: u32,
    ) -> Self {
        let threshold = if identical_output_threshold == 0 {
            DEFAULT_IDENTICAL_OUTPUT_THRESHOLD
        } else {
            identical_output_threshold
        };
        Self {
            loop_detector: crate::agent::loop_::detector::LoopDetector::new(config),
            last_tool_output_hash: None,
            consecutive_identical_outputs: 0,
            seen_tool_signatures: HashMap::new(),
            coverage: crate::agent::loop_::coverage::CoverageLedger::new(),
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

    pub fn has_completed_signature(&self, name: &str, arguments: &str) -> bool {
        matches!(
            self.seen_tool_signatures
                .get(&(name.to_string(), arguments.to_string())),
            Some(SeenSignature::Completed(_))
        )
    }

    pub fn completed_signature_blob(&self, name: &str, arguments: &str) -> Option<&str> {
        match self
            .seen_tool_signatures
            .get(&(name.to_string(), arguments.to_string()))?
        {
            SeenSignature::Completed(blob) => blob.as_deref(),
            SeenSignature::Pending => None,
        }
    }

    pub fn claim_signature(&mut self, name: &str, arguments: &str) -> bool {
        match self
            .seen_tool_signatures
            .entry((name.to_string(), arguments.to_string()))
        {
            Entry::Occupied(_) => true,
            Entry::Vacant(slot) => {
                slot.insert(SeenSignature::Pending);
                false
            }
        }
    }

    pub fn complete_signature(&mut self, name: &str, arguments: &str, blob_id: Option<String>) {
        self.seen_tool_signatures.insert(
            (name.to_string(), arguments.to_string()),
            SeenSignature::Completed(blob_id),
        );
    }

    pub fn release_pending_signature(&mut self, name: &str, arguments: &str) {
        let key = (name.to_string(), arguments.to_string());
        if matches!(
            self.seen_tool_signatures.get(&key),
            Some(SeenSignature::Pending)
        ) {
            self.seen_tool_signatures.remove(&key);
        }
    }

    pub fn invalidate_read_signatures(&mut self) {
        self.seen_tool_signatures
            .retain(|(name, _), _| name != "file_read" && name != "content_search");
    }

    pub fn peek_window_repeat_count(&self, name: &str, canonical_args: &str) -> usize {
        self.loop_detector.peek_window_count_hashed(
            name,
            crate::agent::loop_::detector::hash_canonical_str(canonical_args),
        )
    }

    pub fn loop_block_threshold(&self) -> usize {
        self.loop_detector.max_repeats()
    }

    pub fn loop_detection_enabled(&self) -> bool {
        self.loop_detector.is_enabled()
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
            let canonical_args =
                crate::agent::loop_::detector::canonicalise_args_string(args);
            name.hash(&mut fingerprint_hasher);
            canonical_args.hash(&mut fingerprint_hasher);
            output.hash(&mut fingerprint_hasher);
            let det = self.loop_detector.record_with_failure_hashed(
                name,
                crate::agent::loop_::detector::hash_canonical_str(&canonical_args),
                output,
                false,
            );
            match det {
                crate::agent::loop_::detector::LoopDetectionResult::Warning(msg) => {
                    tracing::warn!("[Loop Detection] {msg}");
                    self.notify(&msg);
                    notification = Some(msg);
                }
                crate::agent::loop_::detector::LoopDetectionResult::Block(msg) => {
                    tracing::warn!("[Loop Detection  - BLOCKED] {msg}");
                    let formatted = format!("[Loop Detection  - BLOCKED] {msg}");
                    self.notify(&formatted);
                    notification = Some(formatted);
                }
                crate::agent::loop_::detector::LoopDetectionResult::Break(msg) => {
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
                crate::agent::loop_::detector::LoopDetectionResult::Warning(msg) => {
                    tracing::warn!("[Loop Detection] {msg}");
                    self.notify(&msg);
                    notification = Some(msg);
                }
                crate::agent::loop_::detector::LoopDetectionResult::Block(msg) => {
                    tracing::warn!("[Loop Detection  - BLOCKED] {msg}");
                    let formatted = format!("[Loop Detection  - BLOCKED] {msg}");
                    self.notify(&formatted);
                    notification = Some(formatted);
                }
                crate::agent::loop_::detector::LoopDetectionResult::Break(msg) => {
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
        self.coverage.reset();
        self.loop_detector.reset();
    }

    pub fn record_per_tool(
        &mut self,
        name: &str,
        args: &serde_json::Value,
        output: &str,
    ) -> crate::agent::loop_::detector::LoopDetectionResult {
        self.record_per_tool_with_failure(name, args, output, false)
    }

    pub fn record_per_tool_with_failure(
        &mut self,
        name: &str,
        args: &serde_json::Value,
        output: &str,
        was_failure: bool,
    ) -> crate::agent::loop_::detector::LoopDetectionResult {
        let result = self
            .loop_detector
            .record_with_failure(name, args, output, was_failure);
        self.notify_detection(&result);
        result
    }

    pub fn record_per_tool_with_failure_canonical(
        &mut self,
        name: &str,
        canonical_args: &str,
        output: &str,
        was_failure: bool,
    ) -> crate::agent::loop_::detector::LoopDetectionResult {
        let result = self.loop_detector.record_with_failure_hashed(
            name,
            crate::agent::loop_::detector::hash_canonical_str(canonical_args),
            output,
            was_failure,
        );
        self.notify_detection(&result);
        result
    }

    fn notify_detection(
        &self,
        result: &crate::agent::loop_::detector::LoopDetectionResult,
    ) {
        match result {
            crate::agent::loop_::detector::LoopDetectionResult::Warning(msg) => {
                self.notify(msg);
            }
            crate::agent::loop_::detector::LoopDetectionResult::Block(msg) => {
                self.notify(&format!("[Loop Detection  - BLOCKED] {msg}"));
            }
            crate::agent::loop_::detector::LoopDetectionResult::Break(msg) => {
                self.notify(&format!("Agent loop aborted by loop detector: {msg}"));
            }
            _ => {}
        }
    }

    pub fn consecutive_identical_failures(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> usize {
        self.loop_detector.peek_consecutive_failures(name, args)
    }

    pub fn consecutive_identical_failures_canonical(
        &self,
        name: &str,
        canonical_args: &str,
    ) -> usize {
        self.loop_detector.peek_consecutive_failures_hashed(
            name,
            crate::agent::loop_::detector::hash_canonical_str(canonical_args),
        )
    }

    pub fn check_iteration_fingerprint(
        &mut self,
        fingerprint: u64,
    ) -> Result<(), String> {
        if self.last_tool_output_hash == Some(fingerprint) {
            self.consecutive_identical_outputs += 1;
        } else {
            self.consecutive_identical_outputs = 0;
            self.last_tool_output_hash = Some(fingerprint);
        }
        if self.consecutive_identical_outputs >= self.identical_output_threshold {
            return Err(format!(
                "identical tool call (name + arguments + output) detected {} consecutive times",
                self.consecutive_identical_outputs
            ));
        }
        Ok(())
    }

    #[must_use]
    pub fn identical_output_threshold(&self) -> u32 {
        self.identical_output_threshold
    }

    #[must_use]
    pub fn consecutive_identical_outputs(&self) -> u32 {
        self.consecutive_identical_outputs
    }
}
