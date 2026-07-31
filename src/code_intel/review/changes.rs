// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::Serialize;

use crate::code_intel::symbol_graph::{EdgeKind, SymbolGraph, SymbolId};

const SECURITY_KEYWORDS: &[&str] = &[
    "auth",
    "login",
    "logout",
    "password",
    "passwd",
    "token",
    "secret",
    "crypt",
    "cipher",
    "cookie",
    "session",
    "permission",
    "privilege",
    "admin",
    "credential",
    "signature",
    "verify",
    "sanitize",
    "escape",
    "csrf",
    "xss",
    "inject",
    "exec",
    "eval",
];

pub const MAX_CHANGED_FUNCS: usize = 500;

#[must_use]
pub fn safe_git_ref(s: &str) -> bool {
    !s.is_empty()
        && s.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '_' | '.' | '~' | '^' | '/' | '@' | '{' | '}' | '-')
        })
}

fn run_git(repo_root: &Path, args: &[&str]) -> Option<String> {
    let output = crate::util::hidden_sync_command("git")
        .args(args)
        .current_dir(repo_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(crate::util::decode_subprocess_bytes(&output.stdout))
}

#[must_use]
pub fn changed_files(repo_root: &Path, base: &str) -> Vec<PathBuf> {
    if !safe_git_ref(base) {
        return Vec::new();
    }
    if let Some(out) = run_git(repo_root, &["diff", "--name-only", base, "--"]) {
        let files: Vec<PathBuf> = out
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(PathBuf::from)
            .collect();
        if !files.is_empty() {
            return files;
        }
    }
    staged_and_unstaged(repo_root)
}

#[must_use]
pub fn staged_and_unstaged(repo_root: &Path) -> Vec<PathBuf> {
    let Some(out) = run_git(repo_root, &["status", "--porcelain"]) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for line in out.lines() {
        if line.len() <= 3 {
            continue;
        }
        let mut entry = line[3..].trim().to_string();
        if let Some(idx) = entry.find(" -> ") {
            entry = entry[idx + 4..].to_string();
        }
        if !entry.is_empty() {
            files.push(PathBuf::from(entry));
        }
    }
    files
}

#[must_use]
pub fn diff_ranges(repo_root: &Path, base: &str) -> HashMap<PathBuf, Vec<(u32, u32)>> {
    if !safe_git_ref(base) {
        return HashMap::new();
    }
    let out = run_git(repo_root, &["diff", "--unified=0", base, "--"])
        .or_else(|| run_git(repo_root, &["diff", "--unified=0", "HEAD", "--"]))
        .unwrap_or_default();
    parse_unified_diff(&out)
}

fn parse_unified_diff(diff_text: &str) -> HashMap<PathBuf, Vec<(u32, u32)>> {
    let mut ranges: HashMap<PathBuf, Vec<(u32, u32)>> = HashMap::new();
    let mut current: Option<PathBuf> = None;
    for line in diff_text.lines() {
        if let Some(rest) = line.strip_prefix("+++ b/") {
            current = Some(PathBuf::from(rest.trim()));
            continue;
        }
        if line.starts_with("@@") {
            if let Some(file) = &current {
                if let Some((start, end)) = parse_hunk_new_range(line) {
                    ranges.entry(file.clone()).or_default().push((start, end));
                }
            }
        }
    }
    ranges
}

fn parse_hunk_new_range(line: &str) -> Option<(u32, u32)> {
    let plus = line.split_whitespace().find(|t| t.starts_with('+'))?;
    let body = &plus[1..];
    let (start_s, count_s) = match body.split_once(',') {
        Some((s, c)) => (s, Some(c)),
        None => (body, None),
    };
    let start: u32 = start_s.parse().ok()?;
    let count: u32 = match count_s {
        Some(c) => c.parse().ok()?,
        None => 1,
    };
    if count == 0 {
        Some((start, start))
    } else {
        Some((start, start + count - 1))
    }
}

#[must_use]
pub fn map_changes_to_symbols(
    graph: &SymbolGraph,
    changed_ranges: &HashMap<PathBuf, Vec<(u32, u32)>>,
) -> Vec<SymbolId> {
    let mut out: Vec<SymbolId> = Vec::new();
    for (file, ranges) in changed_ranges {
        for sym in graph.symbols_in_file(file) {
            if sym.id.is_file_anchor() {
                continue;
            }
            let s = sym.id.line;
            let e = sym.line_end.max(sym.id.line);
            if ranges.iter().any(|(rs, re)| s <= *re && e >= *rs) {
                out.push(sym.id.clone());
            }
        }
    }
    out
}

#[must_use]
pub fn symbols_in_files(graph: &SymbolGraph, files: &[PathBuf]) -> Vec<SymbolId> {
    let mut out: Vec<SymbolId> = Vec::new();
    for f in files {
        for sym in graph.symbols_in_file(f) {
            if !sym.id.is_file_anchor() {
                out.push(sym.id.clone());
            }
        }
    }
    out
}

#[must_use]
pub fn compute_risk_score(graph: &SymbolGraph, id: &SymbolId) -> f64 {
    let mut score = 0.0_f64;

    let test_count = graph.tests_covering(id).len();
    let coverage = (test_count as f64 / 5.0).min(1.0);
    score += 0.30 - coverage * 0.25;

    let callers: Vec<&SymbolId> = graph
        .in_edges(id)
        .into_iter()
        .filter(|e| matches!(e.kind, EdgeKind::Calls))
        .map(|e| &e.from)
        .collect();
    let caller_count = callers.len();
    let cross_file = callers.iter().filter(|c| c.file != id.file).count();
    score += (cross_file as f64 * 0.05).min(0.15);
    score += (caller_count as f64 / 20.0).min(0.10);

    let name_lower = id.name.to_ascii_lowercase();
    if SECURITY_KEYWORDS.iter().any(|kw| name_lower.contains(kw)) {
        score += 0.20;
    }

    (score.clamp(0.0, 1.0) * 10000.0).round() / 10000.0
}

#[derive(Debug, Clone, Serialize)]
pub struct ScoredSymbol {
    pub name: String,
    pub file: String,
    pub line_start: u32,
    pub line_end: u32,
    pub kind: String,
    pub risk_score: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestGap {
    pub name: String,
    pub file: String,
    pub line_start: u32,
    pub line_end: u32,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ChangeAnalysis {
    pub summary: String,
    pub risk_score: f64,
    pub changed_functions: Vec<ScoredSymbol>,
    pub test_gaps: Vec<TestGap>,
    pub review_priorities: Vec<ScoredSymbol>,
    pub functions_truncated: bool,
}

fn is_reviewable_kind(kind: &str) -> bool {
    matches!(
        kind,
        "function" | "class" | "struct" | "enum" | "trait" | "method"
    )
}

#[must_use]
pub fn analyze_changes(
    graph: &SymbolGraph,
    changed_files_rel: &[PathBuf],
    changed_ranges: Option<&HashMap<PathBuf, Vec<(u32, u32)>>>,
) -> ChangeAnalysis {
    let changed_ids = match changed_ranges {
        Some(ranges) if !ranges.is_empty() => map_changes_to_symbols(graph, ranges),
        _ => symbols_in_files(graph, changed_files_rel),
    };

    let mut changed_funcs: Vec<(SymbolId, String, bool, u32, u32)> = Vec::new();
    for id in &changed_ids {
        if let Some(entry) = graph.symbol_entry(id) {
            if is_reviewable_kind(&entry.kind) {
                changed_funcs.push((
                    id.clone(),
                    entry.kind.clone(),
                    entry.is_test,
                    entry.id.line,
                    entry.line_end.max(entry.id.line),
                ));
            }
        }
    }

    let functions_truncated = changed_funcs.len() > MAX_CHANGED_FUNCS;
    if functions_truncated {
        changed_funcs.truncate(MAX_CHANGED_FUNCS);
    }

    let mut scored: Vec<ScoredSymbol> = Vec::new();
    for (id, kind, _is_test, ls, le) in &changed_funcs {
        let risk = compute_risk_score(graph, id);
        scored.push(ScoredSymbol {
            name: id.name.clone(),
            file: id.file.to_string_lossy().replace('\\', "/"),
            line_start: *ls,
            line_end: *le,
            kind: kind.clone(),
            risk_score: risk,
        });
    }

    let overall_risk = scored
        .iter()
        .map(|s| s.risk_score)
        .fold(0.0_f64, f64::max);

    let mut test_gaps: Vec<TestGap> = Vec::new();
    for (id, _kind, is_test, ls, le) in &changed_funcs {
        if *is_test {
            continue;
        }
        if graph.tests_covering(id).is_empty() {
            test_gaps.push(TestGap {
                name: id.name.clone(),
                file: id.file.to_string_lossy().replace('\\', "/"),
                line_start: *ls,
                line_end: *le,
            });
        }
    }

    let mut review_priorities = scored.clone();
    review_priorities.sort_by(|a, b| {
        b.risk_score
            .partial_cmp(&a.risk_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    review_priorities.truncate(10);

    let mut summary_parts = vec![
        format!("Analyzed {} changed file(s):", changed_files_rel.len()),
        format!("  - {} changed function(s)/type(s)", changed_funcs.len()),
        format!("  - {} test gap(s)", test_gaps.len()),
        format!("  - Overall risk score: {overall_risk:.2}"),
    ];
    if !test_gaps.is_empty() {
        let names: Vec<String> = test_gaps.iter().take(5).map(|g| g.name.clone()).collect();
        summary_parts.push(format!("  - Untested: {}", names.join(", ")));
    }
    if functions_truncated {
        summary_parts.push(format!(
            "  - Warning: analysis capped at {MAX_CHANGED_FUNCS} functions"
        ));
    }

    ChangeAnalysis {
        summary: summary_parts.join("\n"),
        risk_score: overall_risk,
        changed_functions: scored,
        test_gaps,
        review_priorities,
        functions_truncated,
    }
}
