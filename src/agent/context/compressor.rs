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
    0.90
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
    6_000
}
fn default_source_max_chars() -> usize {
    80_000
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
    pub duration_ms: u64,
    pub summarized: bool,
}

pub const REFERENCE_RATIO: f64 = 0.70;
pub const SUMMARIZE_RATIO: f64 = 0.90;
pub const EMERGENCY_RATIO: f64 = 0.80;
pub const COMPACTION_WALL_BUDGET_SECS: u64 = 10;

fn ratio_limit(context_window: usize, ratio: f64) -> usize {
    let ratio = if ratio.is_finite() && ratio > 0.0 {
        ratio
    } else {
        SUMMARIZE_RATIO
    };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        (context_window as f64 * ratio) as usize
    }
}

pub fn payload_soft_limit(context_window: usize) -> usize {
    ratio_limit(context_window, REFERENCE_RATIO)
}

pub fn payload_hard_limit(context_window: usize) -> usize {
    payload_hard_limit_for_config(context_window, SUMMARIZE_RATIO)
}

pub fn payload_hard_limit_for_config(context_window: usize, threshold_ratio: f64) -> usize {
    let ratio = if threshold_ratio.is_finite() && threshold_ratio > SUMMARIZE_RATIO {
        threshold_ratio
    } else {
        SUMMARIZE_RATIO
    };
    ratio_limit(context_window, ratio)
}

pub fn payload_tokens(history_tokens: usize, tools_overhead_tokens: usize) -> usize {
    history_tokens.saturating_add(tools_overhead_tokens)
}

pub fn history_token_threshold(
    context_window: usize,
    threshold_ratio: f64,
    tools_overhead_tokens: usize,
) -> usize {
    payload_hard_limit_for_config(context_window, threshold_ratio)
        .saturating_sub(tools_overhead_tokens)
}

pub fn estimate_compressible_tokens(
    history: &[ChatMessage],
    config: &ContextCompressionConfig,
    preserved_indices: &[usize],
    model: &str,
) -> usize {
    let n = history.len();
    if n <= config.protect_first_n + config.protect_last_n {
        return 0;
    }
    let start = align_boundary_forward(history, config.protect_first_n.min(n));
    let end = align_boundary_backward(history, n.saturating_sub(config.protect_last_n));
    if start >= end {
        return 0;
    }
    let raw: usize = history[start..end]
        .iter()
        .enumerate()
        .filter(|(offset, _)| !preserved_indices.contains(&(start + offset)))
        .map(|(_, m)| crate::providers::traits::estimate_message_tokens(m))
        .sum();
    let factor = crate::agent::token::budget::calibration_factor_for(model);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        ((raw as f64 * 1.05) * factor).round() as usize
    }
}

static SESSION_COMPRESSION_FLOORS: LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, usize>>,
> = LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

#[must_use]
pub fn session_compression_floor(session_id: &str) -> Option<usize> {
    SESSION_COMPRESSION_FLOORS
        .lock()
        .ok()
        .and_then(|floors| floors.get(session_id).copied())
}

pub fn set_session_compression_floor(session_id: &str, floor: Option<usize>) {
    let Ok(mut floors) = SESSION_COMPRESSION_FLOORS.lock() else {
        return;
    };
    match floor {
        Some(value) => {
            if floors.len() > 512 && !floors.contains_key(session_id) {
                floors.clear();
            }
            floors.insert(session_id.to_string(), value);
        }
        None => {
            floors.remove(session_id);
        }
    }
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
    emergency_mode: bool,
}

