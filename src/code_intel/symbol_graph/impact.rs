// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::core::{Edge, SymbolGraph, SymbolId};

pub const DEFAULT_MAX_IMPACT_DEPTH: usize = 2;
pub const DEFAULT_MAX_IMPACT_NODES: usize = 500;

#[must_use]
pub fn max_impact_depth() -> usize {
    std::env::var("SEN_MAX_IMPACT_DEPTH")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_MAX_IMPACT_DEPTH)
}

#[must_use]
pub fn max_impact_nodes() -> usize {
    std::env::var("SEN_MAX_IMPACT_NODES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_MAX_IMPACT_NODES)
}

#[derive(Debug, Clone, Default)]
pub struct ImpactResult {
    pub changed: Vec<SymbolId>,
    pub impacted: Vec<SymbolId>,
    pub impacted_files: Vec<PathBuf>,
    pub edges: Vec<Edge>,
    pub truncated: bool,
}

#[must_use]
pub fn seeds_for_files(graph: &SymbolGraph, files: &[PathBuf]) -> Vec<SymbolId> {
    let mut seeds: Vec<SymbolId> = Vec::new();
    for f in files {
        for entry in graph.symbols_in_file(f) {
            seeds.push(entry.id.clone());
        }
    }
    for f in files {
        seeds.push(SymbolId::file_anchor(f.clone()));
    }
    seeds
}

#[must_use]
pub fn impact_radius(
    graph: &SymbolGraph,
    seeds: &[SymbolId],
    max_depth: usize,
    max_nodes: usize,
) -> ImpactResult {
    let mut result = ImpactResult::default();
    if seeds.is_empty() {
        return result;
    }

    let seed_set: HashSet<SymbolId> = seeds.iter().cloned().collect();
    let mut visited: HashSet<SymbolId> = seed_set.clone();
    let mut impacted: Vec<SymbolId> = Vec::new();
    let mut frontier: Vec<SymbolId> = seeds.to_vec();

    'outer: for _ in 0..max_depth {
        let mut next_frontier: Vec<SymbolId> = Vec::new();
        for node in &frontier {
            for e in graph.out_edges(node) {
                if !visited.contains(&e.to) {
                    visited.insert(e.to.clone());
                    next_frontier.push(e.to.clone());
                    impacted.push(e.to.clone());
                    if visited.len() >= max_nodes {
                        result.truncated = true;
                        break 'outer;
                    }
                }
            }
            for e in graph.in_edges(node) {
                if !visited.contains(&e.from) {
                    visited.insert(e.from.clone());
                    next_frontier.push(e.from.clone());
                    impacted.push(e.from.clone());
                    if visited.len() >= max_nodes {
                        result.truncated = true;
                        break 'outer;
                    }
                }
            }
        }
        if next_frontier.is_empty() {
            break;
        }
        frontier = next_frontier;
    }

    let mut impacted_files: Vec<PathBuf> = Vec::new();
    let mut seen_files: HashSet<PathBuf> = HashSet::new();
    for id in &impacted {
        if seen_files.insert(id.file.clone()) {
            impacted_files.push(id.file.clone());
        }
    }

    let reached: HashSet<&SymbolId> = seed_set.iter().chain(impacted.iter()).collect();
    let mut edges: Vec<Edge> = Vec::new();
    let mut edge_seen: HashSet<(SymbolId, SymbolId, super::core::EdgeKind)> = HashSet::new();
    for id in reached.iter() {
        for e in graph.out_edges(id) {
            if reached.contains(&e.to) {
                let key = (e.from.clone(), e.to.clone(), e.kind);
                if edge_seen.insert(key) {
                    edges.push(e.clone());
                }
            }
        }
    }

    result.changed = seeds.iter().filter(|s| !s.is_file_anchor()).cloned().collect();
    result.impacted = impacted;
    result.impacted_files = impacted_files;
    result.edges = edges;
    result
}

#[must_use]
pub fn impacted_paths_excluding(result: &ImpactResult, exclude: &[PathBuf]) -> Vec<PathBuf> {
    let ex: HashSet<&PathBuf> = exclude.iter().collect();
    result
        .impacted_files
        .iter()
        .filter(|p| !ex.contains(p))
        .cloned()
        .collect()
}

#[must_use]
pub fn rel_display(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
