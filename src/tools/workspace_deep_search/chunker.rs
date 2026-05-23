// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::recall::{RawHit, RecallReport};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Chunk {
    pub path: PathBuf,
    pub rel_path: PathBuf,
    pub line_start: usize,
    pub line_end: usize,
    pub body: String,
    pub tokens_matched: Vec<String>,
    pub raw_score: f32,
    pub structural_boost: f32,
}

pub async fn build_chunks(
    workspace_root: &Path,
    report: &RecallReport,
    context_lines: usize,
) -> Vec<Chunk> {
    let mut by_file: HashMap<PathBuf, Vec<RawHit>> = HashMap::new();
    for hit in &report.hits {
        by_file.entry(hit.path.clone()).or_default().push(hit.clone());
    }
    let mut chunks = Vec::new();
    let total_files = report.total_files.max(1) as f32;
    for (path, mut hits) in by_file {
        let Ok(text) = tokio::fs::read_to_string(&path).await else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        hits.sort_by_key(|h| h.line_number);
        let groups = group_neighbors(&hits, context_lines.max(2) * 2);
        let rel = path
            .strip_prefix(workspace_root)
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| path.clone());
        let structural_boost = if report
            .structural_paths
            .iter()
            .any(|p| p == &path)
        {
            0.35
        } else {
            0.0
        };
        for group in groups {
            if group.is_empty() {
                continue;
            }
            let first = group[0].line_number;
            let last = group[group.len() - 1].line_number;
            let start = first.saturating_sub(context_lines).max(1);
            let end = (last + context_lines).min(lines.len());
            let body = lines
                .iter()
                .skip(start.saturating_sub(1))
                .take(end - start + 1)
                .copied()
                .collect::<Vec<_>>()
                .join("\n");
            let tokens_matched: Vec<String> = group
                .iter()
                .map(|h| h.token.clone())
                .filter(|t| !t.is_empty())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();
            let mut raw_score = 0.0f32;
            for token in &tokens_matched {
                let df = *report.token_doc_counts.get(token).unwrap_or(&1) as f32;
                let idf = ((total_files + 1.0) / (df + 0.5) + 1.0).ln();
                let tf = group.iter().filter(|h| &h.token == token).count() as f32;
                raw_score += idf * tf / (tf + 1.5);
            }
            chunks.push(Chunk {
                path: path.clone(),
                rel_path: rel.clone(),
                line_start: start,
                line_end: end,
                body,
                tokens_matched,
                raw_score,
                structural_boost,
            });
        }
    }
    chunks
}

fn group_neighbors(hits: &[RawHit], window: usize) -> Vec<Vec<RawHit>> {
    let mut groups: Vec<Vec<RawHit>> = Vec::new();
    for hit in hits {
        if let Some(last) = groups.last_mut() {
            let last_line = last.last().map(|h| h.line_number).unwrap_or(0);
            if hit.line_number.saturating_sub(last_line) <= window {
                last.push(hit.clone());
                continue;
            }
        }
        groups.push(vec![hit.clone()]);
    }
    groups
}
