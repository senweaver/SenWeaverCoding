// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::super::outline::{OutlineEntry, OutlineError, extract_outline};

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

    #[must_use]
    pub fn file_anchor(file: impl Into<PathBuf>) -> Self {
        Self {
            file: file.into(),
            name: String::new(),
            line: 0,
        }
    }

    #[must_use]
    pub fn is_file_anchor(&self) -> bool {
        self.name.is_empty() && self.line == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EdgeKind {
    Calls,
    Implements,
    Uses,
    Imports,
    TestedBy,
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
    #[serde(default)]
    pub line_end: u32,
    #[serde(default)]
    pub is_test: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SymbolGraph {
    pub symbols: Vec<SymbolEntry>,
    pub edges: Vec<Edge>,
    pub version: u32,

    #[serde(skip)]
    name_index: HashMap<String, Vec<SymbolId>>,

    #[serde(skip)]
    out_index: HashMap<SymbolId, Vec<usize>>,

    #[serde(skip)]
    in_index: HashMap<SymbolId, Vec<usize>>,

    #[serde(skip)]
    symbol_index: HashMap<SymbolId, usize>,

    #[serde(skip)]
    symbols_by_file_name: HashMap<String, Vec<usize>>,

    #[serde(skip)]
    symbols_by_file: HashMap<PathBuf, Vec<usize>>,

    #[serde(skip)]
    edges_by_target_name: HashMap<String, Vec<usize>>,

    #[serde(skip)]
    recent_edits: VecDeque<(SymbolId, Instant)>,
}

impl SymbolGraph {
    pub const SCHEMA_VERSION: u32 = 2;

    pub fn build(root: &Path) -> io::Result<Self> {
        const MAX_INDEXED_FILES: usize = 20_000;
        const PROGRESS_LOG_EVERY: usize = 1_000;

        let mut files = walk_source_files(root)?;
        if files.len() > MAX_INDEXED_FILES {
            tracing::warn!(
                target: "code_intel",
                total = files.len(),
                cap = MAX_INDEXED_FILES,
                "symbol graph: workspace exceeds file cap; indexing a truncated subset"
            );
            files.truncate(MAX_INDEXED_FILES);
        }
        let total_files = files.len();
        let mut graph = SymbolGraph {
            version: Self::SCHEMA_VERSION,
            ..Default::default()
        };

        let mut per_file: Vec<(PathBuf, String, Vec<OutlineEntry>)> = Vec::new();
        let mut test_syms: HashSet<SymbolId> = HashSet::new();
        let mut known_files: HashSet<PathBuf> = HashSet::new();
        for (file_idx, file) in files.into_iter().enumerate() {
            if file_idx > 0 && file_idx % PROGRESS_LOG_EVERY == 0 {
                tracing::info!(
                    target: "code_intel",
                    processed = file_idx,
                    total = total_files,
                    "symbol graph build progress"
                );
            }
            let Ok(src) = fs::read_to_string(&file) else {
                continue;
            };
            let rel = file.strip_prefix(root).unwrap_or(&file).to_path_buf();
            let entries = match extract_outline(&file, None) {
                Ok(v) => v,
                Err(OutlineError::UnsupportedLanguage(_)) => continue,
                Err(OutlineError::Io { .. }) => continue,
            };
            known_files.insert(rel.clone());
            let lines: Vec<&str> = src.lines().collect();
            let is_test_file = is_test_path(&rel);
            let ends = compute_line_ends(&entries, lines.len() as u32);
            for (entry, &end) in entries.iter().zip(ends.iter()) {
                let id = SymbolId::new(rel.clone(), &entry.name, entry.line);
                let is_test =
                    is_test_file || is_test_symbol(&entry.name, entry.line, &lines);
                if is_test {
                    test_syms.insert(id.clone());
                }
                graph.symbols.push(SymbolEntry {
                    id,
                    kind: entry.kind.clone(),
                    line_end: end,
                    is_test,
                });
            }
            per_file.push((rel, src, entries));
        }

        graph.edges = collect_edges(&graph, &per_file, &test_syms, &known_files);
        graph.rebuild_name_index();
        Ok(graph)
    }

    pub fn persist(&self, root: &Path) -> io::Result<PathBuf> {
        let body = self.serialize_for_persist()?;
        Self::persist_bytes(root, &body)
    }

    pub fn serialize_for_persist(&self) -> io::Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(io::Error::other)
    }

    pub fn persist_bytes(root: &Path, body: &[u8]) -> io::Result<PathBuf> {
        let dir = root.join(".sen");
        fs::create_dir_all(&dir)?;
        let target = dir.join("symbol_graph.json");
        let tmp = dir.join("symbol_graph.json.tmp");
        fs::write(&tmp, body)?;
        fs::rename(&tmp, &target)?;
        Ok(target)
    }

    pub fn load_cached(root: &Path) -> Option<std::sync::Arc<Self>> {
        static CACHE: std::sync::OnceLock<
            parking_lot::Mutex<
                HashMap<PathBuf, (std::time::SystemTime, std::sync::Arc<SymbolGraph>)>,
            >,
        > = std::sync::OnceLock::new();
        let cache = CACHE.get_or_init(|| parking_lot::Mutex::new(HashMap::new()));

        let path = root.join(".sen").join("symbol_graph.json");
        let mtime = fs::metadata(&path).ok()?.modified().ok()?;
        {
            let guard = cache.lock();
            if let Some((cached_mtime, graph)) = guard.get(&path) {
                if *cached_mtime == mtime {
                    return Some(std::sync::Arc::clone(graph));
                }
            }
        }
        let graph = Self::load(root).ok().flatten()?;
        let arc = std::sync::Arc::new(graph);
        let mut guard = cache.lock();
        guard.insert(path, (mtime, std::sync::Arc::clone(&arc)));
        if guard.len() > 8 {
            let stale: Vec<PathBuf> = guard
                .iter()
                .filter(|(p, _)| !p.exists())
                .map(|(p, _)| p.clone())
                .collect();
            for p in stale {
                guard.remove(&p);
            }
        }
        Some(arc)
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
        self.edges_by_target(name, EdgeKind::Calls)
    }

    pub fn implementors_of(&self, name: &str) -> Vec<&SymbolId> {
        self.edges_by_target(name, EdgeKind::Implements)
    }

    fn edges_by_target(&self, name: &str, kind: EdgeKind) -> Vec<&SymbolId> {
        match self.edges_by_target_name.get(name) {
            Some(indices) => indices
                .iter()
                .filter_map(|&i| self.edges.get(i))
                .filter(|e| e.kind == kind)
                .map(|e| &e.from)
                .collect(),
            None if self.edges_by_target_name.is_empty() => self
                .edges
                .iter()
                .filter(|e| e.kind == kind && e.to.name == name)
                .map(|e| &e.from)
                .collect(),
            None => Vec::new(),
        }
    }

    pub fn rebuild_name_index(&mut self) {
        self.name_index.clear();
        self.symbol_index.clear();
        self.symbols_by_file_name.clear();
        self.symbols_by_file.clear();
        for (idx, sym) in self.symbols.iter().enumerate() {
            self.name_index
                .entry(sym.id.name.clone())
                .or_default()
                .push(sym.id.clone());
            self.symbol_index.insert(sym.id.clone(), idx);
            let file_key = sym
                .id
                .file
                .file_name()
                .map(|n| n.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            self.symbols_by_file_name
                .entry(file_key)
                .or_default()
                .push(idx);
            self.symbols_by_file
                .entry(sym.id.file.clone())
                .or_default()
                .push(idx);
        }
        self.rebuild_edge_index();
    }

    #[must_use]
    pub fn symbol_indices_for_file(&self, file: &Path) -> &[usize] {
        self.symbols_by_file
            .get(file)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    #[must_use]
    pub fn symbol_indices_for_file_name(&self, file_name_lower: &str) -> &[usize] {
        self.symbols_by_file_name
            .get(file_name_lower)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn rebuild_edge_index(&mut self) {
        self.out_index.clear();
        self.in_index.clear();
        self.edges_by_target_name.clear();
        for (idx, e) in self.edges.iter().enumerate() {
            self.out_index.entry(e.from.clone()).or_default().push(idx);
            self.in_index.entry(e.to.clone()).or_default().push(idx);
            self.edges_by_target_name
                .entry(e.to.name.clone())
                .or_default()
                .push(idx);
        }
    }

    #[must_use]
    pub fn out_edges(&self, id: &SymbolId) -> Vec<&Edge> {
        match self.out_index.get(id) {
            Some(indices) => indices.iter().filter_map(|&i| self.edges.get(i)).collect(),
            None => self
                .edges
                .iter()
                .filter(|e| &e.from == id)
                .collect(),
        }
    }

    #[must_use]
    pub fn in_edges(&self, id: &SymbolId) -> Vec<&Edge> {
        match self.in_index.get(id) {
            Some(indices) => indices.iter().filter_map(|&i| self.edges.get(i)).collect(),
            None => self
                .edges
                .iter()
                .filter(|e| &e.to == id)
                .collect(),
        }
    }

    #[must_use]
    pub fn symbol_entry(&self, id: &SymbolId) -> Option<&SymbolEntry> {
        if let Some(&idx) = self.symbol_index.get(id) {
            return self.symbols.get(idx);
        }
        self.symbols.iter().find(|s| &s.id == id)
    }

    #[must_use]
    pub fn symbols_in_file(&self, file: &Path) -> Vec<&SymbolEntry> {
        if !self.symbols_by_file.is_empty() {
            return self
                .symbols_by_file
                .get(file)
                .map(|indices| {
                    indices
                        .iter()
                        .filter_map(|&i| self.symbols.get(i))
                        .collect()
                })
                .unwrap_or_default();
        }
        self.symbols.iter().filter(|s| s.id.file == file).collect()
    }

    #[must_use]
    pub fn tests_covering(&self, id: &SymbolId) -> Vec<&SymbolId> {
        self.in_edges(id)
            .into_iter()
            .filter(|e| matches!(e.kind, EdgeKind::TestedBy))
            .map(|e| &e.from)
            .collect()
    }

    #[must_use]
    pub fn find_by_name(&self, name: &str, kind: Option<&str>) -> Vec<SymbolId> {
        if let Some(ids) = self.name_index.get(name) {
            return match kind {
                Some(k) => ids
                    .iter()
                    .filter(|id| {
                        self.symbol_index
                            .get(*id)
                            .and_then(|&idx| self.symbols.get(idx))
                            .is_some_and(|s| s.kind == k)
                    })
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
        self.in_edges(sym)
            .into_iter()
            .filter(|e| matches!(e.kind, EdgeKind::Calls))
            .map(|e| &e.from)
            .collect()
    }

    #[must_use]
    pub fn find_implementors(&self, sym: &SymbolId) -> Vec<&SymbolId> {
        self.in_edges(sym)
            .into_iter()
            .filter(|e| matches!(e.kind, EdgeKind::Implements))
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
        changed: &HashSet<PathBuf>,
        removed: &HashSet<PathBuf>,
        root: &Path,
    ) {
        crate::observability::subsystem_metrics::incr_symbol_graph_rebuild();

        let to_rel = |p: &Path| -> PathBuf { p.strip_prefix(root).unwrap_or(p).to_path_buf() };
        let changed_rel: HashSet<PathBuf> = changed.iter().map(|p| to_rel(p)).collect();
        let removed_rel: HashSet<PathBuf> = removed.iter().map(|p| to_rel(p)).collect();
        let dirty_rel: HashSet<PathBuf> = changed_rel
            .iter()
            .chain(removed_rel.iter())
            .cloned()
            .collect();

        const MAX_CALLER_RESCAN: usize = 512;
        let mut caller_rel: HashSet<PathBuf> = HashSet::new();
        for e in &self.edges {
            if dirty_rel.contains(&e.to.file) && !dirty_rel.contains(&e.from.file) {
                caller_rel.insert(e.from.file.clone());
            }
        }
        if caller_rel.len() > MAX_CALLER_RESCAN {
            tracing::debug!(
                target: "code_intel.symbol_graph",
                callers = caller_rel.len(),
                cap = MAX_CALLER_RESCAN,
                "capping caller re-scan for symbol-graph partial rebuild"
            );
            let capped: HashSet<PathBuf> =
                caller_rel.into_iter().take(MAX_CALLER_RESCAN).collect();
            caller_rel = capped;
        }

        let mut rescan_rel: HashSet<PathBuf> = dirty_rel.clone();
        rescan_rel.extend(caller_rel.iter().cloned());

        self.symbols.retain(|s| !dirty_rel.contains(&s.id.file));

        self.edges
            .retain(|e| !rescan_rel.contains(&e.from.file) && !removed_rel.contains(&e.to.file));

        let mut per_file: Vec<(PathBuf, String, Vec<OutlineEntry>)> = Vec::new();
        let mut test_syms: HashSet<SymbolId> = HashSet::new();
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
            let lines: Vec<&str> = src.lines().collect();
            let is_test_file = is_test_path(&rel);
            let ends = compute_line_ends(&entries, lines.len() as u32);
            for (entry, &end) in entries.iter().zip(ends.iter()) {
                let id = SymbolId::new(rel.clone(), &entry.name, entry.line);
                let is_test =
                    is_test_file || is_test_symbol(&entry.name, entry.line, &lines);
                if is_test {
                    test_syms.insert(id.clone());
                }
                self.symbols.push(SymbolEntry {
                    id,
                    kind: entry.kind.clone(),
                    line_end: end,
                    is_test,
                });
            }
            per_file.push((rel, src, entries));
        }

        for rel in &caller_rel {
            let abs_path = root.join(rel);
            let Ok(src) = fs::read_to_string(&abs_path) else {
                continue;
            };
            let entries = match extract_outline(&abs_path, None) {
                Ok(v) => v,
                Err(OutlineError::UnsupportedLanguage(_)) | Err(OutlineError::Io { .. }) => {
                    continue;
                }
            };
            per_file.push((rel.clone(), src, entries));
        }

        let mut known_files: HashSet<PathBuf> =
            self.symbols.iter().map(|s| s.id.file.clone()).collect();
        known_files.extend(changed_rel.iter().cloned());

        let new_edges = collect_edges(self, &per_file, &test_syms, &known_files);
        self.edges.extend(new_edges);

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

fn collect_edges(
    view: &SymbolGraph,
    per_file: &[(PathBuf, String, Vec<OutlineEntry>)],
    test_syms: &HashSet<SymbolId>,
    known_files: &HashSet<PathBuf>,
) -> Vec<Edge> {
    let mut edge_set: BTreeSet<(SymbolId, SymbolId, EdgeKind)> = BTreeSet::new();

    let mut name_to: HashMap<&str, Vec<&SymbolId>> = HashMap::new();
    for entry in &view.symbols {
        if entry.id.name.is_empty() {
            continue;
        }
        name_to.entry(entry.id.name.as_str()).or_default().push(&entry.id);
    }
    let names: Vec<&str> = name_to.keys().copied().collect();
    let call_use_ac = aho_corasick::AhoCorasick::new(&names).ok();

    for (rel, src, entries) in per_file {
        let ranges = compute_symbol_ranges(entries, src);
        for (placeholder, body) in ranges {
            let sym = SymbolId {
                file: rel.clone(),
                name: placeholder.name,
                line: placeholder.line,
            };
            let sym_is_test = test_syms.contains(&sym);
            let (calls, uses) = match call_use_ac.as_ref() {
                Some(ac) => detect_calls_and_uses(&body, rel, &name_to, ac, &names),
                None => (Vec::new(), Vec::new()),
            };
            for target in calls {
                if target == sym {
                    continue;
                }
                edge_set.insert((sym.clone(), target.clone(), EdgeKind::Calls));
                if sym_is_test {
                    edge_set.insert((sym.clone(), target, EdgeKind::TestedBy));
                }
            }
            for target in detect_implements(&body, rel, &name_to) {
                if target == sym {
                    continue;
                }
                edge_set.insert((sym.clone(), target, EdgeKind::Implements));
            }
            for target in uses {
                if target == sym {
                    continue;
                }
                let call_seen = edge_set.contains(&(sym.clone(), target.clone(), EdgeKind::Calls));
                let impl_seen =
                    edge_set.contains(&(sym.clone(), target.clone(), EdgeKind::Implements));
                if !call_seen && !impl_seen {
                    edge_set.insert((sym.clone(), target, EdgeKind::Uses));
                }
            }
        }

        for target_file in resolve_import_targets(src, rel, known_files) {
            if target_file == *rel {
                continue;
            }
            edge_set.insert((
                SymbolId::file_anchor(rel.clone()),
                SymbolId::file_anchor(target_file),
                EdgeKind::Imports,
            ));
        }
    }

    edge_set
        .into_iter()
        .map(|(from, to, kind)| Edge { from, to, kind })
        .collect()
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
                if crate::util::is_index_skip_dir(name_str.as_ref()) {
                    continue;
                }
                recurse(&path, out)?;
            } else if ft.is_file() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if matches!(
                        ext,
                        "rs" | "py" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "go"
                            | "java" | "c" | "h" | "cpp" | "hpp" | "cc"
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

fn compute_line_ends(entries: &[OutlineEntry], total_lines: u32) -> Vec<u32> {
    let mut sorted_lines: Vec<u32> = entries.iter().map(|e| e.line).collect();
    sorted_lines.sort_unstable();
    sorted_lines.dedup();
    entries
        .iter()
        .map(|e| {
            let next = sorted_lines.iter().copied().find(|&l| l > e.line);
            match next {
                Some(n) => n.saturating_sub(1).max(e.line),
                None => total_lines.max(e.line),
            }
        })
        .collect()
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

fn resolve_same_name<'a>(
    candidates: &[&'a SymbolId],
    from_file: &Path,
) -> Option<&'a SymbolId> {
    candidates
        .iter()
        .find(|s| s.file == from_file)
        .or_else(|| {
            candidates
                .iter()
                .find(|s| s.file.parent() == from_file.parent())
        })
        .or_else(|| candidates.first())
        .copied()
}

fn detect_calls_and_uses(
    body: &str,
    from_file: &Path,
    name_to: &HashMap<&str, Vec<&SymbolId>>,
    ac: &aho_corasick::AhoCorasick,
    names: &[&str],
) -> (Vec<SymbolId>, Vec<SymbolId>) {
    let bytes = body.as_bytes();
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut call_names: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut use_names: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for m in ac.find_overlapping_iter(body) {
        let start = m.start();
        let end = m.end();
        let before_ok = start == 0 || !is_ident(bytes[start - 1]);
        if !before_ok {
            continue;
        }
        let after_byte = bytes.get(end).copied();
        if after_byte.map(is_ident).unwrap_or(false) {
            continue;
        }
        let name = names[m.pattern().as_usize()];
        if after_byte == Some(b'(') {
            call_names.insert(name);
        } else {
            use_names.insert(name);
        }
    }
    for name in &call_names {
        use_names.remove(name);
    }
    let calls = call_names
        .iter()
        .filter_map(|n| name_to.get(*n).and_then(|c| resolve_same_name(c, from_file)))
        .cloned()
        .collect();
    let uses = use_names
        .iter()
        .filter_map(|n| name_to.get(*n).and_then(|c| resolve_same_name(c, from_file)))
        .cloned()
        .collect();
    (calls, uses)
}

fn detect_implements(
    body: &str,
    from_file: &Path,
    name_to: &HashMap<&str, Vec<&SymbolId>>,
) -> Vec<SymbolId> {
    let resolve = |name: &str, out: &mut Vec<SymbolId>| {
        if name.is_empty() {
            return;
        }
        if let Some(candidates) = name_to.get(name) {
            if let Some(id) = resolve_same_name(candidates, from_file) {
                out.push(id.clone());
            }
        }
    };

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
                resolve(trait_name, &mut out);
            }
        }

        if let Some(rest) = t.strip_prefix("class ") {
            if let Some(l) = rest.find('(') {
                if let Some(r) = rest.find(')') {
                    if r > l {
                        let parents = &rest[l + 1..r];
                        for parent in parents.split(',') {
                            resolve(parent.trim(), &mut out);
                        }
                    }
                }
            }
            if let Some(ext_idx) = rest.find(" extends ") {
                let tail = &rest[ext_idx + " extends ".len()..];
                let parent = tail.split([' ', '{']).next().unwrap_or("").trim();
                resolve(parent, &mut out);
            }
            if let Some(impl_idx) = rest.find(" implements ") {
                let tail = &rest[impl_idx + " implements ".len()..];
                for parent in tail.split([',', '{']) {
                    resolve(parent.trim(), &mut out);
                }
            }
        }
    }
    out
}

fn is_test_path(rel: &Path) -> bool {
    let has_test_dir = rel.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        s == "tests" || s == "test" || s == "__tests__"
    });
    if has_test_dir {
        return true;
    }
    let file = rel
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    let lower = file.to_ascii_lowercase();
    lower.starts_with("test_")
        || lower.ends_with("_test.py")
        || lower.ends_with("_test.go")
        || lower.ends_with("_test.rs")
        || lower.contains(".test.")
        || lower.contains(".spec.")
        || lower.ends_with("_spec.rb")
}

fn is_test_symbol(name: &str, line: u32, lines: &[&str]) -> bool {
    if name.starts_with("Test") || name.to_ascii_lowercase().starts_with("test_") {
        return true;
    }
    let idx = line.saturating_sub(1) as usize;
    let mut cursor = idx;
    while cursor > 0 {
        cursor -= 1;
        let prev = lines.get(cursor).map(|s| s.trim()).unwrap_or("");
        if prev.is_empty() {
            continue;
        }
        if prev.starts_with("#[") || prev.starts_with("//") {
            if prev.contains("test]") || prev.contains("test(") || prev.contains("::test") {
                return true;
            }
            continue;
        }
        break;
    }
    false
}

fn resolve_import_targets(
    src: &str,
    rel: &Path,
    known_files: &HashSet<PathBuf>,
) -> Vec<PathBuf> {
    let ext = rel
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let parent = rel.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut out: Vec<PathBuf> = Vec::new();

    match ext.as_str() {
        "rs" => {
            for line in src.lines() {
                let t = line.trim_start();
                let rest = t
                    .strip_prefix("pub mod ")
                    .or_else(|| t.strip_prefix("mod "));
                if let Some(rest) = rest {
                    let modname = rest
                        .split([';', ' ', '{'])
                        .next()
                        .unwrap_or("")
                        .trim();
                    if modname.is_empty() {
                        continue;
                    }
                    for cand in [
                        parent.join(format!("{modname}.rs")),
                        parent.join(modname).join("mod.rs"),
                    ] {
                        if known_files.contains(&cand) {
                            out.push(cand);
                            break;
                        }
                    }
                }
            }
        }
        "py" | "pyi" => {
            for line in src.lines() {
                let t = line.trim_start();
                if let Some(rest) = t.strip_prefix("from ") {
                    let module = rest.split(" import ").next().unwrap_or("").trim();
                    if let Some(target) = resolve_python_relative(module, &parent, known_files) {
                        out.push(target);
                    }
                }
            }
        }
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" => {
            for line in src.lines() {
                let t = line.trim();
                if let Some(spec) = extract_js_import_spec(t) {
                    if spec.starts_with('.') {
                        if let Some(target) = resolve_js_relative(&spec, &parent, known_files) {
                            out.push(target);
                        }
                    }
                }
            }
        }
        _ => {}
    }

    out
}

fn resolve_python_relative(
    module: &str,
    parent: &Path,
    known_files: &HashSet<PathBuf>,
) -> Option<PathBuf> {
    if !module.starts_with('.') {
        return None;
    }
    let dots = module.chars().take_while(|c| *c == '.').count();
    let tail = &module[dots..];
    let mut base = parent.to_path_buf();
    for _ in 1..dots {
        base = base.parent().map(Path::to_path_buf).unwrap_or_default();
    }
    let rel_mod = tail.replace('.', "/");
    let target_base = if rel_mod.is_empty() {
        base
    } else {
        base.join(rel_mod)
    };
    for cand in [
        target_base.with_extension("py"),
        target_base.join("__init__.py"),
    ] {
        if known_files.contains(&cand) {
            return Some(cand);
        }
    }
    None
}

fn extract_js_import_spec(line: &str) -> Option<String> {
    let from_marker = if line.starts_with("import ") || line.starts_with("export ") {
        line.find(" from ").map(|i| i + " from ".len())
    } else {
        None
    };
    if let Some(start) = from_marker {
        return extract_quoted(&line[start..]);
    }
    if let Some(idx) = line.find("require(") {
        return extract_quoted(&line[idx + "require(".len()..]);
    }
    if line.starts_with("import ") {
        return extract_quoted(line);
    }
    None
}

fn extract_quoted(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let quote = bytes
        .iter()
        .position(|&b| b == b'\'' || b == b'"' || b == b'`')?;
    let qc = bytes[quote];
    let rest = &s[quote + 1..];
    let end = rest.find(qc as char)?;
    Some(rest[..end].to_string())
}

fn resolve_js_relative(
    spec: &str,
    parent: &Path,
    known_files: &HashSet<PathBuf>,
) -> Option<PathBuf> {
    let raw = parent.join(spec);
    let normalized = normalize_path(&raw);
    let exts = ["ts", "tsx", "js", "jsx", "mjs", "cjs"];
    for ext in exts {
        let cand = normalized.with_extension(ext);
        if known_files.contains(&cand) {
            return Some(cand);
        }
    }
    for ext in exts {
        let cand = normalized.join(format!("index.{ext}"));
        if known_files.contains(&cand) {
            return Some(cand);
        }
    }
    if known_files.contains(&normalized) {
        return Some(normalized);
    }
    None
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}
