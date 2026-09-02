// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::hash_map::{DefaultHasher, Entry};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

pub type LoopDetectionCallback = Box<dyn Fn(&str) + Send + Sync>;

pub const DEFAULT_IDENTICAL_OUTPUT_THRESHOLD: u32 = 5;

pub const IDENTICAL_TEXT_RESPONSE_ABORT_STREAK: u32 = 2;

pub const STREAM_REPEAT_LINE_MIN_CHARS: usize = 24;

pub const STREAM_REPEAT_LINE_COUNT: usize = 3;

pub fn text_response_fingerprint(text: &str) -> Option<u64> {
    let mut hasher = DefaultHasher::new();
    let mut words = 0usize;
    for word in text.split_whitespace() {
        word.to_lowercase().hash(&mut hasher);
        words += 1;
    }
    (words > 0).then(|| hasher.finish())
}

pub fn detect_trailing_line_repetition(text: &str) -> Option<usize> {
    let completed_end = text.rfind('\n')?;
    let completed = &text[..completed_end];
    if completed.matches("```").count() % 2 == 1 {
        return None;
    }
    let mut tail: Vec<(usize, &str)> = Vec::with_capacity(STREAM_REPEAT_LINE_COUNT);
    let mut segment_end = completed.len();
    for line in completed.rsplit('\n') {
        let start = segment_end - line.len();
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            tail.push((start, trimmed));
            if tail.len() == STREAM_REPEAT_LINE_COUNT {
                break;
            }
        }
        segment_end = start.saturating_sub(1);
    }
    if tail.len() < STREAM_REPEAT_LINE_COUNT {
        return None;
    }
    let reference = text_response_fingerprint(tail[0].1)?;
    if tail[0].1.chars().count() < STREAM_REPEAT_LINE_MIN_CHARS {
        return None;
    }
    if tail
        .iter()
        .skip(1)
        .any(|(_, line)| text_response_fingerprint(line) != Some(reference))
    {
        return None;
    }
    Some(tail[STREAM_REPEAT_LINE_COUNT - 2].0)
}

pub fn repeated_text_response_error(model: &str, repeats: u32, context: &str) -> String {
    format!(
        "repeated_model_response: the model '{model}' produced the same reply {attempts} times in a row without making progress ({context}); \
         the turn was aborted to stop the loop. Likely causes: the output token limit (max_tokens) is too small for the model's reasoning plus its answer, \
         or the model is not honoring the required tool call. Try raising the output limit, disabling extended thinking, or switching models.",
        attempts = repeats + 1,
    )
}

enum SeenSignature {
    Pending,
    Completed(Option<String>),
}

pub struct LoopControlState {

    pub loop_detector: crate::agent::loop_::detector::LoopDetector,

    last_tool_output_hash: Option<u64>,

    consecutive_identical_outputs: u32,

    last_text_response_fingerprint: Option<u64>,

    identical_text_response_streak: u32,

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
            last_text_response_fingerprint: None,
            identical_text_response_streak: 0,
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
        self.seen_tool_signatures.retain(|(name, _), _| {
            !matches!(
                name.as_str(),
                "file_read" | "content_search" | "dir_list" | "file_list" | "codebase_search"
            )
        });
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

    pub fn reset(&mut self) {
        self.last_tool_output_hash = None;
        self.consecutive_identical_outputs = 0;
        self.reset_text_response_streak();
        self.seen_tool_signatures.clear();
        self.coverage.reset();
        self.loop_detector.reset();
    }

    pub fn note_text_response(&mut self, text: &str) -> u32 {
        let Some(fingerprint) = text_response_fingerprint(text) else {
            self.reset_text_response_streak();
            return 0;
        };
        if self.last_text_response_fingerprint == Some(fingerprint) {
            self.identical_text_response_streak =
                self.identical_text_response_streak.saturating_add(1);
        } else {
            self.identical_text_response_streak = 0;
            self.last_text_response_fingerprint = Some(fingerprint);
        }
        self.identical_text_response_streak
    }

    pub fn reset_text_response_streak(&mut self) {
        self.last_text_response_fingerprint = None;
        self.identical_text_response_streak = 0;
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
