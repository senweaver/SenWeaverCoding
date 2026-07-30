// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use std::sync::LazyLock;
use std::time::Duration;

use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::providers::traits::{ChatMessage, Provider};
use crate::agent::history::compaction::{
    build_compaction_transcript_full, estimate_tokens_filtered,
    replace_history_range_with_assistant, split_transcript_chunks,
};

const SUMMARY_BANNER_PREFIX: &str = "[CONTEXT SUMMARY \u{2014}";
const COMPACTION_BANNER_PREFIX: &str = "[CONTEXT COMPACTION \u{2014}";

fn default_enabled() -> bool {
    true
}
fn default_threshold_ratio() -> f64 {
    0.85
}
fn default_protect_first_n() -> usize {
    3
}
fn default_protect_last_n() -> usize {
    4
}
fn default_max_passes() -> u32 {
    3
}
fn default_summary_max_chars() -> usize {
    4_000
}
fn default_source_max_chars() -> usize {
    50_000
}
fn default_timeout_secs() -> u64 {
    60
}
fn default_identifier_policy() -> String {
    "strict".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContextCompressionConfig {

    #[serde(default = "default_enabled")]
    pub enabled: bool,

    #[serde(default = "default_threshold_ratio")]
    pub threshold_ratio: f64,

    #[serde(default = "default_protect_first_n")]
    pub protect_first_n: usize,

    #[serde(default = "default_protect_last_n")]
    pub protect_last_n: usize,

    #[serde(default = "default_max_passes")]
    pub max_passes: u32,

    #[serde(default = "default_summary_max_chars")]
    pub summary_max_chars: usize,

    #[serde(default = "default_source_max_chars")]
    pub source_max_chars: usize,

    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,

    #[serde(default)]
    pub summary_model: Option<String>,

    #[serde(default = "default_identifier_policy")]
    pub identifier_policy: String,
}

impl Default for ContextCompressionConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            threshold_ratio: default_threshold_ratio(),
            protect_first_n: default_protect_first_n(),
            protect_last_n: default_protect_last_n(),
            max_passes: default_max_passes(),
            summary_max_chars: default_summary_max_chars(),
            source_max_chars: default_source_max_chars(),
            timeout_secs: default_timeout_secs(),
            summary_model: None,
            identifier_policy: default_identifier_policy(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompressionResult {
    pub compressed: bool,
    pub tokens_before: usize,
    pub tokens_after: usize,
    pub passes_used: u32,
}

const PROBE_TIERS: &[usize] = &[
    2_000_000, 1_000_000, 512_000, 200_000, 128_000, 64_000, 32_000,
];

fn next_probe_tier(current: usize) -> usize {
    PROBE_TIERS
        .iter()
        .copied()
        .find(|&tier| tier < current)
        .unwrap_or(32_000)
}

static CONTEXT_LIMIT_PATTERNS: LazyLock<Vec<regex::Regex>> = LazyLock::new(|| {
    [

        r"(?:max(?:imum)?|limit)\s*(?:context\s*)?(?:length|size|window)?\s*(?:is|of|:)?\s*(\d{4,})",

        r"context\s*(?:length|size|window)\s*(?:is|of|:)?\s*(\d{4,})",

        r"(\d{4,})\s*(?:tokens?\s*)?(?:context|limit)",

        r"available context size\s*\(\s*(\d{4,})",

        r">\s*(\d{4,})\s*(?:maximum|max)?\s*(?:context)?\s*(?:length|size|window|tokens?)",
    ]
    .iter()
    .filter_map(|p| regex::Regex::new(p).ok())
    .collect()
});

pub fn parse_context_limit_from_error(msg: &str) -> Option<usize> {
    let lower = msg.to_lowercase();
    for re in CONTEXT_LIMIT_PATTERNS.iter() {
        if let Some(caps) = re.captures(&lower) {
            if let Some(m) = caps.get(1) {
                if let Ok(limit) = m.as_str().parse::<usize>() {
                    if (1024..=10_000_000).contains(&limit) {
                        return Some(limit);
                    }
                }
            }
        }
    }
    None
}

pub fn estimate_tokens(messages: &[ChatMessage]) -> usize {
    let raw: usize = messages
        .iter()
        .map(crate::providers::traits::estimate_message_tokens)
        .sum();

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        (raw as f64 * 1.05) as usize
    }
}

fn estimate_tokens_for(messages: &[ChatMessage], model: &str) -> usize {
    crate::agent::token::budget::estimate_history_tokens_calibrated(messages, model)
}

const SUMMARIZER_SYSTEM: &str = "\
You are a conversation compaction engine. Distill the conversation segment below into a
high-fidelity, STRUCTURED summary. Fill every section; if a section has nothing, write
`- (none)` rather than omitting it — the sections act as a checklist that prevents silently
dropping context whose importance only becomes clear later.

Use EXACTLY these headings, in this order:

## Session Intent
- The user's overall goal(s) and the current task, in their own terms.

## Files Modified
- For each file touched: its FULL path and the LATEST state of the changed region (final
  content after the most recent edit), so later turns never act on a stale version.

## Key Decisions
- Decisions made, approaches chosen/rejected, user preferences and constraints expressed.

## Key Findings
- Concrete facts obtained: data, results, error messages, identifiers (UUIDs, hashes,
  paths, URLs, tokens, IPs) — preserve identifiers verbatim.

## Active Goals / Next Steps
- What is still in progress, unresolved items, and the immediate next actions.

OMIT verbose tool output (keep only key results), greetings, and already-restated content.
Be thorough on the five sections but concise within each bullet.";

#[derive(Debug, Clone, Copy)]
pub struct CompressionProgress {
    pub pass: usize,
    pub max_passes: usize,
    pub tokens_current: usize,
    pub tokens_target: usize,
}

pub type CompressionProgressFn = dyn Fn(CompressionProgress) + Send + Sync;

pub type PreservedIndexFn = dyn Fn(&[ChatMessage]) -> Vec<usize> + Send + Sync;

pub struct ContextCompressor {
    config: ContextCompressionConfig,
    context_window: usize,
    tool_overhead_tokens: usize,
}

impl ContextCompressor {
    pub fn new(config: ContextCompressionConfig, context_window: usize) -> Self {
        Self {
            config,
            context_window,
            tool_overhead_tokens: 0,
        }
    }

    #[must_use]
    pub fn with_tool_overhead_tokens(mut self, tokens: usize) -> Self {
        self.tool_overhead_tokens = tokens;
        self
    }

    pub fn set_context_window(&mut self, window: usize) {
        self.context_window = window;
    }

    pub fn context_window(&self) -> usize {
        self.context_window
    }

    pub async fn compress_if_needed(
        &self,
        history: &mut Vec<ChatMessage>,
        provider: &dyn Provider,
        model: &str,
    ) -> Result<CompressionResult> {
        self.compress_if_needed_with_preserved(history, provider, model, &[])
            .await
    }

    pub async fn compress_if_needed_with_preserved(
        &self,
        history: &mut Vec<ChatMessage>,
        provider: &dyn Provider,
        model: &str,
        preserved_indices: &[usize],
    ) -> Result<CompressionResult> {
        let snapshot: Vec<usize> = preserved_indices.to_vec();
        let preserved_fn: Box<PreservedIndexFn> = Box::new(move |_history| snapshot.clone());
        self.compress_if_needed_with_progress(history, provider, model, Some(&*preserved_fn), None)
            .await
    }

    pub async fn compress_if_needed_with_progress(
        &self,
        history: &mut Vec<ChatMessage>,
        provider: &dyn Provider,
        model: &str,
        preserved_fn: Option<&PreservedIndexFn>,
        progress: Option<&CompressionProgressFn>,
    ) -> Result<CompressionResult> {
        if !self.config.enabled {
            let tokens = estimate_tokens_for(history, model);
            return Ok(CompressionResult {
                compressed: false,
                tokens_before: tokens,
                tokens_after: tokens,
                passes_used: 0,
            });
        }

        let tokens_before = estimate_tokens_for(history, model);
        let system_tokens = estimate_tokens_filtered(history, true);
        let non_system_tokens = estimate_tokens_filtered(history, false);
        tracing::debug!(
            tokens_before,
            system_tokens,
            non_system_tokens,
            "context compression token breakdown"
        );
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let threshold = ((self.context_window as f64 * self.config.threshold_ratio) as usize)
            .saturating_sub(self.tool_overhead_tokens);

        if tokens_before <= threshold {
            return Ok(CompressionResult {
                compressed: false,
                tokens_before,
                tokens_after: tokens_before,
                passes_used: 0,
            });
        }

        let started_at = std::time::Instant::now();

        {
            let preserved: Vec<usize> = preserved_fn.map(|f| f(history)).unwrap_or_default();
            let evicted = self.microcompact_tool_outputs(history, model, threshold, &preserved);
            if evicted > 0 {
                let tokens_now = estimate_tokens_for(history, model);
                tracing::info!(
                    target: "agent.context.compress",
                    evicted,
                    tokens_before,
                    tokens_now,
                    threshold,
                    "microcompact evicted stale tool outputs before summarization"
                );
                if tokens_now <= threshold {
                    return Ok(CompressionResult {
                        compressed: true,
                        tokens_before,
                        tokens_after: tokens_now,
                        passes_used: 0,
                    });
                }
            }
        }

        let mut passes_used = 0;
        for pass in 0..self.config.max_passes {
            if let Some(cb) = progress {
                cb(CompressionProgress {
                    pass: (pass + 1) as usize,
                    max_passes: self.config.max_passes as usize,
                    tokens_current: estimate_tokens_for(history, model),
                    tokens_target: threshold,
                });
            }
            let preserved_indices: Vec<usize> =
                preserved_fn.map(|f| f(history)).unwrap_or_default();
            let did_compress = self
                .compress_once_with_preserved(history, provider, model, &preserved_indices)
                .await?;
            if did_compress {
                passes_used += 1;
            }
            if estimate_tokens_for(history, model) <= threshold || !did_compress {
                break;
            }
        }

        let tokens_after = estimate_tokens_for(history, model);
        let elapsed_ms = started_at.elapsed().as_millis() as u64;
        crate::observability::runtime_trace::record_event(
            "context_compress",
            None,
            None,
            Some(model),
            None,
            Some(passes_used > 0),
            None,
            serde_json::json!({
                "tokens_before": tokens_before,
                "tokens_after": tokens_after,
                "threshold": threshold,
                "context_window": self.context_window,
                "passes_used": passes_used,
                "duration_ms": elapsed_ms,
                "message_count": history.len(),
            }),
        );
        Ok(CompressionResult {
            compressed: passes_used > 0,
            tokens_before,
            tokens_after,
            passes_used,
        })
    }

    pub async fn compress_on_error(
        &mut self,
        history: &mut Vec<ChatMessage>,
        provider: &dyn Provider,
        model: &str,
        error_msg: &str,
        preserved_fn: Option<&PreservedIndexFn>,
    ) -> Result<bool> {

        if let Some(limit) = parse_context_limit_from_error(error_msg) {
            self.context_window = limit;
        } else {

            self.context_window = next_probe_tier(self.context_window);
        }

        tracing::info!(
            context_window = self.context_window,
            "Context limit adjusted, re-compressing"
        );

        let result = self
            .compress_if_needed_with_progress(history, provider, model, preserved_fn, None)
            .await?;
        Ok(result.compressed)
    }

    pub async fn summarize_messages(
        &self,
        messages: &[ChatMessage],
        provider: &dyn Provider,
        model: &str,
    ) -> Result<String> {
        let (summary, _degraded) = self.summarize_transcript(messages, provider, model).await;
        Ok(summary)
    }

    async fn summarize_transcript(
        &self,
        messages: &[ChatMessage],
        provider: &dyn Provider,
        model: &str,
    ) -> (String, bool) {
        const MAX_MAP_CHUNKS: usize = 6;

        let full = build_compaction_transcript_full(messages);
        if full.trim().is_empty() {
            return (String::new(), false);
        }
        let message_count = messages.len();
        let summary_model = self.config.summary_model.as_deref().unwrap_or(model);
        let identifier_note = if self.config.identifier_policy == "strict" {
            "\nIMPORTANT: Preserve all identifiers exactly as they appear."
        } else {
            ""
        };
        let timeout = Duration::from_secs(self.config.timeout_secs);

        if full.len() <= self.config.source_max_chars {
            let user_prompt = format!(
                "Summarize the following conversation history ({message_count} messages) for context preservation. \
                 Keep it concise (max 20 bullet points).{identifier_note}\n\n{full}"
            );
            return match tokio::time::timeout(
                timeout,
                provider.chat_with_system(
                    Some(SUMMARIZER_SYSTEM),
                    &user_prompt,
                    summary_model,
                    0.1,
                ),
            )
            .await
            {
                Ok(Ok(s)) if !s.trim().is_empty() => {
                    (truncate_chars(&s, self.config.summary_max_chars), false)
                }
                other => {
                    if let Ok(Err(e)) = other {
                        tracing::warn!(error = %e, "summarization LLM call failed, using transcript truncation");
                    } else {
                        tracing::warn!(
                            "summarization timed out after {}s, using transcript truncation",
                            self.config.timeout_secs
                        );
                    }
                    let clipped = crate::util::truncate_head_tail(
                        &full,
                        self.config.summary_max_chars,
                        30,
                    )
                    .unwrap_or(full);
                    (truncate_chars(&clipped, self.config.summary_max_chars), true)
                }
            };
        }

        let chunks =
            split_transcript_chunks(&full, self.config.source_max_chars, MAX_MAP_CHUNKS);
        let total = chunks.len();
        tracing::info!(
            chunks = total,
            transcript_bytes = full.len(),
            "compaction range exceeds single-call budget; map-reduce summarizing"
        );
        let map_futures = chunks.iter().enumerate().map(|(i, chunk)| {
            let prompt = format!(
                "This is segment {}/{} of one longer conversation ({} messages total). \
                 Summarize THIS SEGMENT for context preservation. Keep it concise (max 20 \
                 bullet points).{}\n\n{}",
                i + 1,
                total,
                message_count,
                identifier_note,
                chunk
            );
            async move {
                match tokio::time::timeout(
                    timeout,
                    provider.chat_with_system(
                        Some(SUMMARIZER_SYSTEM),
                        &prompt,
                        summary_model,
                        0.1,
                    ),
                )
                .await
                {
                    Ok(Ok(s)) if !s.trim().is_empty() => (s, false),
                    _ => {
                        let clipped = crate::util::truncate_head_tail(chunk, 4_000, 30)
                            .unwrap_or_else(|| chunk.clone());
                        (
                            format!("[segment summarizer unavailable; raw excerpt]\n{clipped}"),
                            true,
                        )
                    }
                }
            }
        });
        let mapped = futures_util::future::join_all(map_futures).await;
        let degraded = mapped.iter().any(|(_, d)| *d);
        let combined = mapped
            .iter()
            .enumerate()
            .map(|(i, (s, _))| format!("### Segment {}/{}\n{}", i + 1, total, s))
            .collect::<Vec<_>>()
            .join("\n\n");

        let reduce_prompt = format!(
            "Below are structured summaries of {total} consecutive segments of ONE conversation \
             ({message_count} messages). Merge them into a SINGLE summary with EXACTLY the same \
             five headings, deduplicating overlap while keeping every concrete identifier, file \
             path, and final file state verbatim.{identifier_note}\n\n{combined}"
        );
        let reduced = match tokio::time::timeout(
            timeout,
            provider.chat_with_system(
                Some(SUMMARIZER_SYSTEM),
                &reduce_prompt,
                summary_model,
                0.1,
            ),
        )
        .await
        {
            Ok(Ok(s)) if !s.trim().is_empty() => {
                truncate_chars(&s, self.config.summary_max_chars)
            }
            _ => {
                truncate_chars(&combined, self.config.summary_max_chars.saturating_mul(3))
            }
        };
        (reduced, degraded)
    }

    fn microcompact_tool_outputs(
        &self,
        history: &mut [ChatMessage],
        model: &str,
        threshold: usize,
        preserved_indices: &[usize],
    ) -> usize {
        const MIN_EVICT_BYTES: usize = 2_048;

        let n = history.len();
        let start = align_boundary_forward(history, self.config.protect_first_n.min(n));
        let end = align_boundary_backward(
            history,
            n.saturating_sub(self.config.protect_last_n),
        );
        if start >= end {
            return 0;
        }
        let mut current = estimate_tokens_for(history, model);
        let calibration =
            crate::agent::token::budget::calibration_factor_for(model) * 1.05;
        let mut evicted = 0usize;
        for idx in start..end {
            if current <= threshold {
                break;
            }
            if preserved_indices.contains(&idx) {
                continue;
            }
            let msg = &mut history[idx];
            if msg.role != "tool" || is_compaction_banner(&msg.content) {
                continue;
            }
            let before = crate::providers::traits::estimate_message_tokens(msg);
            if !evict_tool_message_content(msg, MIN_EVICT_BYTES) {
                continue;
            }
            let after = crate::providers::traits::estimate_message_tokens(msg);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let delta = ((before.saturating_sub(after)) as f64 * calibration).round() as usize;
            current = current.saturating_sub(delta);
            evicted += 1;
        }
        evicted
    }

    async fn compress_once_with_preserved(
        &self,
        history: &mut Vec<ChatMessage>,
        provider: &dyn Provider,
        model: &str,
        preserved_indices: &[usize],
    ) -> Result<bool> {
        let n = history.len();
        let protected_total = self.config.protect_first_n + self.config.protect_last_n;
        if n <= protected_total {
            return Ok(false);
        }

        let mut start = self.config.protect_first_n.min(n);
        let mut end = n.saturating_sub(self.config.protect_last_n);

        start = align_boundary_forward(history, start);
        end = align_boundary_backward(history, end);

        if start >= end {
            return Ok(false);
        }

        if !preserved_indices.is_empty() {
            let in_range: Vec<usize> = preserved_indices
                .iter()
                .copied()
                .filter(|i| *i >= start && *i < end)
                .collect();
            if !in_range.is_empty() {
                let min_pres = *in_range.iter().min().unwrap_or(&start);
                let max_pres = *in_range.iter().max().unwrap_or(&end);
                let shrink_from_front = max_pres + 1;
                let shrink_from_back = min_pres;
                let keep_from_front = end.saturating_sub(shrink_from_front);
                let keep_from_back = shrink_from_back.saturating_sub(start);

                if keep_from_front == 0 && keep_from_back == 0 {
                    crate::observability::code_intel_metrics::incr_context_preserve_skip_compress();
                    return Ok(false);
                }
                if keep_from_front >= keep_from_back {
                    start = shrink_from_front.min(end);
                } else {
                    end = shrink_from_back.max(start);
                }

                start = align_boundary_forward(history, start);
                end = align_boundary_backward(history, end);
                if start >= end {
                    crate::observability::code_intel_metrics::incr_context_preserve_skip_compress();
                    return Ok(false);
                }
            }
        }

        let middle = &history[start..end];
        let message_count = end - start;
        let (summary, degraded) = self.summarize_transcript(middle, provider, model).await;
        if summary.trim().is_empty() {
            return Ok(false);
        }

        let summary_msg = if degraded {
            format!(
                "{COMPACTION_BANNER_PREFIX} summarizer unavailable; {message_count} earlier \
                 messages were TRUNCATED (not summarized), so middle content may be missing. \
                 Re-read source files/tool outputs if you need details.]\n\n{summary}"
            )
        } else {
            format!(
                "{SUMMARY_BANNER_PREFIX} {message_count} earlier messages compressed]\n\n{summary}"
            )
        };
        replace_history_range_with_assistant(history, start, end, summary_msg);

        repair_tool_pairs(history);

        Ok(true)
    }
}

fn align_boundary_forward(messages: &[ChatMessage], idx: usize) -> usize {
    let mut i = idx;
    while i < messages.len() && messages[i].role == "tool" {
        i += 1;
    }
    i
}

fn align_boundary_backward(messages: &[ChatMessage], idx: usize) -> usize {
    let mut i = idx;

    while i > 0 && i < messages.len() && messages[i].role == "tool" {

        i -= 1;
    }
    i
}

fn is_compaction_banner(content: &str) -> bool {
    content.starts_with(SUMMARY_BANNER_PREFIX) || content.starts_with(COMPACTION_BANNER_PREFIX)
}

const EVICTED_OUTPUT_MARKER: &str = "[tool output evicted";

fn eviction_placeholder(bytes: usize) -> String {
    format!(
        "{EVICTED_OUTPUT_MARKER} during context compaction ({bytes} bytes). The result is no \
         longer in context — re-run the tool or re-read the file if these details are needed \
         again.]"
    )
}

fn evict_tool_message_content(msg: &mut ChatMessage, min_bytes: usize) -> bool {
    if msg.content.contains(EVICTED_OUTPUT_MARKER) {
        return false;
    }
    if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&msg.content) {
        if let Some(obj) = value.as_object_mut() {
            if obj.contains_key("tool_call_id") {
                let payload_len = obj
                    .get("content")
                    .and_then(|c| c.as_str())
                    .map(str::len)
                    .unwrap_or(0);
                if payload_len < min_bytes {
                    return false;
                }
                obj.insert(
                    "content".to_string(),
                    serde_json::Value::String(eviction_placeholder(payload_len)),
                );
                if let Ok(serialized) = serde_json::to_string(&value) {
                    msg.content = serialized;
                    return true;
                }
                return false;
            }
        }
    }
    if msg.content.len() < min_bytes {
        return false;
    }
    let bytes = msg.content.len();
    msg.content = eviction_placeholder(bytes);
    true
}

pub(crate) fn repair_tool_pairs(messages: &mut Vec<ChatMessage>) {

    let mut i = 0;
    while i < messages.len() {
        if is_compaction_banner(&messages[i].content) {

            while i + 1 < messages.len() && messages[i + 1].role == "tool" {
                messages.remove(i + 1);
            }
        }
        i += 1;
    }

    let start = usize::from(messages.first().is_some_and(|m| m.role == "system"));
    while start < messages.len() && messages[start].role == "tool" {
        messages.remove(start);
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }

    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut result = s[..end].to_string();
    result.push_str("...");
    result
}

