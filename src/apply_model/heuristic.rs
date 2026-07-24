// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::ops::Range;

use super::traits::{Applier, ApplyError, ApplyOptions, ApplyOutcome};

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
            allow_full_scan: false,
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
        allow_full_scan: false,
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
    let mut parsed: Vec<ParsedHunk> = hunks
        .into_iter()
        .map(parse_hunk_lines)
        .collect::<Result<Vec<_>, _>>()?;
    parsed.sort_by_key(|h| h.old_start);

    let (bom_prefix, source): (&str, &str) = match source.strip_prefix('\u{feff}') {
        Some(rest) => ("\u{feff}", rest),
        None => ("", source),
    };

    let source_lines: Vec<&str> = source.split_inclusive('\n').collect();
    let newline: &str = if source.contains("\r\n") { "\r\n" } else { "\n" };
    let source_had_trailing_newline = source.ends_with('\n');
    let mut cursor = 0usize;
    let mut output: Vec<String> = Vec::with_capacity(source_lines.len());
    let mut hunks_exact = 0usize;
    let mut hunks_fuzzy = 0usize;
    let mut hunks_failed = 0usize;
    let mut failure_details: Vec<String> = Vec::new();

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
        let ideal = if hunk.old_lines.is_empty() {
            hunk.old_start
        } else {
            hunk.old_start.saturating_sub(1)
        };
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

                let hit = locate_hunk_detailed(
                    &source_lines,
                    search_ideal,
                    subslice_cursor,
                    &hunk.old_lines,
                    opts.max_fuzz,
                    true,
                );
                hit.filter(|h| h.pos + hunk.old_lines.len() <= scope_end)
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
                    locate_hunk_detailed(
                        &source_lines,
                        ideal,
                        cursor,
                        &hunk.old_lines,
                        opts.max_fuzz,
                        false,
                    )
                } else {
                    None
                }
            })
        } else {
            locate_hunk_detailed(
                &source_lines,
                ideal,
                cursor,
                &hunk.old_lines,
                opts.max_fuzz,
                false,
            )
        };

        match located {
            Some(hit) => {
                for line in &source_lines[cursor..hit.pos] {
                    output.push((*line).to_string());
                }
                for new_line in &hunk.new_lines {
                    let rewritten = match &hit.reindent {
                        Some((from, to)) => reindent_line(new_line, from, to),
                        None => new_line.clone(),
                    };
                    output.push(with_newline(&rewritten, newline));
                }
                cursor = hit.pos + hunk.old_lines.len();
                if hit.drift == 0 {
                    hunks_exact += 1;
                } else {
                    hunks_fuzzy += 1;
                }
            }
            None => {
                hunks_failed += 1;
                failure_details.push(diagnose_hunk_failure(
                    &source_lines,
                    hunks_failed,
                    ideal,
                    &hunk.old_lines,
                ));
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
            details: failure_details,
        });
    }

    let mut applied: String = output.concat();
    if !source_had_trailing_newline {
        if applied.ends_with("\r\n") {
            applied.truncate(applied.len() - 2);
        } else if applied.ends_with('\n') {
            applied.truncate(applied.len() - 1);
        }
    }

    if opts.validate {
        let report =
            super::validator::validate_edit(Some(source), &applied, opts.path.as_deref());
        if report.is_confident_failure() {
            return Err(ApplyError::Validation {
                reasons: report.issues.iter().map(|i| i.message.clone()).collect(),
            });
        }
        if !report.is_ok() {
            tracing::debug!(
                target: "apply_model.heuristic",
                issues = %report.advisory_summary(),
                "applied edit has advisory (non-tree-sitter bracket) validation warnings; \
                 proceeding without hard failure"
            );
        }
    }

    if !bom_prefix.is_empty() {
        applied.insert_str(0, bom_prefix);
    }

    Ok(ApplyOutcome {
        applied,
        hunks_exact,
        hunks_fuzzy,
        hunks_failed,
    })
}

