// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::budget::ContextBudget;
use super::types::{ContextItem, ContextResolveError};

pub const RRF_K: f64 = 60.0;

pub const MAX_RESULTS: usize = 8;

const SKIP_DIRS: &[&str] = &[
    ".git",
    ".sen",
    "target",
    "node_modules",
    "dist",
    "build",
    ".venv",
    "__pycache__",
];

const SOURCE_EXTENSIONS: &[&str] = &[
    "rs", "py", "ts", "tsx", "js", "jsx", "mjs", "cjs", "go", "toml", "md", "yaml", "yml", "json",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodebaseHit {
    pub path: PathBuf,
    pub reason: String,

    pub score_x1000: u32,
}

pub(crate) fn collect_codebase_body(
    root: &Path,
    query: &str,
) -> Result<String, ContextResolveError> {
    if query.trim().is_empty() {
        return Err(ContextResolveError::NotFound {
            tag: "codebase:".into(),
            reason: "empty query".into(),
        });
    }

    let query_lc = query.to_ascii_lowercase();
    let path_hits = rank_by_path(root, &query_lc);
    let symbol_hits = rank_by_symbols(root, &query_lc);
    let fused = rrf_fuse(&path_hits, &symbol_hits);

    let top: Vec<&CodebaseHit> = fused.iter().take(MAX_RESULTS).collect();
    Ok(render_body(root, &top))
}

pub(crate) fn finish_codebase_item(
    query: &str,
    body: String,
    budget: &ContextBudget,
) -> ContextItem {
    let final_body = super::handlers::budget_clip_body(body, budget);

    ContextItem::new(
        format!("codebase:{query}"),
        format!("Codebase search: {query}"),
        final_body,
    )
    .with_source("rrf")
}

pub fn resolve_codebase(
    root: &Path,
    query: &str,
    budget: &ContextBudget,
) -> Result<ContextItem, ContextResolveError> {
    let body = collect_codebase_body(root, query)?;
    Ok(finish_codebase_item(query, body, budget))
}

fn rank_by_path(root: &Path, query_lc: &str) -> Vec<(PathBuf, usize)> {
    let mut candidates = Vec::new();
    walk(root, root, &mut candidates);
    let mut scored: Vec<(PathBuf, usize)> = candidates
        .into_iter()
        .filter_map(|rel| {
            let s = rel.to_string_lossy().to_ascii_lowercase();
            s.find(query_lc).map(|off| (rel, off))
        })
        .collect();

    scored.sort_by(|a, b| {
        a.1.cmp(&b.1)
            .then_with(|| a.0.as_os_str().len().cmp(&b.0.as_os_str().len()))
            .then_with(|| a.0.cmp(&b.0))
    });
    scored
}

fn rank_by_symbols(root: &Path, query_lc: &str) -> Vec<(PathBuf, usize)> {
    let writer = match crate::code_intel::symbol_graph::incremental::get_writer_nonblocking(root) {
        crate::code_intel::symbol_graph::incremental::WriterAvailability::Ready(w) => w,
        _ => return Vec::new(),
    };
    let graph_lock = writer.graph();
    let graph = graph_lock.read();
    let mut symbol_hits: Vec<(PathBuf, usize)> = Vec::new();
    for s in &graph.symbols {
        let name_lc = s.id.name.to_ascii_lowercase();
        if let Some(off) = name_lc.find(query_lc) {
            symbol_hits.push((s.id.file.clone(), off));
        }
    }

    symbol_hits.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

    let mut seen = std::collections::HashSet::new();
    symbol_hits.retain(|(p, _)| seen.insert(p.clone()));
    symbol_hits
}

fn rrf_fuse(path_hits: &[(PathBuf, usize)], symbol_hits: &[(PathBuf, usize)]) -> Vec<CodebaseHit> {
    let mut scores: HashMap<PathBuf, (f64, bool, bool)> = HashMap::new();
    for (rank, (p, _)) in path_hits.iter().enumerate() {
        let entry = scores.entry(p.clone()).or_insert((0.0, false, false));
        entry.0 += 1.0 / (RRF_K + (rank + 1) as f64);
        entry.1 = true;
    }
    for (rank, (p, _)) in symbol_hits.iter().enumerate() {
        let entry = scores.entry(p.clone()).or_insert((0.0, false, false));
        entry.0 += 1.0 / (RRF_K + (rank + 1) as f64);
        entry.2 = true;
    }
    let mut fused: Vec<CodebaseHit> = scores
        .into_iter()
        .map(|(p, (score, by_path, by_sym))| {
            let reason = match (by_path, by_sym) {
                (true, true) => "symbol + path",
                (true, false) => "path",
                (false, true) => "symbol",
                _ => "",
            }
            .to_string();
            CodebaseHit {
                path: p,
                reason,
                score_x1000: (score * 1000.0) as u32,
            }
        })
        .collect();
    fused.sort_by(|a, b| {
        b.score_x1000
            .cmp(&a.score_x1000)
            .then_with(|| a.path.cmp(&b.path))
    });
    fused
}

fn render_body(root: &Path, hits: &[&CodebaseHit]) -> String {
    if hits.is_empty() {
        return "(no codebase matches  -  try a broader query; if the symbol index was missing \
it is being built in the background, so retry shortly)"
            .to_string();
    }
    let mut out = String::new();
    for hit in hits {
        out.push_str(&format!(
            "- {} [score={:.3}, via {}]\n",
            hit.path.display(),
            hit.score_x1000 as f64 / 1000.0,
            hit.reason
        ));
        if let Some(snip) = head_snippet(&root.join(&hit.path), 6) {
            for line in snip.lines() {
                out.push_str("    ");
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

fn head_snippet(abs: &Path, max_lines: usize) -> Option<String> {
    let bytes = fs::read(abs).ok()?;
    if bytes.len() > 32 * 1024 {
        return None;
    }
    let text = String::from_utf8_lossy(&bytes);
    let head = text.lines().take(max_lines).collect::<Vec<_>>().join("\n");
    if head.trim().is_empty() {
        None
    } else {
        Some(head)
    }
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_dir() {
            let name = entry.file_name();
            if SKIP_DIRS.contains(&name.to_string_lossy().as_ref()) {
                continue;
            }
            walk(root, &path, out);
        } else if ft.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if SOURCE_EXTENSIONS.contains(&ext) {
                    if let Ok(rel) = path.strip_prefix(root) {
                        out.push(rel.to_path_buf());
                    }
                }
            }
        }
    }
}
