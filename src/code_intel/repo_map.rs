// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::path::PathBuf;

use super::symbol_graph::SymbolGraph;

const DAMPING: f32 = 0.85;
const ITERATIONS: usize = 24;
const CONVERGENCE_EPS: f32 = 1e-6;

pub struct RepoMap {
    pub files: Vec<RepoMapFile>,
}

pub struct RepoMapFile {
    pub path: PathBuf,
    pub score: f32,
    pub symbols: Vec<RepoMapSymbol>,
}

pub struct RepoMapSymbol {
    pub name: String,
    pub kind: String,
    pub line: u32,
    pub score: f32,
}

pub fn build_repo_map(
    graph: &SymbolGraph,
    max_files: usize,
    max_symbols_per_file: usize,
) -> RepoMap {
    let n = graph.symbols.len();
    if n == 0 || max_files == 0 {
        return RepoMap { files: Vec::new() };
    }

    let mut index_of: HashMap<&super::symbol_graph::SymbolId, usize> =
        HashMap::with_capacity(n);
    for (i, sym) in graph.symbols.iter().enumerate() {
        index_of.insert(&sym.id, i);
    }

    let mut out_degree: Vec<u32> = vec![0; n];
    let mut incoming: Vec<Vec<u32>> = vec![Vec::new(); n];
    for edge in &graph.edges {
        let (Some(&from), Some(&to)) = (index_of.get(&edge.from), index_of.get(&edge.to))
        else {
            continue;
        };
        if from == to {
            continue;
        }
        out_degree[from] += 1;
        incoming[to].push(from as u32);
    }

    let uniform = 1.0f32 / n as f32;
    let mut rank: Vec<f32> = vec![uniform; n];
    let mut next: Vec<f32> = vec![0.0; n];
    for _ in 0..ITERATIONS {
        let mut dangling_mass = 0.0f32;
        for i in 0..n {
            if out_degree[i] == 0 {
                dangling_mass += rank[i];
            }
        }
        let base = (1.0 - DAMPING) * uniform + DAMPING * dangling_mass * uniform;
        for slot in next.iter_mut() {
            *slot = base;
        }
        for to in 0..n {
            let mut acc = 0.0f32;
            for &from in &incoming[to] {
                let from = from as usize;
                acc += rank[from] / out_degree[from] as f32;
            }
            next[to] += DAMPING * acc;
        }
        let delta: f32 = rank
            .iter()
            .zip(next.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        std::mem::swap(&mut rank, &mut next);
        if delta < CONVERGENCE_EPS {
            break;
        }
    }

    let mut per_file: HashMap<&std::path::Path, (f32, Vec<usize>)> = HashMap::new();
    for (i, sym) in graph.symbols.iter().enumerate() {
        if sym.is_test {
            continue;
        }
        let entry = per_file
            .entry(sym.id.file.as_path())
            .or_insert((0.0, Vec::new()));
        entry.0 += rank[i];
        entry.1.push(i);
    }

    let mut files: Vec<(&std::path::Path, f32, Vec<usize>)> = per_file
        .into_iter()
        .map(|(path, (score, syms))| (path, score, syms))
        .collect();
    files.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    files.truncate(max_files);

    let out_files = files
        .into_iter()
        .map(|(path, score, mut sym_indices)| {
            sym_indices.sort_by(|&a, &b| {
                rank[b]
                    .partial_cmp(&rank[a])
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            sym_indices.truncate(max_symbols_per_file);
            sym_indices.sort_by_key(|&i| graph.symbols[i].id.line);
            let symbols = sym_indices
                .into_iter()
                .map(|i| {
                    let sym = &graph.symbols[i];
                    RepoMapSymbol {
                        name: sym.id.name.clone(),
                        kind: sym.kind.clone(),
                        line: sym.id.line,
                        score: rank[i],
                    }
                })
                .collect();
            RepoMapFile {
                path: path.to_path_buf(),
                score,
                symbols,
            }
        })
        .collect();

    RepoMap { files: out_files }
}

impl RepoMap {
    pub fn render(&self, max_chars: usize) -> String {
        if self.files.is_empty() {
            return String::new();
        }
        let mut out = String::with_capacity(max_chars.min(8 * 1024));
        out.push_str("[Repo map] Key files ranked by symbol-graph centrality:\n");
        for file in &self.files {
            let path = file.path.to_string_lossy().replace('\\', "/");
            let mut block = format!("{path}:\n");
            for sym in &file.symbols {
                block.push_str(&format!(
                    "  {} {} (L{})\n",
                    sym.kind, sym.name, sym.line
                ));
            }
            if out.len() + block.len() > max_chars {
                break;
            }
            out.push_str(&block);
        }
        out
    }
}
