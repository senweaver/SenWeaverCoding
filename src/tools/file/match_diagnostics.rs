// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

//! Edit-match diagnostics.
//!
//! When an exact `old_string` match fails, a bare "not found" forces the model
//! to guess blindly. Instead we analyze the file's ACTUAL content, locate the
//! closest region, classify why it did not match (whitespace/indentation,
//! line endings, or genuine drift), and return the real text with line numbers
//! so the next edit can be grounded in the document rather than a hallucination.

const MAX_SNIPPET_LINES: usize = 24;
const MAX_OLD_PREVIEW_LINES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MismatchKind {
    // Content matches after trimming each line: only leading/trailing
    // whitespace or indentation differs.
    WhitespaceOnly,
    // Only line endings differ (CRLF vs LF) once whitespace is normalized.
    LineEndingOnly,
    // A close-but-not-identical region exists (some lines differ).
    NearMatch,
    // Nothing resembling old_string was found.
    NotFound,
}

pub struct MatchDiagnosis {
    pub kind: MismatchKind,
    pub message: String,
}

fn norm_line(s: &str) -> &str {
    s.trim_end_matches(['\r', '\n']).trim()
}

// Similarity of two line slices, scored over NON-BLANK target lines only, in
// order. Blank lines carry no content signal, so counting them would let a
// coincidental run of empty lines inflate the score and point at the wrong
// region. Returns 0 when the target has no meaningful (non-blank) lines.
fn window_similarity(window: &[&str], target: &[&str]) -> f32 {
    let mut meaningful = 0usize;
    let mut matched = 0usize;
    for (a, b) in window.iter().zip(target.iter()) {
        let tb = norm_line(b);
        if tb.is_empty() {
            continue;
        }
        meaningful += 1;
        if norm_line(a) == tb {
            matched += 1;
        }
    }
    if meaningful == 0 {
        return 0.0;
    }
    matched as f32 / meaningful as f32
}

// True when every line matches after trimming: the two blocks are identical
// apart from leading/trailing whitespace and indentation.
fn is_trimmed_equal(window: &[&str], target: &[&str]) -> bool {
    if window.len() != target.len() {
        return false;
    }
    window
        .iter()
        .zip(target.iter())
        .all(|(a, b)| norm_line(a) == norm_line(b))
}

fn render_snippet(source_lines: &[&str], start: usize, len: usize) -> String {
    let end = (start + len).min(source_lines.len());
    let shown_end = end.min(start + MAX_SNIPPET_LINES);
    let mut out = String::new();
    for (offset, line) in source_lines[start..shown_end].iter().enumerate() {
        let line_no = start + offset + 1;
        out.push_str(&format!("{line_no:>6}\u{2502}{}\n", line.trim_end_matches(['\r', '\n'])));
    }
    if shown_end < end {
        out.push_str(&format!("      \u{2502}... ({} more lines)\n", end - shown_end));
    }
    out
}

fn old_preview(old_lines: &[&str]) -> String {
    let mut out = String::new();
    for line in old_lines.iter().take(MAX_OLD_PREVIEW_LINES) {
        out.push_str(&format!("       {}\n", line));
    }
    if old_lines.len() > MAX_OLD_PREVIEW_LINES {
        out.push_str(&format!(
            "       ... ({} more lines)\n",
            old_lines.len() - MAX_OLD_PREVIEW_LINES
        ));
    }
    out
}