fn parse_hunks(diff: &str) -> Result<Vec<Hunk>, ApplyError> {
    let lines: Vec<&str> = diff.lines().collect();
    let mut hunks = Vec::new();
    let mut current: Option<Hunk> = None;
    let mut remaining_old: Option<i64> = None;
    let mut remaining_new: Option<i64> = None;

    for (idx, line) in lines.iter().enumerate() {
        let line = *line;
        if let Some(rest) = line.strip_prefix("@@ ") {
            if let Some(h) = current.take() {
                hunks.push(h);
            }
            let header = parse_hunk_header(rest).map_err(ApplyError::Parse)?;
            remaining_old = header.old_count;
            remaining_new = header.new_count;
            current = Some(Hunk {
                old_start: header.old_start,
                lines: Vec::new(),
            });
            continue;
        }

        if line.starts_with("diff ") {
            if let Some(h) = current.take() {
                hunks.push(h);
            }
            remaining_old = None;
            remaining_new = None;
            continue;
        }

        if current.is_none() {
            continue;
        }

        let counts_known = remaining_old.is_some() && remaining_new.is_some();
        let body_open = match (remaining_old, remaining_new) {
            (Some(o), Some(n)) => o > 0 || n > 0,
            _ => true,
        };

        if line.starts_with("--- ") {
            let next_is_plus = lines
                .get(idx + 1)
                .is_some_and(|n| n.starts_with("+++ "));
            let is_header = !body_open || (!counts_known && next_is_plus);
            if is_header {
                if let Some(done) = current.take() {
                    hunks.push(done);
                }
                remaining_old = None;
                remaining_new = None;
                continue;
            }
        } else if line.starts_with("+++ ") && !body_open {
            continue;
        }

        let Some(h) = current.as_mut() else {
            continue;
        };

        match line.as_bytes().first() {
            Some(b'\\') => {
                h.lines.push(line.to_string());
            }
            Some(b'-') => {
                if let Some(o) = remaining_old.as_mut() {
                    *o -= 1;
                }
                h.lines.push(line.to_string());
            }
            Some(b'+') => {
                if let Some(n) = remaining_new.as_mut() {
                    *n -= 1;
                }
                h.lines.push(line.to_string());
            }
            Some(b' ') | None => {
                if let Some(o) = remaining_old.as_mut() {
                    *o -= 1;
                }
                if let Some(n) = remaining_new.as_mut() {
                    *n -= 1;
                }
                h.lines.push(line.to_string());
            }
            Some(_) => {
                if !body_open {
                    continue;
                }
                h.lines.push(line.to_string());
            }
        }
    }
    if let Some(h) = current {
        hunks.push(h);
    }
    Ok(hunks)
}

struct ParsedHunkHeader {
    old_start: usize,

    old_count: Option<i64>,

    new_count: Option<i64>,
}

fn parse_hunk_header(rest: &str) -> Result<ParsedHunkHeader, String> {
    let mut old_start: Option<usize> = None;
    let mut old_count: Option<i64> = None;
    let mut new_count: Option<i64> = None;
    for token in rest.split_whitespace() {
        if token == "@@" {
            break;
        }
        if let Some(spec) = token.strip_prefix('-') {
            if old_start.is_none() {
                let (start, count) = parse_range_spec(spec);
                old_start = start;
                old_count = count;
            }
        } else if let Some(spec) = token.strip_prefix('+') {
            if new_count.is_none() {
                let (_, count) = parse_range_spec(spec);
                new_count = count;
            }
        }
    }
    let old_start = old_start
        .ok_or_else(|| format!("hunk header missing valid '-' range: @@ {rest}"))?;
    Ok(ParsedHunkHeader {
        old_start,
        old_count,
        new_count,
    })
}

fn parse_range_spec(spec: &str) -> (Option<usize>, Option<i64>) {
    let mut parts = spec.splitn(2, ',');
    let start = parts.next().and_then(|s| s.parse::<usize>().ok());
    if start.is_none() {
        return (None, None);
    }
    let count = parts.next().and_then(|s| s.parse::<i64>().ok());
    (start, count)
}

