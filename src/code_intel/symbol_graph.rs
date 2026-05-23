// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::outline::{OutlineEntry, OutlineError, extract_outline};

pub const RECENT_EDITS_CAPACITY: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SymbolId {
    pub file: PathBuf,
    pub name: String,
    pub line: u32,
}

impl SymbolId {
    fn new(file: impl Into<PathBuf>, name: impl Into<String>, line: u32) -> Self {
        Self {
            file: file.into(),
            name: name.into(),
            line,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EdgeKind {
    Calls,
    Implements,
    Uses,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Edge {
    pub from: SymbolId,
    pub to: SymbolId,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SymbolEntry {
    pub id: SymbolId,
    pub kind: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SymbolGraph {
    pub symbols: Vec<SymbolEntry>,
    pub edges: Vec<Edge>,
    pub version: u32,

    #[serde(skip)]
    name_index: HashMap<String, Vec<SymbolId>>,

    #[serde(skip)]
    recent_edits: VecDeque<(SymbolId, Instant)>,
}

impl SymbolGraph {

    pub const SCHEMA_VERSION: u32 = 1;

    pub fn build(root: &Path) -> io::Result<Self> {
        let files = walk_source_files(root)?;
        let mut graph = SymbolGraph {
            version: Self::SCHEMA_VERSION,
            ..Default::default()
        };

        let mut per_file: Vec<(PathBuf, String, Vec<OutlineEntry>)> = Vec::new();
        for file in files {
            let Ok(src) = fs::read_to_string(&file) else {
                continue;
            };
            let rel = file.strip_prefix(root).unwrap_or(&file).to_path_buf();
            let entries = match extract_outline(&file, None) {
                Ok(v) => v,
                Err(OutlineError::UnsupportedLanguage(_)) => continue,
                Err(OutlineError::Io { .. }) => continue,
            };
            for entry in &entries {
                graph.symbols.push(SymbolEntry {
                    id: SymbolId::new(rel.clone(), &entry.name, entry.line),
                    kind: entry.kind.clone(),
                });
            }
            per_file.push((rel, src, entries));
        }

        let mut edge_set: BTreeSet<(SymbolId, SymbolId, EdgeKind)> = BTreeSet::new();
        for (rel, src, entries) in &per_file {
            let ranges = compute_symbol_ranges(entries, src);
            for (placeholder, body) in ranges {

                let sym = SymbolId {
                    file: rel.clone(),
                    name: placeholder.name,
                    line: placeholder.line,
                };
                for target in detect_calls(&body, &graph) {
                    if target == sym {
                        continue;
                    }
                    edge_set.insert((sym.clone(), target, EdgeKind::Calls));
                }
                for target in detect_implements(&body, &graph) {
                    if target == sym {
                        continue;
                    }
                    edge_set.insert((sym.clone(), target, EdgeKind::Implements));
                }
                for target in detect_uses(&body, &graph) {
                    if target == sym {
                        continue;
                    }
                    let call_seen =
                        edge_set.contains(&(sym.clone(), target.clone(), EdgeKind::Calls));
                    let impl_seen =
                        edge_set.contains(&(sym.clone(), target.clone(), EdgeKind::Implements));
                    if !call_seen && !impl_seen {
                        edge_set.insert((sym.clone(), target, EdgeKind::Uses));
                    }
                }
            }
        }

        graph.edges = edge_set
            .into_iter()
            .map(|(from, to, kind)| Edge { from, to, kind })
            .collect();
        graph.rebuild_name_index();
        Ok(graph)
    }

    pub fn persist(&self, root: &Path) -> io::Result<PathBuf> {
        let dir = root.join(".sen");
        fs::create_dir_all(&dir)?;
        let target = dir.join("symbol_graph.json");
        let tmp = dir.join("symbol_graph.json.tmp");
        let body =
            serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        fs::write(&tmp, &body)?;
        fs::rename(&tmp, &target)?;
        Ok(target)
    }

    pub fn load(root: &Path) -> io::Result<Option<Self>> {
        let path = root.join(".sen").join("symbol_graph.json");
        if !path.exists() {
            return Ok(None);
        }
        let body = fs::read(&path)?;
        let mut graph: Self = match serde_json::from_slice(&body) {
            Ok(g) => g,
            Err(_) => return Ok(None),
        };
        if graph.version != Self::SCHEMA_VERSION {
            return Ok(None);
        }
        graph.rebuild_name_index();
        Ok(Some(graph))
    }

    pub fn callers_of(&self, name: &str) -> Vec<&SymbolId> {
        self.edges
            .iter()
            .filter(|e| matches!(e.kind, EdgeKind::Calls) && e.to.name == name)
            .map(|e| &e.from)
            .collect()
    }

    pub fn implementors_of(&self, name: &str) -> Vec<&SymbolId> {
        self.edges
            .iter()
            .filter(|e| matches!(e.kind, EdgeKind::Implements) && e.to.name == name)
            .map(|e| &e.from)
            .collect()
    }

    pub fn rebuild_name_index(&mut self) {
        self.name_index.clear();
        for sym in &self.symbols {
            self.name_index
                .entry(sym.id.name.clone())
                .or_default()
                .push(sym.id.clone());
        }
    }

    #[must_use]
    pub fn find_by_name(&self, name: &str, kind: Option<&str>) -> Vec<SymbolId> {
        if let Some(ids) = self.name_index.get(name) {
            return match kind {
                Some(k) => ids
                    .iter()
                    .filter(|id| self.symbols.iter().any(|s| s.id == **id && s.kind == k))
                    .cloned()
                    .collect(),
                None => ids.clone(),
            };
        }

        self.symbols
            .iter()
            .filter(|s| s.id.name == name && kind.map_or(true, |k| s.kind == k))
            .map(|s| s.id.clone())
            .collect()
    }

    #[must_use]
    pub fn find_callers(&self, sym: &SymbolId) -> Vec<&SymbolId> {
        self.edges
            .iter()
            .filter(|e| matches!(e.kind, EdgeKind::Calls) && &e.to == sym)
            .map(|e| &e.from)
            .collect()
    }

    #[must_use]
    pub fn find_implementors(&self, sym: &SymbolId) -> Vec<&SymbolId> {
        self.edges
            .iter()
            .filter(|e| matches!(e.kind, EdgeKind::Implements) && &e.to == sym)
            .map(|e| &e.from)
            .collect()
    }

    #[must_use]
    pub fn find_recent_edits(&self, path: Option<&Path>, window: usize) -> Vec<SymbolId> {
        self.recent_edits
            .iter()
            .rev()
            .filter(|(sym, _)| match path {
                Some(p) => sym.file == *p,
                None => true,
            })
            .take(window)
            .map(|(sym, _)| sym.clone())
            .collect()
    }

    pub fn record_recent_edits<I: IntoIterator<Item = SymbolId>>(&mut self, ids: I) {
        let now = Instant::now();
        for id in ids {
            self.recent_edits.push_back((id, now));
            while self.recent_edits.len() > RECENT_EDITS_CAPACITY {
                self.recent_edits.pop_front();
            }
        }
    }

    pub fn partial_rebuild(
        &mut self,
        changed: &std::collections::HashSet<PathBuf>,
        removed: &std::collections::HashSet<PathBuf>,
        root: &Path,
    ) {
        use std::collections::HashSet;
        crate::observability::subsystem_metrics::incr_symbol_graph_rebuild();

        let to_rel = |p: &Path| -> PathBuf { p.strip_prefix(root).unwrap_or(p).to_path_buf() };
        let changed_rel: HashSet<PathBuf> = changed.iter().map(|p| to_rel(p)).collect();
        let removed_rel: HashSet<PathBuf> = removed.iter().map(|p| to_rel(p)).collect();
        let dirty_rel: HashSet<PathBuf> = changed_rel
            .iter()
            .chain(removed_rel.iter())
            .cloned()
            .collect();

        self.symbols.retain(|s| !dirty_rel.contains(&s.id.file));

        self.edges
            .retain(|e| !dirty_rel.contains(&e.from.file) && !removed_rel.contains(&e.to.file));

        let mut per_file: Vec<(PathBuf, String, Vec<OutlineEntry>)> = Vec::new();
        for abs_path in changed {
            let rel = to_rel(abs_path);
            let Ok(src) = fs::read_to_string(abs_path) else {
                continue;
            };
            let entries = match extract_outline(abs_path, None) {
                Ok(v) => v,
                Err(OutlineError::UnsupportedLanguage(_)) | Err(OutlineError::Io { .. }) => {
                    continue;
                }
            };
            for entry in &entries {
                self.symbols.push(SymbolEntry {
                    id: SymbolId::new(rel.clone(), &entry.name, entry.line),
                    kind: entry.kind.clone(),
                });
            }
            per_file.push((rel, src, entries));
        }

        let mut edge_set: BTreeSet<(SymbolId, SymbolId, EdgeKind)> = BTreeSet::new();
        for (rel, src, entries) in &per_file {
            let ranges = compute_symbol_ranges(entries, src);
            for (placeholder, body) in ranges {
                let sym = SymbolId {
                    file: rel.clone(),
                    name: placeholder.name,
                    line: placeholder.line,
                };
                for target in detect_calls(&body, self) {
                    if target == sym {
                        continue;
                    }
                    edge_set.insert((sym.clone(), target, EdgeKind::Calls));
                }
                for target in detect_implements(&body, self) {
                    if target == sym {
                        continue;
                    }
                    edge_set.insert((sym.clone(), target, EdgeKind::Implements));
                }
                for target in detect_uses(&body, self) {
                    if target == sym {
                        continue;
                    }
                    let call_seen =
                        edge_set.contains(&(sym.clone(), target.clone(), EdgeKind::Calls));
                    let impl_seen =
                        edge_set.contains(&(sym.clone(), target.clone(), EdgeKind::Implements));
                    if !call_seen && !impl_seen {
                        edge_set.insert((sym.clone(), target, EdgeKind::Uses));
                    }
                }
            }
        }

        for (from, to, kind) in edge_set {
            self.edges.push(Edge { from, to, kind });
        }

        self.rebuild_name_index();
        let newly_touched: Vec<SymbolId> = self
            .symbols
            .iter()
            .filter(|s| changed_rel.contains(&s.id.file))
            .map(|s| s.id.clone())
            .collect();
        self.record_recent_edits(newly_touched);
    }
}

fn walk_source_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    fn recurse(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let ft = entry.file_type()?;
            if ft.is_dir() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if matches!(
                    name_str.as_ref(),
                    ".git"
                        | ".sen"
                        | "target"
                        | "node_modules"
                        | "dist"
                        | "build"
                        | ".venv"
                        | "__pycache__"
                ) {
                    continue;
                }
                recurse(&path, out)?;
            } else if ft.is_file() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if matches!(
                        ext,
                        "rs" | "py" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "go"
                    ) {
                        out.push(path);
                    }
                }
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    if root.exists() {
        recurse(root, &mut out)?;
    }
    Ok(out)
}

fn compute_symbol_ranges(entries: &[OutlineEntry], src: &str) -> Vec<(SymbolId, String)> {
    let lines: Vec<&str> = src.lines().collect();
    let mut sorted: Vec<&OutlineEntry> = entries.iter().collect();
    sorted.sort_by_key(|e| e.line);
    let mut out = Vec::with_capacity(sorted.len());
    for (i, entry) in sorted.iter().enumerate() {
        let start = entry.line.saturating_sub(1) as usize;
        let end = if i + 1 < sorted.len() {
            (sorted[i + 1].line.saturating_sub(1) as usize).min(lines.len())
        } else {
            lines.len()
        };
        if start >= lines.len() {
            continue;
        }
        let body = lines[start..end].join("\n");

        out.push((
            SymbolId::new(PathBuf::new(), entry.name.clone(), entry.line),
            body,
        ));
    }
    out
}

fn detect_calls(body: &str, graph: &SymbolGraph) -> Vec<SymbolId> {
    let mut name_to: HashMap<&str, &SymbolId> = HashMap::new();
    for entry in &graph.symbols {
        name_to.entry(entry.id.name.as_str()).or_insert(&entry.id);
    }
    let mut out = Vec::new();
    for (name, sym) in name_to {
        if name.is_empty() {
            continue;
        }
        let needle = format!("{name}(");
        if body.contains(&needle) {
            out.push((*sym).clone());
        }
    }
    out
}

fn detect_implements(body: &str, graph: &SymbolGraph) -> Vec<SymbolId> {
    let mut out = Vec::new();
    for line in body.lines() {
        let t = line.trim_start();

        if let Some(rest) = t.strip_prefix("impl ") {
            if let Some(for_idx) = rest.find(" for ") {
                let trait_name = rest[..for_idx]
                    .split(['<', ' '])
                    .next()
                    .unwrap_or("")
                    .trim();
                if !trait_name.is_empty() {
                    if let Some(sym) = graph.symbols.iter().find(|s| s.id.name == trait_name) {
                        out.push(sym.id.clone());
                    }
                }
            }
        }

        if let Some(rest) = t.strip_prefix("class ") {
            if let Some(l) = rest.find('(') {
                if let Some(r) = rest.find(')') {
                    if r > l {
                        let parents = &rest[l + 1..r];
                        for parent in parents.split(',') {
                            let p = parent.trim();
                            if p.is_empty() {
                                continue;
                            }
                            if let Some(sym) = graph.symbols.iter().find(|s| s.id.name == p) {
                                out.push(sym.id.clone());
                            }
                        }
                    }
                }
            }
            if let Some(ext_idx) = rest.find(" extends ") {
                let tail = &rest[ext_idx + " extends ".len()..];
                let parent = tail.split([' ', '{']).next().unwrap_or("").trim();
                if !parent.is_empty() {
                    if let Some(sym) = graph.symbols.iter().find(|s| s.id.name == parent) {
                        out.push(sym.id.clone());
                    }
                }
            }
            if let Some(impl_idx) = rest.find(" implements ") {
                let tail = &rest[impl_idx + " implements ".len()..];
                for parent in tail.split([',', '{']) {
                    let p = parent.trim();
                    if p.is_empty() {
                        continue;
                    }
                    if let Some(sym) = graph.symbols.iter().find(|s| s.id.name == p) {
                        out.push(sym.id.clone());
                    }
                }
            }
        }
    }
    out
}

fn detect_uses(body: &str, graph: &SymbolGraph) -> Vec<SymbolId> {
    let mut out = Vec::new();
    for entry in &graph.symbols {
        let sym = &entry.id;
        let name = &sym.name;
        if name.is_empty() {
            continue;
        }

        let mut found = false;
        for (idx, _) in body.match_indices(name.as_str()) {
            let before_ok = idx == 0
                || !body
                    .as_bytes()
                    .get(idx.saturating_sub(1))
                    .map(|b| b.is_ascii_alphanumeric() || *b == b'_')
                    .unwrap_or(false);
            let after = idx + name.len();
            let after_ok = after >= body.len()
                || !body
                    .as_bytes()
                    .get(after)
                    .map(|b| b.is_ascii_alphanumeric() || *b == b'_')
                    .unwrap_or(false);
            if before_ok && after_ok {
                found = true;
                break;
            }
        }
        if found {
            out.push(sym.clone());
        }
    }
    out
}
