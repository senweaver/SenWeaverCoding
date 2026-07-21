// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub const REFINE_SYSTEM_PROMPT: &str = r"You are a patch-refiner.  The user will provide:
  1. The current file contents.
  2. A broken unified diff that failed to apply.
  3. Optional hint about the intended edit.

Return ONLY a new unified diff that applies cleanly to the current
file and expresses the same intent.  No prose, no backticks, no
explanation.  The output must start with `--- ` or `@@ ` and end
with a newline.  Use exact file-path headers (`--- a/…`, `+++ b/…`)
when present in the broken diff.  Preserve context line prefixes
(` `, `+`, `-`).";

const REFINE_USER_PROMPT_V2_FOOTER: &str = "\n# Task\n\nReturn ONLY the corrected unified diff.\nUse a *small* hunk  -  ideally one or two lines of pre-image context\non each side of the change so the heuristic locator has high\nprecision.  Do NOT rewrite unrelated code.  Do NOT widen the hunk\nbeyond the failure site indicated above.\n";

const REFINE_WINDOW_RADIUS: usize = 40;

fn hunk_target_lines(failed_diff: &str) -> Vec<usize> {
    let mut targets = Vec::new();
    for line in failed_diff.lines() {
        if let Some(rest) = line.strip_prefix("@@ -") {
            let num: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(n) = num.parse::<usize>() {
                targets.push(n);
            }
        }
    }
    targets
}

fn windowed_source_for_refine(source: &str, failed_diff: &str) -> Option<String> {
    let targets = hunk_target_lines(failed_diff);
    if targets.is_empty() {
        return None;
    }
    let lines: Vec<&str> = source.lines().collect();
    if lines.len() <= REFINE_WINDOW_RADIUS * 3 {
        return None;
    }
    let mut ranges: Vec<(usize, usize)> = targets
        .iter()
        .map(|&t| {
            let center = t.saturating_sub(1).min(lines.len().saturating_sub(1));
            (
                center.saturating_sub(REFINE_WINDOW_RADIUS),
                (center + REFINE_WINDOW_RADIUS + 1).min(lines.len()),
            )
        })
        .collect();
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        match merged.last_mut() {
            Some((_, last_end)) if start <= *last_end => {
                *last_end = (*last_end).max(end);
            }
            _ => merged.push((start, end)),
        }
    }
    let covered: usize = merged.iter().map(|(a, b)| b - a).sum();
    if covered.saturating_mul(10) >= lines.len().saturating_mul(6) {
        return None;
    }
    let mut out = String::with_capacity(covered * 64);
    let mut cursor = 0usize;
    for (start, end) in merged {
        if start > cursor {
            out.push_str(&format!("(lines {}-{} omitted)\n", cursor + 1, start));
        }
        for (idx, line) in lines.iter().enumerate().take(end).skip(start) {
            out.push_str(&format!("L{}: {}\n", idx + 1, line));
        }
        cursor = end;
    }
    if cursor < lines.len() {
        out.push_str(&format!("(lines {}-{} omitted)\n", cursor + 1, lines.len()));
    }
    Some(out)
}

fn push_source_section(body: &mut String, source: &str, failed_diff: &str) {
    match windowed_source_for_refine(source, failed_diff) {
        Some(windowed) => {
            body.push_str(
                "# Current file contents (excerpt around the failure sites; `L<n>:` prefixes are the real 1-based file line numbers - use them for the `@@` headers, but do NOT include the `L<n>:` prefix itself in diff lines)\n\n```\n",
            );
            body.push_str(&windowed);
            body.push_str("```\n\n# Failed diff\n\n```diff\n");
        }
        None => {
            body.push_str("# Current file contents\n\n```\n");
            body.push_str(source);
            if !source.ends_with('\n') {
                body.push('\n');
            }
            body.push_str("```\n\n# Failed diff\n\n```diff\n");
        }
    }
}