fn parse_hunk_lines(h: Hunk) -> Result<ParsedHunk, ApplyError> {
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
            None => {
                old_lines.push(String::new());
                new_lines.push(String::new());
            }
            Some(_) => {
                return Err(ApplyError::Parse(format!(
                    "hunk line missing +/-/  prefix: {raw}"
                )));
            }
        }
    }
    Ok(ParsedHunk {
        old_start: h.old_start,
        old_lines,
        new_lines,
    })
}

fn split_first_char(s: &str) -> (Option<char>, &str) {
    let mut chars = s.chars();
    let first = chars.next();
    let rest = chars.as_str();
    (first, rest)
}

#[derive(Debug, Clone)]
struct LocatedHunk {
    pos: usize,
    drift: usize,

    reindent: Option<(String, String)>,
}

impl LocatedHunk {
    fn exact(pos: usize, drift: usize) -> Self {
        Self {
            pos,
            drift,
            reindent: None,
        }
    }
}

fn locate_hunk(
    source_lines: &[&str],
    ideal: usize,
    cursor: usize,
    old_lines: &[String],
    max_fuzz: usize,
    allow_ws_insensitive: bool,
) -> Option<(usize, usize)> {
    locate_hunk_detailed(
        source_lines,
        ideal,
        cursor,
        old_lines,
        max_fuzz,
        allow_ws_insensitive,
    )
    .map(|h| (h.pos, h.drift))
}

fn locate_hunk_detailed(
    source_lines: &[&str],
    ideal: usize,
    cursor: usize,
    old_lines: &[String],
    max_fuzz: usize,
    allow_ws_insensitive: bool,
) -> Option<LocatedHunk> {
    if old_lines.is_empty() {
        let pos = ideal.min(source_lines.len()).max(cursor);
        let drift = pos.abs_diff(ideal);
        if drift > max_fuzz {
            return None;
        }
        return Some(LocatedHunk::exact(pos, drift));
    }

    let start = ideal.max(cursor);
    if matches_at(source_lines, start, old_lines) {
        return Some(LocatedHunk::exact(start, 0));
    }

    if max_fuzz == 0 {
        return None;
    }

    for delta in 1..=max_fuzz {
        let up = start
            .checked_sub(delta)
            .filter(|p| *p >= cursor)
            .filter(|p| matches_at(source_lines, *p, old_lines));
        let down = start
            .checked_add(delta)
            .filter(|p| p + old_lines.len() <= source_lines.len())
            .filter(|p| matches_at(source_lines, *p, old_lines));
        match (up, down) {
            (Some(_), Some(_)) => return None,
            (Some(p), None) | (None, Some(p)) => return Some(LocatedHunk::exact(p, delta)),
            (None, None) => {}
        }
    }

    const FALLBACK_SCAN_WINDOW: usize = 200;
    let scan_end = source_lines.len().saturating_sub(old_lines.len());
    let win = FALLBACK_SCAN_WINDOW.min(source_lines.len() / 10).max(32);
    let win_start = cursor.max(ideal.saturating_sub(win));
    let win_end = scan_end.min(ideal.saturating_add(win));
    if win_start <= win_end {
        let anchor = ideal.clamp(win_start, win_end);
        if let Some(hit) = scan_window_for_match(
            source_lines,
            old_lines,
            anchor,
            ideal,
            win_start,
            win_end,
            false,
        ) {
            return Some(hit);
        }
        if allow_ws_insensitive {
            return scan_window_for_match(
                source_lines,
                old_lines,
                anchor,
                ideal,
                win_start,
                win_end,
                true,
            );
        }
    }
    None
}

