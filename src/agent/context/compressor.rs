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
    build_compaction_transcript, estimate_tokens_filtered, replace_history_range_with_assistant,
};

fn default_enabled() -> bool {
    true
}
fn default_threshold_ratio() -> f64 {
    0.50
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
        .map(|m| m.content.len().div_ceil(4) + 4)
        .sum();

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        (raw as f64 * 1.2) as usize
    }
}

const SUMMARIZER_SYSTEM: &str = "\
You are a conversation compaction engine. Summarize the conversation segment below into concise context.

PRESERVE exactly:
- All identifiers (UUIDs, hashes, file paths, URLs, tokens, IPs)
- Actions taken (tool calls, file operations, commands run)
- Key information obtained (data, results, error messages)
- Decisions made and user preferences expressed
- Current task status and unresolved items
- Constraints and requirements mentioned

OMIT:
- Verbose tool output (keep only key results)
- Repeated greetings or filler
- Redundant information already stated

Output concise bullet points. Be thorough but brief.";

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
}

impl ContextCompressor {
    pub fn new(config: ContextCompressionConfig, context_window: usize) -> Self {
        Self {
            config,
            context_window,
        }
    }

    pub fn set_context_window(&mut self, window: usize) {
        self.context_window = window;
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
            let tokens = estimate_tokens(history);
            return Ok(CompressionResult {
                compressed: false,
                tokens_before: tokens,
                tokens_after: tokens,
                passes_used: 0,
            });
        }

        let tokens_before = estimate_tokens(history);
        let system_tokens = estimate_tokens_filtered(history, true);
        let non_system_tokens = estimate_tokens_filtered(history, false);
        tracing::debug!(
            tokens_before,
            system_tokens,
            non_system_tokens,
            "context compression token breakdown"
        );
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let threshold = (self.context_window as f64 * self.config.threshold_ratio) as usize;

        if tokens_before <= threshold {
            return Ok(CompressionResult {
                compressed: false,
                tokens_before,
                tokens_after: tokens_before,
                passes_used: 0,
            });
        }

        let started_at = std::time::Instant::now();
        let mut passes_used = 0;
        for pass in 0..self.config.max_passes {
            if let Some(cb) = progress {
                cb(CompressionProgress {
                    pass: (pass + 1) as usize,
                    max_passes: self.config.max_passes as usize,
                    tokens_current: estimate_tokens(history),
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
            if estimate_tokens(history) <= threshold || !did_compress {
                break;
            }
        }

        let tokens_after = estimate_tokens(history);
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

        let result = self.compress_if_needed(history, provider, model).await?;
        Ok(result.compressed)
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
                crate::observability::code_intel_metrics::incr_context_preserve_skip_compress();
            }
        }

        let middle = &history[start..end];
        let transcript = build_compaction_transcript(middle, self.config.source_max_chars);

        if transcript.is_empty() {
            return Ok(false);
        }

        let message_count = end - start;
        let summary_model = self.config.summary_model.as_deref().unwrap_or(model);

        let identifier_note = if self.config.identifier_policy == "strict" {
            "\nIMPORTANT: Preserve all identifiers exactly as they appear."
        } else {
            ""
        };

        let user_prompt = format!(
            "Summarize the following conversation history ({message_count} messages) for context preservation. \
             Keep it concise (max 20 bullet points).{identifier_note}\n\n{transcript}"
        );

        let timeout = Duration::from_secs(self.config.timeout_secs);
        let summary_raw = match tokio::time::timeout(
            timeout,
            provider.chat_with_system(Some(SUMMARIZER_SYSTEM), &user_prompt, summary_model, 0.1),
        )
        .await
        {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "Summarization LLM call failed, using transcript truncation");
                truncate_chars(&transcript, self.config.summary_max_chars)
            }
            Err(_) => {
                tracing::warn!(
                    "Summarization timed out after {}s, using transcript truncation",
                    self.config.timeout_secs
                );
                truncate_chars(&transcript, self.config.summary_max_chars)
            }
        };

        let summary = truncate_chars(&summary_raw, self.config.summary_max_chars);

        let summary_msg = format!(
            "[CONTEXT SUMMARY \u{2014} {message_count} earlier messages compressed]\n\n{summary}"
        );
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

fn repair_tool_pairs(messages: &mut Vec<ChatMessage>) {

    let mut i = 0;
    while i < messages.len() {
        if messages[i].content.contains("[CONTEXT SUMMARY") {

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

