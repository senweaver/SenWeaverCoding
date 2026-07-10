// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::ops::Range;

use super::traits::{Applier, ApplyError, ApplyOptions, ApplyOutcome};
use super::validator::validate_bytes;

#[derive(Debug, Clone)]
pub struct NamedScope {
    pub kind: super::edit_op::ScopeKind,
    pub name: String,
    pub byte_range: Range<usize>,

    pub line_range: Range<usize>,
}

#[derive(Debug, Clone)]
pub struct LocateContext<'a> {
    pub ideal_line: usize,
    pub cursor_scope: Option<Range<usize>>,
    pub named_scopes: &'a [NamedScope],
    pub allow_full_scan: bool,
}

impl LocateContext<'_> {

    #[must_use]
    pub fn default_for(ideal_line: usize) -> Self {
        Self {
            ideal_line,
            cursor_scope: None,
            named_scopes: &[],
            allow_full_scan: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocateStrategy {
    Ideal,
    CursorScope,
    NamedScope(String),
    FullScan,
    Ambiguous,
}

impl LocateStrategy {

    #[must_use]
    pub fn tag(&self) -> &'static str {
        match self {
            LocateStrategy::Ideal => "ideal",
            LocateStrategy::CursorScope => "cursor_scope",
            LocateStrategy::NamedScope(_) => "named_scope",
            LocateStrategy::FullScan => "full_scan",
            LocateStrategy::Ambiguous => "ambiguous",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LocateOutcome {

    pub pos: usize,

    pub drift: usize,
    pub strategy: LocateStrategy,
}

#[derive(Debug, Clone)]
pub enum LocateError {

    NotFound,

    Ambiguous { candidates: Vec<NamedScope> },
}

#[derive(Debug, Default, Clone, Copy)]
pub struct HeuristicApplier;

impl Applier for HeuristicApplier {
    fn apply(
        &self,
        source: &str,
        diff: &str,
        opts: &ApplyOptions,
    ) -> Result<ApplyOutcome, ApplyError> {
        apply_unified_diff(source, diff, opts)
    }
    fn name(&self) -> &'static str {
        "heuristic"
    }
}

#[derive(Debug, Clone)]
struct Hunk {

    old_start: usize,

    lines: Vec<String>,
}

#[derive(Debug, Clone)]
struct ParsedHunk {
    old_start: usize,

    old_lines: Vec<String>,

    new_lines: Vec<String>,
}

pub fn apply_unified_diff(
    source: &str,
    diff: &str,
    opts: &ApplyOptions,
) -> Result<ApplyOutcome, ApplyError> {
    let empty_ctx = LocateContext {
        ideal_line: 0,
        cursor_scope: None,
        named_scopes: &[],
        allow_full_scan: true,
    };
    apply_unified_diff_with_ctx(source, diff, opts, &empty_ctx)
}

pub fn apply_unified_diff_with_ctx(
    source: &str,
    diff: &str,
    opts: &ApplyOptions,
    ctx: &LocateContext<'_>,
) -> Result<ApplyOutcome, ApplyError> {
    let hunks = parse_hunks(diff)?;
    if hunks.is_empty() {
        return Err(ApplyError::EmptyDiff);
    }
    let parsed: Vec<ParsedHunk> = hunks.into_iter().map(parse_hunk_lines).collect();

    let source_lines: Vec<&str> = source.split_inclusive('\n').collect();
    // Match the file's existing newline style for inserted lines so we never turn
    // a CRLF file into a mixed LF/CRLF file.
    let newline: &str = if source.contains("\r\n") { "\r\n" } else { "\n" };
    let source_had_trailing_newline = source.ends_with('\n');
    let mut cursor = 0usize;
    let mut output: Vec<String> = Vec::with_capacity(source_lines.len());
    let mut hunks_exact = 0usize;
    let mut hunks_fuzzy = 0usize;
    let mut hunks_failed = 0usize;

    let anchor_scope: Option<&NamedScope> = if ctx.named_scopes.is_empty() {
        None
    } else if ctx.ideal_line > 0 {
        ctx.named_scopes
            .iter()
            .find(|s| s.line_range.contains(&ctx.ideal_line))
            .or_else(|| ctx.named_scopes.first())
    } else {
        ctx.named_scopes.first()
    };

    for hunk in &parsed {
        let ideal = hunk.old_start.saturating_sub(1);
        let located = if let Some(scope) = anchor_scope {

            let scope_start = scope.line_range.start.saturating_sub(1);
            let scope_end = scope
                .line_range
                .end
                .min(source_lines.len())
                .max(scope_start);
            let subslice_cursor = cursor.max(scope_start);
            let search_ideal = ideal.max(scope_start);
            let result = if subslice_cursor < scope_end {

                let hit = locate_hunk(
                    &source_lines,
                    search_ideal,
                    subslice_cursor,
                    &hunk.old_lines,
                    opts.max_fuzz,
                );
                hit.filter(|(pos, _)| *pos + hunk.old_lines.len() <= scope_end)
            } else {
                None
            };
            if result.is_some() {
                crate::observability::code_intel_metrics::incr_apply_hunk_anchor_hit_named_scope();
            } else {
                crate::observability::code_intel_metrics::incr_apply_hunk_anchor_fallback_full_scan();
            }
            result.or_else(|| {
                if ctx.allow_full_scan {
                    locate_hunk(&source_lines, ideal, cursor, &hunk.old_lines, opts.max_fuzz)
                } else {
                    None
                }
            })
        } else {
            locate_hunk(&source_lines, ideal, cursor, &hunk.old_lines, opts.max_fuzz)
        };

        match located {
            Some((pos, drift)) => {
                for line in &source_lines[cursor..pos] {
                    output.push((*line).to_string());
                }
                for new_line in &hunk.new_lines {
                    output.push(with_newline(new_line, newline));
                }
                cursor = pos + hunk.old_lines.len();
                if drift == 0 {
                    hunks_exact += 1;
                } else {
                    hunks_fuzzy += 1;
                }
            }
            None => {
                hunks_failed += 1;
            }
        }
    }

    for line in &source_lines[cursor..] {
        output.push((*line).to_string());
    }

    crate::observability::subsystem_metrics::incr_apply_model_exact(hunks_exact as u64);
    crate::observability::subsystem_metrics::incr_apply_model_fuzzy(hunks_fuzzy as u64);
    if hunks_failed > 0 {
        crate::observability::subsystem_metrics::incr_apply_model_failed(hunks_failed as u64);
        return Err(ApplyError::HunkMismatch {
            failed: hunks_failed,
            total: parsed.len(),
        });
    }

    let mut applied: String = output.concat();
    // Respect the original file's lack of a trailing newline instead of forcing one.
    if !source_had_trailing_newline {
        if applied.ends_with("\r\n") {
            applied.truncate(applied.len() - 2);
        } else if applied.ends_with('\n') {
            applied.truncate(applied.len() - 1);
        }
    }

    if opts.validate {
        let report = validate_bytes(&applied);
        if !report.is_ok() {
            return Err(ApplyError::Validation {
                reasons: report.issues.iter().map(|i| i.message.clone()).collect(),
            });
        }
    }

    Ok(ApplyOutcome {
        applied,
        hunks_exact,
        hunks_fuzzy,
        hunks_failed,
    })
}

fn parse_hunks(diff: &str) -> Result<Vec<Hunk>, ApplyError> {
    let mut hunks = Vec::new();
    let mut current: Option<Hunk> = None;
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("@@ ") {
            if let Some(h) = current.take() {
                hunks.push(h);
            }
            let old_start = parse_old_start(rest).map_err(ApplyError::Parse)?;
            current = Some(Hunk {
                old_start,
                lines: Vec::new(),
            });
        } else if let Some(h) = current.as_mut() {

            if line.starts_with("---") || line.starts_with("+++") || line.starts_with("diff ") {
                continue;
            }
            h.lines.push(line.to_string());
        }

    }
    if let Some(h) = current {
        hunks.push(h);
    }
    Ok(hunks)
}

fn parse_old_start(rest: &str) -> Result<usize, String> {

    let minus = rest
        .split_whitespace()
        .next()
        .ok_or_else(|| "hunk header missing '-' component".to_string())?;
    let stripped = minus.strip_prefix('-').unwrap_or(minus);
    let start_str = stripped.split(',').next().unwrap_or(stripped);
    start_str
        .parse::<usize>()
        .map_err(|e| format!("invalid old_start '{start_str}': {e}"))
}

fn parse_hunk_lines(h: Hunk) -> ParsedHunk {
    let mut old_lines = Vec::new();
    let mut new_lines = Vec::new();
    for raw in h.lines {
        if raw.starts_with('\\') {

            continue;
        }
        let (tag, body) = split_first_char(&raw);
        match tag {
            Some(' ') => {
                old_lines.push(body.to_string());
                new_lines.push(body.to_string());
            }
            Some('-') => old_lines.push(body.to_string()),
            Some('+') => new_lines.push(body.to_string()),
            _ => {

                old_lines.push(raw.clone());
                new_lines.push(raw);
            }
        }
    }
    ParsedHunk {
        old_start: h.old_start,
        old_lines,
        new_lines,
    }
}

fn split_first_char(s: &str) -> (Option<char>, &str) {
    let mut chars = s.chars();
    let first = chars.next();
    let rest = chars.as_str();
    (first, rest)
}

fn locate_hunk(
    source_lines: &[&str],
    ideal: usize,
    cursor: usize,
    old_lines: &[String],
    max_fuzz: usize,
) -> Option<(usize, usize)> {
    if old_lines.is_empty() {
        // Pure-insertion hunk: there is no old context to verify against, so the
        // placement is purely positional (by line number).
        let pos = ideal.min(source_lines.len()).max(cursor);
        return Some((pos, pos.abs_diff(ideal)));
    }

    let start = ideal.max(cursor);
    if matches_at(source_lines, start, old_lines) {
        return Some((start, 0));
    }

    for delta in 1..=max_fuzz {
        let up = start.checked_sub(delta).filter(|p| *p >= cursor);
        if let Some(p) = up
            && matches_at(source_lines, p, old_lines)
        {
            return Some((p, delta));
        }
        let down = start.checked_add(delta);
        if let Some(p) = down
            && p + old_lines.len() <= source_lines.len()
            && matches_at(source_lines, p, old_lines)
        {
            return Some((p, delta));
        }
    }

    const FALLBACK_SCAN_WINDOW: usize = 5_000;
    let scan_end = source_lines.len().saturating_sub(old_lines.len());
    let win_start = cursor.max(ideal.saturating_sub(FALLBACK_SCAN_WINDOW));
    let win_end = scan_end.min(ideal.saturating_add(FALLBACK_SCAN_WINDOW));
    if win_start <= win_end {
        // Scan outward from the ideal position so short/ambiguous context
        // (for example a lone `}` line) resolves to the closest candidate
        // instead of silently landing on the first match in the window.
        let anchor = ideal.clamp(win_start, win_end);
        if matches_at(source_lines, anchor, old_lines) {
            return Some((anchor, anchor.abs_diff(ideal)));
        }
        let span = win_end - win_start;
        for delta in 1..=span {
            if let Some(p) = anchor.checked_sub(delta).filter(|p| *p >= win_start) {
                if matches_at(source_lines, p, old_lines) {
                    return Some((p, p.abs_diff(ideal)));
                }
            }
            if let Some(p) = anchor.checked_add(delta).filter(|p| *p <= win_end) {
                if matches_at(source_lines, p, old_lines) {
                    return Some((p, p.abs_diff(ideal)));
                }
            }
        }
    }
    None
}

fn matches_at(source_lines: &[&str], at: usize, old_lines: &[String]) -> bool {
    if at + old_lines.len() > source_lines.len() {
        return false;
    }
    for (i, expected) in old_lines.iter().enumerate() {

        let actual = source_lines[at + i].trim_end_matches(['\n', '\r']);
        let exp = expected.trim_end_matches(['\n', '\r']);
        if actual != exp {
            return false;
        }
    }
    true
}

pub fn locate_hunk_with_ctx(
    source_lines: &[&str],
    cursor: usize,
    old_lines: &[String],
    max_fuzz: usize,
    ctx: &LocateContext<'_>,
) -> Result<LocateOutcome, LocateError> {
    let ideal = ctx.ideal_line.saturating_sub(1);

    if !ctx.named_scopes.is_empty() {
        let scope = ctx
            .named_scopes
            .iter()
            .find(|s| s.line_range.contains(&ctx.ideal_line))
            .or_else(|| ctx.named_scopes.first());
        if let Some(scope) = scope {
            let scope_start = scope.line_range.start.saturating_sub(1);
            let scope_end = scope
                .line_range
                .end
                .min(source_lines.len())
                .max(scope_start);
            let subslice_cursor = cursor.max(scope_start);
            if subslice_cursor < scope_end {
                if let Some((pos, drift)) = locate_hunk(
                    source_lines,
                    ideal.max(scope_start),
                    subslice_cursor,
                    old_lines,
                    max_fuzz,
                ) {
                    if pos + old_lines.len() <= scope_end {
                        return Ok(LocateOutcome {
                            pos,
                            drift,
                            strategy: LocateStrategy::NamedScope(scope.name.clone()),
                        });
                    }
                }
            }
        }

    }

    if ctx.allow_full_scan {
        match locate_hunk(source_lines, ideal, cursor, old_lines, max_fuzz) {
            Some((pos, drift)) => {
                let strategy = if drift == 0 {
                    LocateStrategy::Ideal
                } else if drift <= max_fuzz {
                    LocateStrategy::Ideal
                } else {
                    LocateStrategy::FullScan
                };
                Ok(LocateOutcome {
                    pos,
                    drift,
                    strategy,
                })
            }
            None => Err(LocateError::NotFound),
        }
    } else {

        let start = ideal.max(cursor);
        if matches_at(source_lines, start, old_lines) {
            return Ok(LocateOutcome {
                pos: start,
                drift: 0,
                strategy: LocateStrategy::Ideal,
            });
        }
        for delta in 1..=max_fuzz {
            if let Some(p) = start.checked_sub(delta).filter(|p| *p >= cursor)
                && matches_at(source_lines, p, old_lines)
            {
                return Ok(LocateOutcome {
                    pos: p,
                    drift: delta,
                    strategy: LocateStrategy::Ideal,
                });
            }
            let down = start.checked_add(delta);
            if let Some(p) = down
                && p + old_lines.len() <= source_lines.len()
                && matches_at(source_lines, p, old_lines)
            {
                return Ok(LocateOutcome {
                    pos: p,
                    drift: delta,
                    strategy: LocateStrategy::Ideal,
                });
            }
        }
        Err(LocateError::NotFound)
    }
}

fn with_newline(line: &str, newline: &str) -> String {
    let core = line.strip_suffix('\n').unwrap_or(line);
    let core = core.strip_suffix('\r').unwrap_or(core);
    format!("{core}{newline}")
}
