// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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
        rationale: "function signature changed  -  review call sites for breakage".into(),
        confidence: Some(0.4),
        origin: "heuristic_nep",
    })
}

fn rule_pending_close_bracket(req: &NepRequest) -> Option<NepSuggestion> {
    let edit = req.recent_edits.first()?;
    let last_added = edit
        .diff
        .lines()
        .rfind(|l| l.starts_with('+') && !l.starts_with("+++"))?
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
        rationale: format!("close the pending `{}` block", close),
        confidence: Some(0.7),
        origin: "heuristic_nep",
    })
}
