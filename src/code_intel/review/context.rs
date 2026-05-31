// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::code_intel::symbol_graph::{
    EdgeKind, ImpactResult, SymbolGraph, SymbolId, impact_radius, max_impact_depth,
    max_impact_nodes, seeds_for_files,
};

use super::changes::{analyze_changes, changed_files, diff_ranges, map_changes_to_symbols};

fn rel_str(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn data_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "sen")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| std::env::temp_dir().join("sen"))
}

fn baseline_file_bytes(repo_root: &Path, files: &[PathBuf]) -> usize {
    let mut total = 0usize;
    for f in files {
        let full = if f.is_absolute() {
            f.clone()
        } else {
            repo_root.join(f)
        };
        if let Ok(meta) = std::fs::metadata(&full) {
            total = total.saturating_add(meta.len() as usize);
        }
    }
    total
}

fn fmt_int(n: u64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    let len = bytes.len();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

fn boxed_panel(title: &str, lines: &[String]) -> String {
    let inner_content = lines
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0)
        .max(title.chars().count() + 2);
    let inner_w = inner_content + 2;
    let title_str = format!(" {title} ");
    let dash_total = inner_w.saturating_sub(title_str.chars().count()).max(2);
    let left = dash_total / 2;
    let right = dash_total - left;
    let top = format!("┌{}{}{}┐", "─".repeat(left), title_str, "─".repeat(right));
    let bottom = format!("└{}┘", "─".repeat(inner_w));
    let mut out = vec![top];
    for l in lines {
        let pad = inner_w.saturating_sub(1 + l.chars().count());
        out.push(format!("│ {}{}│", l, " ".repeat(pad)));
    }
    out.push(bottom);
    out.join("\n")
}

struct Savings {
    baseline_bytes: usize,
    returned_bytes: usize,
}

fn record_and_panel(action: &str, files: &[PathBuf], s: &Savings) -> Option<String> {
    if s.baseline_bytes == 0 {
        return None;
    }
    let _ = crate::token_saver::tracking::record(
        &format!("code_review:{action}"),
        "code_review",
        s.baseline_bytes,
        s.returned_bytes,
        0,
        &data_dir(),
    );
    let _ = files;
    let baseline_tokens = ((s.baseline_bytes as f64) / 3.5).ceil() as u64;
    let returned_tokens = ((s.returned_bytes as f64) / 3.5).ceil() as u64;
    let saved = baseline_tokens.saturating_sub(returned_tokens);
    let percent = if baseline_tokens > 0 {
        ((saved as f64 / baseline_tokens as f64) * 100.0).round() as i64
    } else {
        0
    };
    let lines = vec![
        format!("Full file read would be: {:>10} tokens", fmt_int(baseline_tokens)),
        format!("Review context used:     {:>10} tokens", fmt_int(returned_tokens)),
        format!("Saved (estimated):       {:>10} tokens (~{percent}%)", fmt_int(saved)),
    ];
    Some(boxed_panel("Token Savings", &lines))
}

fn symbol_line(graph: &SymbolGraph, id: &SymbolId) -> String {
    let entry = graph.symbol_entry(id);
    let kind = entry.map(|e| e.kind.clone()).unwrap_or_default();
    let end = entry.map(|e| e.line_end.max(id.line)).unwrap_or(id.line);
    let range = if end > id.line {
        format!("{}-{}", id.line, end)
    } else {
        id.line.to_string()
    };
    let test_tag = if entry.map(|e| e.is_test).unwrap_or(false) {
        " [test]"
    } else {
        ""
    };
    format!("{} ({}:{}) [{}]{}", id.name, rel_str(&id.file), range, kind, test_tag)
}

fn risk_band(score: f64) -> &'static str {
    if score > 0.7 {
        "high"
    } else if score > 0.4 {
        "medium"
    } else {
        "low"
    }
}

fn edge_kind_counts(impact: &ImpactResult) -> String {
    let (mut calls, mut implements, mut uses, mut imports, mut tested) = (0, 0, 0, 0, 0);
    for e in &impact.edges {
        match e.kind {
            EdgeKind::Calls => calls += 1,
            EdgeKind::Implements => implements += 1,
            EdgeKind::Uses => uses += 1,
            EdgeKind::Imports => imports += 1,
            EdgeKind::TestedBy => tested += 1,
        }
    }
    format!(
        "calls {calls} · implements {implements} · uses {uses} · imports {imports} · tested_by {tested}"
    )
}