// Analyzes why `old_string` did not match `content` and produces actionable,
// document-grounded guidance. Returns None only when old_string is empty.
pub fn diagnose(content: &str, old_string: &str) -> Option<MatchDiagnosis> {
    if old_string.is_empty() {
        return None;
    }

    // CRLF-normalized whole-string containment: the content is present but the
    // file uses different line endings than the supplied old_string.
    let content_lf = content.replace("\r\n", "\n");
    let old_lf = old_string.replace("\r\n", "\n");
    if content.contains('\r') != old_string.contains('\r') && content_lf.contains(&old_lf) {
        return Some(MatchDiagnosis {
            kind: MismatchKind::LineEndingOnly,
            message:
                "The text exists but the line endings differ (the file uses \
                 CRLF/LF differently than your old_string). Re-read the file and copy the \
                 exact bytes, or supply old_string with matching line endings."
                    .to_string(),
        });
    }

    let source_lines: Vec<&str> = content.split_inclusive('\n').collect();
    let mut old_lines: Vec<&str> = old_lf.split('\n').collect();
    // Drop the empty element produced by a trailing newline so the window
    // length matches the visible content (keeps whitespace detection and the
    // echoed snippet aligned with what the model actually supplied).
    if old_lf.ends_with('\n') && old_lines.last() == Some(&"") {
        old_lines.pop();
    }
    if old_lines.is_empty() {
        old_lines.push("");
    }
    let win_len = old_lines.len().max(1);

    // Bound the sliding-window scan so a failed match on a very large file
    // cannot cost hundreds of ms. Beyond the budget we skip straight to the
    // cheaper first-line anchor search below.
    const SCAN_BUDGET: usize = 4_000_000;
    let mut best_start = 0usize;
    let mut best_score = 0.0f32;
    if source_lines.len() >= win_len {
        let positions = source_lines.len() - win_len + 1;
        if positions.saturating_mul(win_len) <= SCAN_BUDGET {
            for start in 0..positions {
                let window = &source_lines[start..start + win_len];
                let score = window_similarity(window, &old_lines);
                if score > best_score {
                    best_score = score;
                    best_start = start;
                    if (score - 1.0).abs() < f32::EPSILON {
                        break;
                    }
                }
            }
        }
    } else {
        // old_string is longer than the file: compare against the whole file.
        best_score = window_similarity(&source_lines, &old_lines);
    }

    // Only leading/trailing whitespace differs when EVERY line of the best
    // window (blanks included) is trimmed-equal to old_string. Checking the
    // real window — not just the (blank-ignoring) score — avoids mislabeling a
    // near match as whitespace-only.
    let best_window_trimmed_equal = source_lines.len() >= win_len
        && best_start + win_len <= source_lines.len()
        && is_trimmed_equal(&source_lines[best_start..best_start + win_len], &old_lines);
    if best_window_trimmed_equal {
        let snippet = render_snippet(&source_lines, best_start, win_len);
        return Some(MatchDiagnosis {
            kind: MismatchKind::WhitespaceOnly,
            message: format!(
                "The text matches at line {} but the WHITESPACE/INDENTATION differs from your \
                 old_string. Copy the exact indentation shown below (leading spaces/tabs \
                 matter):\n{snippet}",
                best_start + 1
            ),
        });
    }

    // A reasonably similar region exists: show it so the model can correct.
    if best_score >= 0.34 {
        let snippet = render_snippet(&source_lines, best_start, win_len);
        let pct = (best_score * 100.0).round() as u32;
        return Some(MatchDiagnosis {
            kind: MismatchKind::NearMatch,
            message: format!(
                "old_string was not found exactly. The closest region is around line {} \
                 (~{pct}% of lines match). Actual file content there:\n{snippet}\nYour \
                 old_string was:\n{}\nRe-issue the edit using the exact text above (verbatim, \
                 including whitespace).",
                best_start + 1,
                old_preview(&old_lines)
            ),
        });
    }

    // Nothing close: maybe the first line exists elsewhere; point at it.
    let anchor = old_lines.iter().map(|l| norm_line(l)).find(|l| !l.is_empty());
    if let Some(anchor) = anchor {
        if let Some((idx, _)) = source_lines
            .iter()
            .enumerate()
            .find(|(_, l)| norm_line(l) == anchor)
        {
            let snippet = render_snippet(&source_lines, idx, win_len.min(6));
            return Some(MatchDiagnosis {
                kind: MismatchKind::NearMatch,
                message: format!(
                    "old_string was not found. Its first line does appear near line {} but the \
                     surrounding lines differ. Actual content there:\n{snippet}\nRe-read the \
                     file and base your edit on its current content.",
                    idx + 1
                ),
            });
        }
    }

    Some(MatchDiagnosis {
        kind: MismatchKind::NotFound,
        message:
            "old_string does not appear in the file, and no similar region was found. The file \
             may have changed, or you may be targeting the wrong path. Re-read the file with \
             file_read and base your edit on its current content."
                .to_string(),
    })
}

// Convenience wrapper producing the full error body appended after a
// "not found" headline. The read-before-edit hint is added only when the text
// is genuinely absent (NotFound): if a whitespace/near match was located the
// file obviously contains related content, so nagging to re-read would be
// noise and the concrete snippet is the actionable guidance instead.
pub fn failure_message(content: &str, old_string: &str, path_display: &str, had_read: bool) -> String {
    let mut msg = format!("old_string not found in '{path_display}'.");
    let kind = match diagnose(content, old_string) {
        Some(diag) => {
            msg.push('\n');
            msg.push_str(&diag.message);
            Some(diag.kind)
        }
        None => None,
    };
    let genuinely_absent = matches!(kind, None | Some(MismatchKind::NotFound));
    if !had_read && genuinely_absent {
        msg.push_str(
            "\nNote: this file was not read in the current session. Prefer calling file_read \
             first so edits are grounded in the file's real content.",
        );
    }
    msg
}
