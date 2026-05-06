// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! LLM-backed next-edit predictor.
//!
//! [`LlmNep`] sends the recent edit history and a window around the
//! cursor to a configured [`Provider`] and asks for a unified diff
//! describing the most likely next edit.  The model is constrained
//! by a tight system prompt so it cannot reply with prose: the only
//! valid output is a single-file unified diff (or an explicit empty
//! string when no useful prediction is possible).
//!
//! By default we route through the same provider/model the inline
//! edit runner uses; surfaces that want a smaller / faster model can
//! override `model` at construction time (e.g. wire it to the M1.5
//! `fast_apply_model` so heuristic-fail → small-model NEP → large
//! model on accept).

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;

use super::{NepError, NepProvider, NepRequest, NepResponse, NepSuggestion};
use crate::providers::traits::Provider;

const SYSTEM_PROMPT: &str = "\
You are a next-edit predictor for a code editor.\n\
Given the user's most recent edit and a window of the active file,\n\
produce ONE unified diff that describes the most likely NEXT edit.\n\
\n\
Rules:\n\
- Output a single unified diff and nothing else.  No prose, no\n\
  Markdown fences.\n\
- The diff must apply cleanly with `patch -p1` on the active file.\n\
- If you cannot identify a clear next edit, reply with an empty\n\
  string.  Do not invent edits.\n\
- Stay focused on the active file.  Do not propose changes in\n\
  other files unless the user's recent edit explicitly references\n\
  them by path.\n";

pub struct LlmNep {
    provider: Arc<dyn Provider>,
    model: String,
    temperature: f64,
    timeout: Duration,

    max_history: usize,

    window_lines: usize,
}

impl std::fmt::Debug for LlmNep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmNep")
            .field("model", &self.model)
            .field("temperature", &self.temperature)
            .field("timeout", &self.timeout)
            .field("max_history", &self.max_history)
            .field("window_lines", &self.window_lines)
            .finish()
    }
}

impl LlmNep {
    pub fn new(provider: Arc<dyn Provider>, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
            temperature: 0.0,
            timeout: Duration::from_secs(15),
            max_history: 8,
            window_lines: 80,
        }
    }

    #[must_use]
    pub fn with_temperature(mut self, t: f64) -> Self {
        self.temperature = t;
        self
    }

    #[must_use]
    pub fn with_timeout(mut self, t: Duration) -> Self {
        self.timeout = t;
        self
    }

    #[must_use]
    pub fn with_max_history(mut self, n: usize) -> Self {
        self.max_history = n.max(1);
        self
    }

    #[must_use]
    pub fn with_window_lines(mut self, n: usize) -> Self {
        self.window_lines = n.max(8);
        self
    }
}

#[async_trait]
impl NepProvider for LlmNep {
    async fn predict(&self, req: NepRequest) -> Result<NepResponse, NepError> {
        let start = Instant::now();
        let prompt = build_prompt(&req, self.max_history, self.window_lines);
        let fut = self.provider.chat_with_system(
            Some(SYSTEM_PROMPT),
            &prompt,
            &self.model,
            self.temperature,
        );
        let raw = match tokio::time::timeout(self.timeout, fut).await {
            Ok(Ok(r)) => r,
            Ok(Err(err)) => {
                return Err(NepError::Provider {
                    provider: "llm_nep".into(),
                    source: anyhow::anyhow!(err),
                });
            }
            Err(_) => {
                return Err(NepError::Timeout {
                    provider: "llm_nep".into(),
                    timeout_ms: self.timeout.as_millis() as u64,
                });
            }
        };
        let diff = strip_markdown_fence(&raw);
        if diff.trim().is_empty() {
            return Ok(NepResponse {
                suggestions: Vec::new(),
                latency_ms: start.elapsed().as_millis() as u64,
                provider: "llm_nep".into(),
            });
        }
        let suggestion = NepSuggestion {
            file_path: req.active_file.clone(),
            diff,
            rationale: "predicted next edit".into(),
            confidence: Some(0.6),
            origin: "llm_nep",
        };
        Ok(NepResponse {
            suggestions: vec![suggestion],
            latency_ms: start.elapsed().as_millis() as u64,
            provider: "llm_nep".into(),
        })
    }

    fn name(&self) -> &'static str {
        "llm_nep"
    }
}

fn build_prompt(req: &NepRequest, max_history: usize, window_lines: usize) -> String {
    let mut out = String::new();
    out.push_str("# Active file\n");
    out.push_str(&format!("Path: {}\n", req.active_file.display()));
    out.push_str(&format!("Cursor line: {}\n\n", req.cursor_line));
    out.push_str("## Source window\n```\n");
    out.push_str(&windowed_source(&req.source, req.cursor_line, window_lines));
    out.push_str("\n```\n\n");
    out.push_str("## Recent edits (newest first)\n");
    let history: Vec<&super::RecentEdit> = req
        .recent_edits
        .iter()
        .take(max_history)
        .collect();
    if history.is_empty() {
        out.push_str("(none)\n");
    } else {
        for (idx, edit) in history.iter().enumerate() {
            out.push_str(&format!(
                "### Edit {idx} — {file}\n",
                idx = idx + 1,
                file = edit.file_path.display(),
            ));
            if let Some(instr) = edit.instruction.as_deref() {
                out.push_str(&format!("Instruction: {instr}\n"));
            }
            out.push_str("Diff:\n```diff\n");
            out.push_str(&edit.diff);
            if !edit.diff.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```\n\n");
        }
    }
    out.push_str(
        "Produce the next-edit unified diff (or empty string if none).\n",
    );
    out
}

fn windowed_source(source: &str, cursor_line: u32, window_lines: usize) -> String {
    let lines: Vec<&str> = source.lines().collect();
    if lines.is_empty() {
        return String::new();
    }
    let half = window_lines / 2;
    let cursor_idx = (cursor_line as usize).saturating_sub(1).min(lines.len());
    let start = cursor_idx.saturating_sub(half);
    let end = (cursor_idx + half).min(lines.len());
    let mut out = String::new();
    for (idx, line) in lines[start..end].iter().enumerate() {
        let line_no = start + idx + 1;
        out.push_str(&format!("{line_no:>5} | {line}\n"));
    }
    out
}

fn strip_markdown_fence(raw: &str) -> String {
    let trimmed = raw.trim_start();
    let without_fence = if let Some(rest) = trimmed.strip_prefix("```diff") {
        rest.trim_start_matches('\n')
    } else if let Some(rest) = trimmed.strip_prefix("```") {
        rest.trim_start_matches('\n')
    } else {
        trimmed
    };
    let end_trimmed = without_fence.trim_end();
    let core = end_trimmed
        .strip_suffix("```")
        .map_or(end_trimmed, str::trim_end);
    if core.ends_with('\n') {
        core.to_string()
    } else {
        format!("{core}\n")
    }
}