fn list_block(header: &str, items: &[String], limit: usize) -> String {
    if items.is_empty() {
        return format!("{header}: none");
    }
    let mut out = format!("{header} ({}):", items.len());
    for it in items.iter().take(limit) {
        out.push_str(&format!("\n  - {it}"));
    }
    if items.len() > limit {
        out.push_str(&format!("\n  - ... ({} more)", items.len() - limit));
    }
    out
}

fn no_changes(base: &str) -> String {
    format!("No changes detected against `{base}`. Nothing to review.")
}

fn resolve_changed(
    graph: &SymbolGraph,
    repo_root: &Path,
    explicit: Option<Vec<PathBuf>>,
    base: &str,
) -> (Vec<PathBuf>, HashMap<PathBuf, Vec<(u32, u32)>>, Vec<SymbolId>) {
    let files = explicit.unwrap_or_else(|| changed_files(repo_root, base));
    let ranges = diff_ranges(repo_root, base);
    let changed_ids = if ranges.is_empty() {
        super::changes::symbols_in_files(graph, &files)
    } else {
        map_changes_to_symbols(graph, &ranges)
    };
    (files, ranges, changed_ids)
}

#[must_use]
pub fn impact_radius_report(
    graph: &SymbolGraph,
    repo_root: &Path,
    explicit_files: Option<Vec<PathBuf>>,
    base: &str,
) -> String {
    let files = explicit_files.unwrap_or_else(|| changed_files(repo_root, base));
    if files.is_empty() {
        return no_changes(base);
    }
    let seeds = seeds_for_files(graph, &files);
    let impact = impact_radius(graph, &seeds, max_impact_depth(), max_impact_nodes());

    let impacted_syms: Vec<String> = impact
        .impacted
        .iter()
        .filter(|id| !id.is_file_anchor())
        .map(|id| symbol_line(graph, id))
        .collect();
    let impacted_files: Vec<String> = impact.impacted_files.iter().map(|p| rel_str(p)).collect();
    let changed_list: Vec<String> = files.iter().map(|p| rel_str(p)).collect();

    let mut body = String::new();
    body.push_str(&format!("# Impact radius (base `{base}`)\n\n"));
    body.push_str(&format!(
        "Blast radius: {} impacted node(s) across {} file(s){}.\n\n",
        impacted_syms.len(),
        impacted_files.len(),
        if impact.truncated { " (truncated)" } else { "" }
    ));
    body.push_str(&list_block("Changed files", &changed_list, 50));
    body.push_str("\n\n");
    body.push_str(&list_block("Impacted files", &impacted_files, 50));
    body.push_str("\n\n");
    body.push_str(&list_block("Impacted symbols", &impacted_syms, 40));
    body.push_str("\n\n");
    body.push_str(&format!("Edges in radius: {}\n", edge_kind_counts(&impact)));

    let savings = Savings {
        baseline_bytes: baseline_file_bytes(repo_root, &files),
        returned_bytes: body.len(),
    };
    if let Some(panel) = record_and_panel("impact_radius", &files, &savings) {
        body.push('\n');
        body.push_str(&panel);
    }
    body
}

#[must_use]
pub fn detect_changes_report(
    graph: &SymbolGraph,
    repo_root: &Path,
    explicit_files: Option<Vec<PathBuf>>,
    base: &str,
) -> String {
    let (files, ranges, _ids) = resolve_changed(graph, repo_root, explicit_files, base);
    if files.is_empty() {
        return no_changes(base);
    }
    let analysis = analyze_changes(
        graph,
        &files,
        if ranges.is_empty() { None } else { Some(&ranges) },
    );

    let mut body = String::new();
    body.push_str(&format!("# Code review (base `{base}`)\n\n"));
    body.push_str(&analysis.summary);
    body.push_str("\n\n");
    body.push_str(&format!(
        "Overall risk: {:.2} ({})\n\n",
        analysis.risk_score,
        risk_band(analysis.risk_score)
    ));

    if analysis.review_priorities.is_empty() {
        body.push_str("Review priorities: none (no reviewable symbols changed)\n");
    } else {
        body.push_str("Review priorities (highest risk first):\n");
        for (i, p) in analysis.review_priorities.iter().enumerate() {
            body.push_str(&format!(
                "  {}. [{:.2}] {} ({}:{}-{}) [{}]\n",
                i + 1,
                p.risk_score,
                p.name,
                p.file,
                p.line_start,
                p.line_end,
                p.kind
            ));
        }
    }
    body.push('\n');

    if analysis.test_gaps.is_empty() {
        body.push_str("Test gaps: none detected.\n");
    } else {
        body.push_str(&format!("Test gaps ({}):\n", analysis.test_gaps.len()));
        for g in analysis.test_gaps.iter().take(20) {
            body.push_str(&format!("  - {} ({}:{})\n", g.name, g.file, g.line_start));
        }
        if analysis.test_gaps.len() > 20 {
            body.push_str(&format!("  - ... ({} more)\n", analysis.test_gaps.len() - 20));
        }
    }

    let savings = Savings {
        baseline_bytes: baseline_file_bytes(repo_root, &files),
        returned_bytes: body.len(),
    };
    if let Some(panel) = record_and_panel("detect_changes", &files, &savings) {
        body.push('\n');
        body.push_str(&panel);
    }
    body
}