#[must_use]
pub fn build_refine_user_prompt(source: &str, failed_diff: &str, hint: Option<&str>) -> String {
    let mut body = String::with_capacity(source.len() + failed_diff.len() + 256);
    push_source_section(&mut body, source, failed_diff);
    body.push_str(failed_diff);
    if !failed_diff.ends_with('\n') {
        body.push('\n');
    }
    body.push_str("```\n");
    if let Some(h) = hint {
        if !h.is_empty() {
            body.push_str("\n# Hint\n\n");
            body.push_str(h);
            body.push('\n');
        }
    }
    body.push_str("\n# Task\n\nReturn ONLY the corrected unified diff.\n");
    body
}

#[must_use]
pub fn build_refine_user_prompt_v2(
    source: &str,
    failed_diff: &str,
    hint: Option<&str>,
    failure: Option<&crate::apply_model::llm_refine::FailureKind>,
    prev: Option<&crate::apply_model::llm_refine::PreviousAttempt>,
) -> String {
    let mut body = String::with_capacity(source.len() + failed_diff.len() + 512);
    push_source_section(&mut body, source, failed_diff);
    body.push_str(failed_diff);
    if !failed_diff.ends_with('\n') {
        body.push('\n');
    }
    body.push_str("```\n");

    if let Some(h) = hint {
        if !h.is_empty() {
            body.push_str("\n# Hint\n\n");
            body.push_str(h);
            body.push('\n');
        }
    }

    if let Some(kind) = failure {
        body.push_str("\n# Failure type\n\n");
        match kind {
            crate::apply_model::llm_refine::FailureKind::ContextMismatch => {
                body.push_str("context_mismatch  -  the failed diff's pre-image lines do not match the current file. Re-read the file contents above and locate the actual surrounding context.\n");
            }
            crate::apply_model::llm_refine::FailureKind::LineDrift { delta } => {
                body.push_str(&format!(
                    "line_drift  -  the previous attempt landed {delta} lines off target. Use the *current* line numbers from the file above when emitting the new `@@` header.\n"
                ));
            }
            crate::apply_model::llm_refine::FailureKind::TreeSitterError { node_kind, line } => {
                body.push_str(&format!(
                    "tree_sitter_error at line {line}: {node_kind}. The post-apply file fails to parse  -  fix the structural error around line {line} in the diff. {context}\n",
                    context = surrounding_lines(source, *line, 3)
                ));
            }
            crate::apply_model::llm_refine::FailureKind::CompileError { code, line } => {
                let label = code.as_deref().unwrap_or("unknown");
                body.push_str(&format!(
                    "compile_error[{label}] at line {line}. {context}\n",
                    context = surrounding_lines(source, *line, 3)
                ));
            }
            crate::apply_model::llm_refine::FailureKind::BracketUnbalanced { line } => {
                body.push_str(&format!(
                    "bracket_unbalanced at line {line}. {context}\n",
                    context = surrounding_lines(source, *line, 3)
                ));
            }
        }
    }

    if let Some(p) = prev {
        body.push_str("\n# Previous attempt\n\nThis is the diff your *previous* response produced, plus why it failed. Do NOT repeat the same diff  -  read the failure above and produce a *different* correction.\n\n```diff\n");
        body.push_str(&p.diff);
        if !p.diff.ends_with('\n') {
            body.push('\n');
        }
        body.push_str("```\n\nFailure reason: ");
        body.push_str(&p.error);
        body.push('\n');
    }

    body.push_str(REFINE_USER_PROMPT_V2_FOOTER);
    body
}

fn surrounding_lines(source: &str, line: u32, radius: u32) -> String {
    if source.is_empty() || line == 0 {
        return String::new();
    }
    let lines: Vec<&str> = source.lines().collect();
    if lines.is_empty() {
        return String::new();
    }
    let center = (line as usize).saturating_sub(1).min(lines.len() - 1);
    let lo = center.saturating_sub(radius as usize);
    let hi = (center + radius as usize + 1).min(lines.len());
    let mut out = String::from("Context:\n```\n");
    for (idx, l) in lines.iter().enumerate().take(hi).skip(lo) {
        let trimmed: &str = crate::util::truncate_str_bytes(l, 200);
        out.push_str(&format!("L{}: {}\n", idx + 1, trimmed.trim_end()));
    }
    out.push_str("```");
    out
}
