// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::planner::QueryPlan;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct RawHit {
    pub path: PathBuf,
    pub line_number: usize,
    pub token: String,
    pub line_text: String,
}

#[derive(Debug, Clone, Default)]
pub struct RecallReport {
    pub hits: Vec<RawHit>,
    pub structural_paths: Vec<PathBuf>,
    pub token_doc_counts: HashMap<String, usize>,
    pub total_files: usize,
}

pub async fn run_recall(
    workspace_root: &Path,
    scope_path: &Path,
    plan: &QueryPlan,
    include_globs: &[String],
    exclude_globs: &[String],
    _context_lines: usize,
) -> anyhow::Result<RecallReport> {
    let mut report = RecallReport::default();

    let mut tokens: Vec<String> = plan
        .tokens
        .iter()
        .filter(|t| t.len() >= 2)
        .cloned()
        .collect();
    for phrase in &plan.phrases {
        tokens.push(phrase.clone());
    }
    tokens.sort();
    tokens.dedup();
    if tokens.is_empty() {
        return Ok(report);
    }

    let mut total_files: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for token in &tokens {
        let pattern = build_pattern_for_token(token);
        let raw_hits =
            engine_search(scope_path, &pattern, include_globs, exclude_globs).await?;
        *report
            .token_doc_counts
            .entry(token.clone())
            .or_insert(0) += raw_hits
            .iter()
            .map(|h| h.path.clone())
            .collect::<std::collections::HashSet<_>>()
            .len();
        for hit in &raw_hits {
            total_files.insert(hit.path.clone());
        }
        for mut hit in raw_hits {
            hit.token = token.clone();
            report.hits.push(hit);
        }
    }
    report.total_files = total_files.len();

    report.structural_paths = structural_recall(workspace_root, scope_path, plan, include_globs).await?;
    Ok(report)
}

fn build_pattern_for_token(token: &str) -> String {
    if token.contains(' ') {
        return token.to_string();
    }
    let escaped = regex::escape(token);
    let len = token.len();
    if len <= 2 {
        return format!(r"\b{}\b", escaped);
    }
    format!(r"\b{}\b", escaped)
}

async fn engine_search(
    scope_path: &Path,
    pattern: &str,
    include_globs: &[String],
    exclude_globs: &[String],
) -> anyhow::Result<Vec<RawHit>> {
    let mut globs: Vec<String> = vec![
        "!.git/".to_string(),
        "!node_modules/".to_string(),
        "!target/".to_string(),
        "!dist/".to_string(),
        "!build/".to_string(),
    ];
    for inc in include_globs {
        globs.push(inc.clone());
    }
    for exc in exclude_globs {
        let pat = if exc.starts_with('!') {
            exc.clone()
        } else {
            format!("!{exc}")
        };
        globs.push(pat);
    }
    let request = crate::tools::content_search::engine::SearchRequest {
        root: scope_path.to_path_buf(),
        pattern: pattern.to_string(),
        fixed_string: false,
        case_sensitive: false,
        smart_case: true,
        whole_word: false,
        multiline: false,
        include_globs: globs,
        respect_ignore: true,
        include_hidden: true,
        max_file_size: Some(2 * 1024 * 1024),
        max_count_per_file: Some(12),
        context_before: 0,
        context_after: 0,
        encoding: None,
        timeout: Some(std::time::Duration::from_secs(20)),
        max_total_matches: u64::MAX,
        collect_lines: true,
    };
    let hits = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<RawHit>> {
        let outcome = crate::tools::content_search::engine::search(&request)?;
        let mut hits = Vec::new();
        for file in outcome.files {
            for lm in &file.lines {
                hits.push(RawHit {
                    path: file.path.clone(),
                    line_number: usize::try_from(lm.line_number).unwrap_or(usize::MAX),
                    token: String::new(),
                    line_text: lm.text.trim().to_string(),
                });
            }
        }
        Ok(hits)
    })
    .await??;
    Ok(hits)
}

async fn structural_recall(
    workspace_root: &Path,
    scope_path: &Path,
    plan: &QueryPlan,
    include_globs: &[String],
) -> anyhow::Result<Vec<PathBuf>> {
    let needles: Vec<String> = plan
        .tokens
        .iter()
        .filter(|t| t.len() >= 3)
        .cloned()
        .collect();
    if needles.is_empty() {
        return Ok(Vec::new());
    }
    let scope = scope_path.to_path_buf();
    let root = workspace_root.to_path_buf();
    let includes = include_globs.to_vec();
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<PathBuf>> {
        let mut matches: Vec<(PathBuf, usize)> = Vec::new();
        walk_into(&scope, &root, &needles, &mut matches, &includes, 0)?;
        matches.sort_by(|a, b| b.1.cmp(&a.1));
        Ok(matches.into_iter().map(|(p, _)| p).take(30).collect())
    })
    .await??;
    Ok(result)
}

fn walk_into(
    dir: &Path,
    workspace_root: &Path,
    needles: &[String],
    matches: &mut Vec<(PathBuf, usize)>,
    include_globs: &[String],
    depth: usize,
) -> anyhow::Result<()> {
    if depth > 6 {
        return Ok(());
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if matches!(name, ".git" | "node_modules" | "target" | "dist" | "build" | ".venv" | "__pycache__") {
                continue;
            }
        }
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if metadata.is_dir() {
            walk_into(&path, workspace_root, needles, matches, include_globs, depth + 1)?;
            continue;
        }
        let rel = path.strip_prefix(workspace_root).unwrap_or(&path);
        let rel_str = rel.to_string_lossy().to_lowercase();
        if !include_globs.is_empty() {
            let included = include_globs.iter().any(|g| {
                globset::Glob::new(g)
                    .map(|gp| gp.compile_matcher())
                    .map(|m| m.is_match(rel))
                    .unwrap_or(false)
            });
            if !included {
                continue;
            }
        }
        let mut score = 0usize;
        for needle in needles {
            if rel_str.contains(&needle.to_lowercase()) {
                score += 2;
            }
            if path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_lowercase().contains(&needle.to_lowercase()))
                .unwrap_or(false)
            {
                score += 1;
            }
        }
        if score > 0 {
            matches.push((path, score));
        }
    }
    Ok(())
}