fn extract_relevant_lines(lines: &[&str], symbol_ranges: &[(u32, u32)], max_fallback: usize) -> String {
    if symbol_ranges.is_empty() {
        return lines
            .iter()
            .take(max_fallback)
            .enumerate()
            .map(|(i, l)| format!("{}: {}", i + 1, l))
            .collect::<Vec<_>>()
            .join("\n");
    }
    let mut ranges: Vec<(usize, usize)> = symbol_ranges
        .iter()
        .map(|(s, e)| {
            let start = (s.saturating_sub(3)) as usize;
            let end = ((*e as usize) + 2).min(lines.len());
            (start.min(lines.len()), end)
        })
        .collect();
    ranges.sort();
    let mut merged: Vec<(usize, usize)> = vec![ranges[0]];
    for (s, e) in ranges.into_iter().skip(1) {
        let last = merged.last_mut().unwrap();
        if s <= last.1 + 1 {
            last.1 = last.1.max(e);
        } else {
            merged.push((s, e));
        }
    }
    let mut parts: Vec<String> = Vec::new();
    for (s, e) in merged {
        if !parts.is_empty() {
            parts.push("    ...".to_string());
        }
        for i in s..e {
            if let Some(l) = lines.get(i) {
                parts.push(format!("{}: {}", i + 1, l));
            }
        }
    }
    parts.join("\n")
}

fn guidance(graph: &SymbolGraph, impact: &ImpactResult, changed_ids: &[SymbolId]) -> Vec<String> {
    let mut out = Vec::new();
    let untested: Vec<&SymbolId> = changed_ids
        .iter()
        .filter(|id| {
            graph.symbol_entry(id).map(|e| !e.is_test).unwrap_or(true)
                && graph.tests_covering(id).is_empty()
        })
        .collect();
    if !untested.is_empty() {
        let names: Vec<String> = untested.iter().take(5).map(|i| i.name.clone()).collect();
        out.push(format!(
            "{} changed symbol(s) lack test coverage: {}",
            untested.len(),
            names.join(", ")
        ));
    }
    if impact.impacted.len() > 20 {
        out.push(format!(
            "Wide blast radius: {} nodes impacted — review callers and dependents carefully.",
            impact.impacted.len()
        ));
    }
    let inheritance = impact
        .edges
        .iter()
        .filter(|e| matches!(e.kind, EdgeKind::Implements))
        .count();
    if inheritance > 0 {
        out.push(format!(
            "{inheritance} inheritance/implementation relationship(s) affected — check substitutability."
        ));
    }
    if impact.impacted_files.len() > 3 {
        out.push(format!(
            "Changes impact {} other file(s) — consider splitting into smaller PRs.",
            impact.impacted_files.len()
        ));
    }
    if out.is_empty() {
        out.push("Changes appear well-contained with minimal blast radius.".to_string());
    }
    out
}

