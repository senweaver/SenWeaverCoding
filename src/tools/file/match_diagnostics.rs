// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.


const MAX_SNIPPET_LINES: usize = 24;
const MAX_OLD_PREVIEW_LINES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MismatchKind {
    WhitespaceOnly,
    LineEndingOnly,
    ByteOrderMark,
    NearMatch,
    NotFound,
}

pub struct MatchDiagnosis {
    pub kind: MismatchKind,
    pub message: String,
}

fn norm_line(s: &str) -> &str {
    s.trim_end_matches(['\r', '\n']).trim()
}

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

pub fn diagnose(content: &str, old_string: &str) -> Option<MatchDiagnosis> {
    if old_string.is_empty() {
        return None;
    }

    if let Some(stripped) = content.strip_prefix('\u{feff}') {
        if !old_string.starts_with('\u{feff}') && stripped.contains(old_string) {
            return Some(MatchDiagnosis {
                kind: MismatchKind::ByteOrderMark,
                message:
                    "The text exists but the file begins with a UTF-8 byte-order mark (BOM) that \
                     your old_string does not include. Target text after the BOM, or edit a \
                     unique region that does not start at the very first byte of the file."
                        .to_string(),
            });
        }
    }

    let content_lf = content.replace("\r\n", "\n");
    let old_lf = old_string.replace("\r\n", "\n");
    if content.contains('\r') != old_string.contains('\r') && content_lf.contains(&old_lf) {
        return Some(MatchDiagnosis {
            kind: MismatchKind::LineEndingOnly,
            message:
                "The text exists but the line endings differ (CRLF vs LF). This difference is \
                 normally reconciled automatically; seeing this error means the match sits \
                 outside the requested scope or the file mixes unusual line endings. Include \
                 more surrounding context in old_string, adjust the scope, or pass near_line \
                 to anchor the intended match."
                    .to_string(),
        });
    }

    let source_lines: Vec<&str> = content.split_inclusive('\n').collect();
    let mut old_lines: Vec<&str> = old_lf.split('\n').collect();
    if old_lf.ends_with('\n') && old_lines.last() == Some(&"") {
        old_lines.pop();
    }
    if old_lines.is_empty() {
        old_lines.push("");
    }
    let win_len = old_lines.len().max(1);

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
        best_score = window_similarity(&source_lines, &old_lines);
    }

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

pub fn find_whitespace_insensitive_unique(
    content: &str,
    old_string: &str,
    new_string: &str,
) -> Option<(usize, usize, String)> {
    if old_string.trim().is_empty() {
        return None;
    }
    let source_lines: Vec<&str> = content.split_inclusive('\n').collect();
    let old_lf = old_string.replace("\r\n", "\n");
    let mut old_lines: Vec<&str> = old_lf.split('\n').collect();
    if old_lf.ends_with('\n') && old_lines.last() == Some(&"") {
        old_lines.pop();
    }
    if old_lines.is_empty() {
        return None;
    }
    let win_len = old_lines.len();
    if source_lines.len() < win_len {
        return None;
    }

    const SCAN_BUDGET: usize = 4_000_000;
    let positions = source_lines.len() - win_len + 1;
    if positions.saturating_mul(win_len) > SCAN_BUDGET {
        return None;
    }

    let mut found: Option<usize> = None;
    for start in 0..positions {
        if is_trimmed_equal(&source_lines[start..start + win_len], &old_lines) {
            if found.is_some() {
                return None;
            }
            found = Some(start);
        }
    }
    let start_line = found?;

    let byte_start: usize = source_lines[..start_line].iter().map(|l| l.len()).sum();
    let matched_len: usize = source_lines[start_line..start_line + win_len]
        .iter()
        .map(|l| l.len())
        .sum();
    let last_line = source_lines[start_line + win_len - 1];
    let trailing_nl = if last_line.ends_with("\r\n") {
        2
    } else if last_line.ends_with('\n') {
        1
    } else {
        0
    };
    let byte_end = if old_lf.ends_with('\n') {
        byte_start + matched_len
    } else {
        byte_start + matched_len - trailing_nl
    };

    let file_indent = leading_ws(source_lines[start_line]);
    let old_indent = leading_ws(old_lines[0]);
    let adjusted_new = if file_indent == old_indent {
        new_string.to_string()
    } else {
        reindent_block(new_string, old_indent, file_indent)
    };
    Some((byte_start, byte_end, adjusted_new))
}

fn leading_ws(line: &str) -> &str {
    let end = line
        .char_indices()
        .find(|(_, c)| *c != ' ' && *c != '\t')
        .map(|(i, _)| i)
        .unwrap_or(line.len());
    &line[..end]
}

fn reindent_block(text: &str, from_indent: &str, to_indent: &str) -> String {
    let mut out = String::with_capacity(text.len() + 64);
    for line in text.split_inclusive('\n') {
        out.push_str(&reindent_line(line, from_indent, to_indent));
    }
    out
}

fn reindent_line(line: &str, from_indent: &str, to_indent: &str) -> String {
    if line.trim().is_empty() {
        return line.to_string();
    }
    if let Some(rest) = line.strip_prefix(from_indent) {
        format!("{to_indent}{rest}")
    } else {
        line.to_string()
    }
}

pub fn line_of_offset(content: &str, byte_pos: usize) -> (usize, &str) {
    let clamped = byte_pos.min(content.len());
    let line_no = content[..clamped].matches('\n').count() + 1;
    let line_start = content[..clamped].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let line_end = content[line_start..]
        .find('\n')
        .map(|p| line_start + p)
        .unwrap_or(content.len());
    (line_no, content[line_start..line_end].trim_end_matches('\r'))
}

pub fn hit_lines_preview(content: &str, positions: &[usize], take: usize) -> String {
    let mut out = String::new();
    for pos in positions.iter().take(take) {
        let (line_no, line) = line_of_offset(content, *pos);
        out.push_str(&format!("  - line {line_no} : {line}\n"));
    }
    out
}

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