fn scan_window_for_match(
    source_lines: &[&str],
    old_lines: &[String],
    anchor: usize,
    ideal: usize,
    win_start: usize,
    win_end: usize,
    ws_insensitive: bool,
) -> Option<LocatedHunk> {
    let matcher: fn(&[&str], usize, &[String]) -> bool = if ws_insensitive {
        matches_at_ws_insensitive
    } else {
        matches_at
    };
    let mut matches: Vec<(usize, usize)> = Vec::new();
    if matcher(source_lines, anchor, old_lines) {
        matches.push((anchor, anchor.abs_diff(ideal)));
    }
    let span = win_end - win_start;
    for delta in 1..=span {
        if let Some(p) = anchor.checked_sub(delta).filter(|p| *p >= win_start) {
            if matcher(source_lines, p, old_lines) {
                matches.push((p, p.abs_diff(ideal)));
            }
        }
        if let Some(p) = anchor.checked_add(delta).filter(|p| *p <= win_end) {
            if matcher(source_lines, p, old_lines) {
                matches.push((p, p.abs_diff(ideal)));
            }
        }
    }
    matches.sort_by_key(|(_, drift)| *drift);
    if matches.len() >= 2 {
        let d0 = matches[0].1;
        let d1 = matches[1].1;
        if d1.saturating_sub(d0) <= 3 {
            return None;
        }
    }
    let (pos, drift) = matches.into_iter().next()?;
    let reindent = if ws_insensitive {
        measure_reindent(source_lines, pos, old_lines)
    } else {
        None
    };
    Some(LocatedHunk {
        pos,
        drift,
        reindent,
    })
}

fn diagnose_hunk_failure(
    source_lines: &[&str],
    hunk_no: usize,
    ideal: usize,
    old_lines: &[String],
) -> String {
    if old_lines.is_empty() {
        return format!(
            "hunk #{hunk_no}: insertion anchor near line {} is out of range",
            ideal + 1
        );
    }
    let first = old_lines
        .iter()
        .find(|l| !l.trim().is_empty())
        .map(|s| s.trim())
        .unwrap_or("");
    let nearest = source_lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim() == first && !first.is_empty())
        .min_by_key(|(idx, _)| idx.abs_diff(ideal))
        .map(|(idx, _)| idx + 1);
    let first_short = if first.len() > 60 {
        format!("{}…", crate::util::truncate_str_bytes(first, 60))
    } else {
        first.to_string()
    };
    match nearest {
        Some(line) => format!(
            "hunk #{hunk_no}: could not match {} context line(s) at expected line {}; \
             closest whitespace-insensitive match for `{first_short}` is at line {line} \
             (the surrounding lines differ — re-read that region and rebuild the hunk)",
            old_lines.len(),
            ideal + 1
        ),
        None => format!(
            "hunk #{hunk_no}: no line matching `{first_short}` found anywhere in the file \
             near expected line {}; the file likely changed — re-read it before editing",
            ideal + 1
        ),
    }
}

fn leading_ws(line: &str) -> &str {
    let end = line
        .find(|c: char| c != ' ' && c != '\t')
        .unwrap_or_else(|| line.trim_end_matches(['\n', '\r']).len());
    &line[..end]
}

fn measure_reindent(
    source_lines: &[&str],
    pos: usize,
    old_lines: &[String],
) -> Option<(String, String)> {
    for (i, old) in old_lines.iter().enumerate() {
        if old.trim().is_empty() {
            continue;
        }
        let src = source_lines.get(pos + i)?;
        let from = leading_ws(old).to_string();
        let to = leading_ws(src).to_string();
        if from == to {
            return None;
        }
        return Some((from, to));
    }
    None
}

fn reindent_line(line: &str, from: &str, to: &str) -> String {
    let indent_end = line
        .find(|c: char| c != ' ' && c != '\t')
        .unwrap_or(line.len());
    let (indent, body) = line.split_at(indent_end);
    match indent.strip_prefix(from) {
        Some(extra) => format!("{to}{extra}{body}"),
        None => line.to_string(),
    }
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

fn matches_at_ws_insensitive(source_lines: &[&str], at: usize, old_lines: &[String]) -> bool {
    if at + old_lines.len() > source_lines.len() {
        return false;
    }
    let mut any_non_blank = false;
    for (i, expected) in old_lines.iter().enumerate() {
        let actual = source_lines[at + i].trim();
        let exp = expected.trim();
        if actual != exp {
            return false;
        }
        if !exp.is_empty() {
            any_non_blank = true;
        }
    }
    any_non_blank
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
                    true,
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
        match locate_hunk(source_lines, ideal, cursor, old_lines, max_fuzz, false) {
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