#[must_use]
pub fn review_context_report(
    graph: &SymbolGraph,
    repo_root: &Path,
    explicit_files: Option<Vec<PathBuf>>,
    base: &str,
    include_source: bool,
    max_lines_per_file: usize,
) -> String {
    let (files, _ranges, changed_ids) = resolve_changed(graph, repo_root, explicit_files, base);
    if files.is_empty() {
        return no_changes(base);
    }

    let seeds = seeds_for_files(graph, &files);
    let impact = impact_radius(graph, &seeds, max_impact_depth(), max_impact_nodes());

    let mut body = String::new();
    body.push_str(&format!("# Review context (base `{base}`)\n\n"));
    body.push_str(&format!(
        "{} changed file(s), {} directly changed symbol(s), {} impacted node(s) in {} file(s){}.\n\n",
        files.len(),
        changed_ids.len(),
        impact.impacted.iter().filter(|i| !i.is_file_anchor()).count(),
        impact.impacted_files.len(),
        if impact.truncated { " (truncated)" } else { "" }
    ));

    body.push_str("Review guidance:\n");
    for g in guidance(graph, &impact, &changed_ids) {
        body.push_str(&format!("  - {g}\n"));
    }
    body.push('\n');

    let changed_list: Vec<String> = changed_ids.iter().map(|id| symbol_line(graph, id)).collect();
    body.push_str(&list_block("Changed symbols", &changed_list, 40));
    body.push_str("\n\n");

    let impacted_files: Vec<String> = impact.impacted_files.iter().map(|p| rel_str(p)).collect();
    body.push_str(&list_block("Impacted files", &impacted_files, 40));
    body.push_str("\n");

    if include_source {
        let mut by_file: HashMap<PathBuf, Vec<(u32, u32)>> = HashMap::new();
        for id in &changed_ids {
            if let Some(entry) = graph.symbol_entry(id) {
                by_file
                    .entry(id.file.clone())
                    .or_default()
                    .push((id.line, entry.line_end.max(id.line)));
            }
        }
        for f in &files {
            let full = if f.is_absolute() {
                f.clone()
            } else {
                repo_root.join(f)
            };
            let Ok(content) = std::fs::read_to_string(&full) else {
                continue;
            };
            let lines: Vec<&str> = content.lines().collect();
            let snippet = if lines.len() > max_lines_per_file {
                let ranges = by_file.get(f).cloned().unwrap_or_default();
                extract_relevant_lines(&lines, &ranges, 50)
            } else {
                lines
                    .iter()
                    .enumerate()
                    .map(|(i, l)| format!("{}: {}", i + 1, l))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            body.push_str(&format!("\n----- {} -----\n", rel_str(f)));
            body.push_str(&snippet);
            body.push('\n');
        }
    }

    let savings = Savings {
        baseline_bytes: baseline_file_bytes(repo_root, &files),
        returned_bytes: body.len(),
    };
    if let Some(panel) = record_and_panel("review_context", &files, &savings) {
        body.push('\n');
        body.push_str(&panel);
    }
    body
}

#[must_use]
pub fn minimal_context_report(
    graph: &SymbolGraph,
    repo_root: &Path,
    task: &str,
    base: &str,
) -> String {
    let files = changed_files(repo_root, base);
    let symbol_count = graph
        .symbols
        .iter()
        .filter(|s| !s.id.is_file_anchor())
        .count();
    let edge_count = graph.edges.len();
    let file_count = {
        let mut set = std::collections::HashSet::new();
        for s in &graph.symbols {
            set.insert(s.id.file.clone());
        }
        set.len()
    };

    let mut risk = "unknown".to_string();
    let mut risk_score = 0.0_f64;
    let mut top_affected: Vec<String> = Vec::new();
    let mut test_gap_count = 0usize;
    if !files.is_empty() {
        let ranges = diff_ranges(repo_root, base);
        let analysis = analyze_changes(
            graph,
            &files,
            if ranges.is_empty() { None } else { Some(&ranges) },
        );
        risk_score = analysis.risk_score;
        risk = risk_band(risk_score).to_string();
        top_affected = analysis
            .review_priorities
            .iter()
            .take(5)
            .map(|s| format!("{} ({}:{})", s.name, s.file, s.line_start))
            .collect();
        test_gap_count = analysis.test_gaps.len();
    }

    let task_lower = task.to_ascii_lowercase();
    let suggestions: Vec<&str> = if ["review", "pr", "merge", "diff"]
        .iter()
        .any(|w| task_lower.contains(w))
    {
        vec!["code_review(detect_changes)", "code_review(review_context)", "code_review(impact_radius)"]
    } else if ["debug", "bug", "error", "fix"]
        .iter()
        .any(|w| task_lower.contains(w))
    {
        vec!["code_graph_query", "content_search", "code_review(impact_radius)"]
    } else if ["refactor", "rename", "clean"]
        .iter()
        .any(|w| task_lower.contains(w))
    {
        vec!["code_review(impact_radius)", "code_graph_query", "code_review(review_context)"]
    } else {
        vec!["code_review(detect_changes)", "code_review(review_context)", "code_graph_query"]
    };

    let mut body = String::new();
    body.push_str("# Minimal review bootstrap\n\n");
    body.push_str(&format!(
        "Graph: {symbol_count} symbols, {edge_count} edges across {file_count} files.\n"
    ));
    if risk != "unknown" {
        body.push_str(&format!(
            "Changes: {} file(s) · risk {risk} ({risk_score:.2}) · {test_gap_count} test gap(s).\n",
            files.len()
        ));
    } else {
        body.push_str("Changes: none detected.\n");
    }
    if !top_affected.is_empty() {
        body.push_str("Top affected:\n");
        for t in &top_affected {
            body.push_str(&format!("  - {t}\n"));
        }
    }
    body.push_str(&format!("\nSuggested next: {}\n", suggestions.join(", ")));
    body
}
