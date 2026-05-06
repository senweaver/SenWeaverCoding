// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Heuristic next-edit predictor.
//!
//! Pattern-matcher that handles the high-frequency cases without an
//! LLM round-trip.  We keep the rule set deliberately small so the
//! predictor stays fast (< 1 ms) and easy to reason about; the
//! [`super::llm::LlmNep`] handles the long tail.
//!
//! Implemented rules (all consume the most recent edit only):
//!
//! 1. **Trailing TODO** — when the recent diff added a `TODO` /
//!    `FIXME` comment and the cursor sits on the same line, suggest
//!    an empty stub immediately below so the user can keep typing.
//! 2. **Function-signature change** — when the recent diff renamed a
//!    parameter or changed a Rust function signature line, surface
//!    a no-op diff that points the user at the call sites.  We
//!    deliberately do not try to *rewrite* call sites here — that's
//!    the [`LlmNep`] / refactor tool's job; the heuristic just
//!    annotates the next location.
//! 3. **Pending close-bracket** — when the recent diff opened a
//!    block (last line ends with `{` / `(` / `[`) but did not close
//!    it, suggest the matching close.
//!
//! Each rule produces at most one [`NepSuggestion`].  We pick the
//! *first* rule that fires — they are ordered by typical hit-rate.

use std::time::Instant;

use async_trait::async_trait;

use super::{NepError, NepProvider, NepRequest, NepResponse, NepSuggestion};

#[derive(Debug, Clone, Default)]
pub struct HeuristicNep;

impl HeuristicNep {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl NepProvider for HeuristicNep {
    async fn predict(&self, req: NepRequest) -> Result<NepResponse, NepError> {
        let start = Instant::now();
        let mut suggestions = Vec::new();
        if let Some(s) = rule_pending_close_bracket(&req) {
            suggestions.push(s);
        } else if let Some(s) = rule_trailing_todo(&req) {
            suggestions.push(s);
        } else if let Some(s) = rule_signature_change(&req) {
            suggestions.push(s);
        }
        Ok(NepResponse {
            suggestions,
            latency_ms: start.elapsed().as_millis() as u64,
            provider: "heuristic_nep".into(),
        })
    }

    fn name(&self) -> &'static str {
        "heuristic_nep"
    }
}

fn rule_trailing_todo(req: &NepRequest) -> Option<NepSuggestion> {
    let edit = req.recent_edits.first()?;
    let added: Vec<&str> = edit
        .diff
        .lines()
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .map(|l| l.trim_start_matches('+'))
        .collect();
    if added.is_empty() {
        return None;
    }
    let last_added = added.last()?;
    let trimmed = last_added.trim();
    if !(trimmed.contains("TODO") || trimmed.contains("FIXME")) {
        return None;
    }
    let file = edit.file_path.display().to_string();
    let line = req.cursor_line.max(1);
    let diff = format!(
        "--- a/{file}\n+++ b/{file}\n@@ -{line},0 +{line},1 @@\n+\n",
        file = file,
        line = line + 1,
    );
    Some(NepSuggestion {
        file_path: edit.file_path.clone(),
        diff,
        rationale: "open a blank line below the TODO so you can keep typing".into(),
        confidence: Some(0.55),
        origin: "heuristic_nep",
    })
}

fn rule_signature_change(req: &NepRequest) -> Option<NepSuggestion> {
    let edit = req.recent_edits.first()?;
    let mut found = false;
    for line in edit.diff.lines() {
        if line.starts_with('+') && !line.starts_with("+++") {
            let trimmed = line.trim_start_matches('+').trim_start();
            if trimmed.starts_with("pub fn ")
                || trimmed.starts_with("fn ")
                || trimmed.starts_with("pub async fn ")
                || trimmed.starts_with("async fn ")
            {
                found = true;
                break;
            }
        }
    }
    if !found {
        return None;
    }
    Some(NepSuggestion {
        file_path: edit.file_path.clone(),
        diff: String::new(),
        rationale: "function signature changed — review call sites for breakage".into(),
        confidence: Some(0.4),
        origin: "heuristic_nep",
    })
}

fn rule_pending_close_bracket(req: &NepRequest) -> Option<NepSuggestion> {
    let edit = req.recent_edits.first()?;
    let last_added = edit
        .diff
        .lines()
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .last()?
        .trim_start_matches('+');
    let trimmed = last_added.trim_end();
    let close = match trimmed.chars().last()? {
        '{' => '}',
        '(' => ')',
        '[' => ']',
        _ => return None,
    };
    let file = edit.file_path.display().to_string();
    let cursor_line = req.cursor_line.max(1);
    let next_line = cursor_line + 1;
    let diff = format!(
        "--- a/{file}\n+++ b/{file}\n@@ -{cursor_line},0 +{next_line},1 @@\n+{close}\n",
        file = file,
        cursor_line = cursor_line,
        next_line = next_line,
        close = close,
    );
    Some(NepSuggestion {
        file_path: edit.file_path.clone(),
        diff,
        rationale: format!("close the pending `{}` block", close).into(),
        confidence: Some(0.7),
        origin: "heuristic_nep",
    })
}
