// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::planner::QueryPlan;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;

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
    let rg_available = which::which("rg").is_ok();
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

    if rg_available {
        let mut total_files: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        for token in &tokens {
            let pattern = build_pattern_for_token(token);
            let raw_hits =
                rg_search(workspace_root, scope_path, &pattern, include_globs, exclude_globs)
                    .await?;
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
    } else {
        let token_refs: Vec<&str> = tokens.iter().map(|s| s.as_str()).collect();
        report.hits = pure_rust_recall(workspace_root, scope_path, &token_refs).await?;
        let files: std::collections::HashSet<PathBuf> =
            report.hits.iter().map(|h| h.path.clone()).collect();
        report.total_files = files.len();
        for hit in &report.hits {
            *report
                .token_doc_counts
                .entry(hit.token.clone())
                .or_insert(0) += 1;
        }
    }

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

async fn rg_search(
    workspace_root: &Path,
    scope_path: &Path,
    pattern: &str,
    include_globs: &[String],
    exclude_globs: &[String],
) -> anyhow::Result<Vec<RawHit>> {
    let mut cmd = crate::util::hidden_async_command("rg");
    cmd.current_dir(workspace_root)
        .arg("--no-heading")
        .arg("--line-number")
        .arg("--smart-case")
        .arg("--color=never")
        .arg("--max-count")
        .arg("12")
        .arg("--max-filesize")
        .arg("2M")
        .arg("--hidden")
        .arg("--glob")
        .arg("!.git/")
        .arg("--glob")
        .arg("!node_modules/")
        .arg("--glob")
        .arg("!target/")
        .arg("--glob")
        .arg("!dist/")
        .arg("--glob")
        .arg("!build/");
    for inc in include_globs {
        cmd.arg("--glob").arg(inc);
    }
    for exc in exclude_globs {
        let pat = if exc.starts_with('!') {
            exc.clone()
        } else {
            format!("!{exc}")
        };
        cmd.arg("--glob").arg(pat);
    }
    cmd.arg("-e").arg(pattern).arg(scope_path);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn()?;
    let mut stdout = child.stdout.take().expect("stdout piped");
    let mut buf = Vec::new();
    let _ = stdout.read_to_end(&mut buf).await;
    let _ = child.wait().await?;
    let text = String::from_utf8_lossy(&buf);
    parse_rg_output(&text, workspace_root)
}

fn parse_rg_output(text: &str, workspace_root: &Path) -> anyhow::Result<Vec<RawHit>> {
    let mut hits = Vec::new();
    for line in text.lines() {
        let mut parts = line.splitn(3, ':');
        let path_str = parts.next().unwrap_or("");
        let lineno_str = parts.next().unwrap_or("");
        let text = parts.next().unwrap_or("");
        if path_str.is_empty() || lineno_str.is_empty() {
            continue;
        }
        let line_number = match lineno_str.parse::<usize>() {
            Ok(n) => n,
            Err(_) => continue,
        };
        let raw_path = PathBuf::from(path_str);
        let abs = if raw_path.is_absolute() {
            raw_path
        } else {
            workspace_root.join(&raw_path)
        };
        hits.push(RawHit {
            path: abs,
            line_number,
            token: String::new(),
            line_text: text.trim().to_string(),
        });
    }
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
    let mut matches: Vec<(PathBuf, usize)> = Vec::new();
    walk_into(scope_path, workspace_root, &needles, &mut matches, include_globs, 0)?;
    matches.sort_by(|a, b| b.1.cmp(&a.1));
    Ok(matches.into_iter().map(|(p, _)| p).take(30).collect())
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
                    .and_then(|gp| Ok(gp.compile_matcher()))
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

async fn pure_rust_recall(
    workspace_root: &Path,
    scope_path: &Path,
    tokens: &[&str],
) -> anyhow::Result<Vec<RawHit>> {
    let mut hits = Vec::new();
    let mut stack = vec![scope_path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
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
                stack.push(path);
                continue;
            }
            if metadata.len() > 2 * 1024 * 1024 {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for (idx, line) in text.lines().enumerate() {
                let lc = line.to_lowercase();
                for token in tokens {
                    if lc.contains(&token.to_lowercase()) {
                        hits.push(RawHit {
                            path: path.clone(),
                            line_number: idx + 1,
                            token: (*token).to_string(),
                            line_text: line.trim().to_string(),
                        });
                        if hits.len() > 3000 {
                            return Ok(hits);
                        }
                        break;
                    }
                }
            }
        }
        let _ = workspace_root;
    }
    Ok(hits)
}