impl ContextCompressor {
    pub fn new(config: ContextCompressionConfig, context_window: usize) -> Self {
        Self {
            config,
            context_window,
            tool_overhead_tokens: 0,
            emergency_mode: false,
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
        let tokens_before = estimate_tokens_for(history, model);
        if !self.config.enabled {
            return Ok(CompressionResult {
                compressed: false,
                tokens_before,
                tokens_after: tokens_before,
                passes_used: 0,
                duration_ms: 0,
                summarized: false,
            });
        }

        let system_tokens = estimate_tokens_filtered(history, true);
        let non_system_tokens = estimate_tokens_filtered(history, false);
        tracing::debug!(
            tokens_before,
            system_tokens,
            non_system_tokens,
            "context compression token breakdown"
        );
        let soft = payload_soft_limit(self.context_window);
        let hard = if self.emergency_mode {
            ratio_limit(self.context_window, EMERGENCY_RATIO)
        } else {
            payload_hard_limit_for_config(self.context_window, self.config.threshold_ratio)
        };
        let payload_before = payload_tokens(tokens_before, self.tool_overhead_tokens);
        let skip_under = if self.emergency_mode { hard } else { soft };
        if payload_before <= skip_under {
            return Ok(CompressionResult {
                compressed: false,
                tokens_before,
                tokens_after: tokens_before,
                passes_used: 0,
                duration_ms: 0,
                summarized: false,
            });
        }

        let started_at = std::time::Instant::now();
        let history_soft = soft.saturating_sub(self.tool_overhead_tokens);
        let history_hard = hard.saturating_sub(self.tool_overhead_tokens);

        let stale_preserved: Vec<usize> = if self.emergency_mode {
            Vec::new()
        } else {
            preserved_fn.map(|f| f(history)).unwrap_or_default()
        };
        let mut evicted =
            self.microcompact_tool_outputs(history, model, history_soft, &stale_preserved);
        let mut tokens_now = estimate_tokens_for(history, model);
        let mut payload_now = payload_tokens(tokens_now, self.tool_overhead_tokens);
        if payload_now > hard {
            evicted += self.microcompact_tool_outputs(history, model, history_hard, &[]);
            tokens_now = estimate_tokens_for(history, model);
            payload_now = payload_tokens(tokens_now, self.tool_overhead_tokens);
        }
        if evicted > 0 {
            tracing::info!(
                target: "agent.context.compress",
                evicted,
                tokens_before,
                tokens_now,
                payload_now,
                soft,
                hard,
                emergency = self.emergency_mode,
                context_window = self.context_window,
                tools_overhead = self.tool_overhead_tokens,
                duration_ms = started_at.elapsed().as_millis() as u64,
                "microcompact evicted stale tool outputs"
            );
        }
        if payload_now <= hard {
            return Ok(CompressionResult {
                compressed: evicted > 0,
                tokens_before,
                tokens_after: tokens_now,
                passes_used: 0,
                duration_ms: started_at.elapsed().as_millis() as u64,
                summarized: false,
            });
        }

        let mut passes_used = 0;
        for pass in 0..self.config.max_passes {
            if let Some(cb) = progress {
                cb(CompressionProgress {
                    pass: (pass + 1) as usize,
                    max_passes: self.config.max_passes as usize,
                    tokens_current: estimate_tokens_for(history, model),
                    tokens_target: history_hard,
                });
            }
            let before_pass = estimate_tokens_for(history, model);
            let preserved_indices: Vec<usize> = if self.emergency_mode {
                Vec::new()
            } else {
                preserved_fn.map(|f| f(history)).unwrap_or_default()
            };
            let did_compress = self
                .compress_once_with_preserved(history, provider, model, &preserved_indices)
                .await?;
            let after_pass = estimate_tokens_for(history, model);
            if did_compress {
                passes_used += 1;
                let reduced = before_pass.saturating_sub(after_pass);
                if before_pass > 0 && reduced.saturating_mul(20) < before_pass {
                    break;
                }
            }
            let payload_now = payload_tokens(after_pass, self.tool_overhead_tokens);
            if payload_now <= hard || !did_compress {
                break;
            }
        }

        let tokens_after = estimate_tokens_for(history, model);
        let elapsed_ms = started_at.elapsed().as_millis() as u64;
        let payload_after = payload_tokens(tokens_after, self.tool_overhead_tokens);
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
                "payload_before": payload_before,
                "payload_after": payload_after,
                "soft": soft,
                "hard": hard,
                "context_window": self.context_window,
                "tools_overhead": self.tool_overhead_tokens,
                "passes_used": passes_used,
                "duration_ms": elapsed_ms,
                "message_count": history.len(),
                "summarized": passes_used > 0,
            }),
        );
        tracing::info!(
            target: "agent.context.compress",
            tokens_before,
            tokens_after,
            payload_before,
            payload_after,
            soft,
            hard,
            context_window = self.context_window,
            tools_overhead = self.tool_overhead_tokens,
            passes_used,
            duration_ms = elapsed_ms,
            summarized = passes_used > 0,
            "context compression finished"
        );
        Ok(CompressionResult {
            compressed: evicted > 0 || passes_used > 0,
            tokens_before,
            tokens_after,
            passes_used,
            duration_ms: elapsed_ms,
            summarized: passes_used > 0,
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

        self.emergency_mode = true;
        let result = self
            .compress_if_needed_with_progress(history, provider, model, preserved_fn, None)
            .await;
        self.emergency_mode = false;
        Ok(result?.compressed)
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
                crate::providers::with_reasoning_suppressed(provider.chat_with_system(
                    Some(SUMMARIZER_SYSTEM),
                    &user_prompt,
                    summary_model,
                    0.1,
                )),
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
                    crate::providers::with_reasoning_suppressed(provider.chat_with_system(
                        Some(SUMMARIZER_SYSTEM),
                        &prompt,
                        summary_model,
                        0.1,
                    )),
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
            crate::providers::with_reasoning_suppressed(provider.chat_with_system(
                Some(SUMMARIZER_SYSTEM),
                &reduce_prompt,
                summary_model,
                0.1,
            )),
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
        const TAIL_EVICT_BYTES: usize = 8_192;

        let n = history.len();
        let start = align_boundary_forward(history, self.config.protect_first_n.min(n));
        let protected_end = align_boundary_backward(
            history,
            n.saturating_sub(self.config.protect_last_n),
        );
        if start >= n {
            return 0;
        }
        let mut current = estimate_tokens_for(history, model);
        let calibration =
            crate::agent::token::budget::calibration_factor_for(model) * 1.05;
        let mut evicted = 0usize;
        let last_keep = n.saturating_sub(1);
        for idx in start..last_keep {
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
            let min_bytes = if idx >= protected_end {
                TAIL_EVICT_BYTES
            } else {
                MIN_EVICT_BYTES
            };
            let before = crate::providers::traits::estimate_message_tokens(msg);
            if !evict_tool_message_content(msg, min_bytes) {
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

const EVICT_EXCERPT_CHARS: usize = 512;

fn take_excerpt(s: &str, max_chars: usize) -> String {
    let excerpt: String = s.chars().take(max_chars).collect();
    excerpt.replace(['\r', '\n'], " ")
}

fn eviction_placeholder(bytes: usize, blob_id: Option<&str>, excerpt: &str) -> String {
    let marker = match blob_id {
        Some(id) => format!(
            "{EVICTED_OUTPUT_MARKER} during context compaction ({bytes} bytes; archived as \
             blob {id} — call tool_result_expand with this id to retrieve it). Do NOT \
             re-execute this tool with the same arguments.]"
        ),
        None => format!(
            "{EVICTED_OUTPUT_MARKER} during context compaction ({bytes} bytes). The result \
             is no longer in context. For file reads, page into line ranges that are not \
             yet covered using a new offset/limit. Do NOT re-run the original call with \
             the same arguments.]"
        ),
    };
    if excerpt.is_empty() {
        marker
    } else {
        format!("{excerpt}\n{marker}")
    }
}

fn evict_tool_message_content(msg: &mut ChatMessage, min_bytes: usize) -> bool {
    if msg.content.contains(EVICTED_OUTPUT_MARKER) {
        return false;
    }
    if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&msg.content) {
        if let Some(obj) = value.as_object_mut() {
            if obj.contains_key("tool_call_id") {
                let payload = obj
                    .get("content")
                    .and_then(|c| c.as_str())
                    .map(str::to_string)
                    .unwrap_or_default();
                if payload.len() < min_bytes {
                    return false;
                }
                let blob_id = crate::agent::history::blob_store::put(&payload);
                let excerpt = take_excerpt(&payload, EVICT_EXCERPT_CHARS);
                obj.insert(
                    "content".to_string(),
                    serde_json::Value::String(eviction_placeholder(
                        payload.len(),
                        blob_id.as_deref(),
                        &excerpt,
                    )),
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
    let blob_id = crate::agent::history::blob_store::put(&msg.content);
    let excerpt = take_excerpt(&msg.content, EVICT_EXCERPT_CHARS);
    msg.content = eviction_placeholder(bytes, blob_id.as_deref(), &excerpt);
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

