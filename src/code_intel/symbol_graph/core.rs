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
        let dir = root.join(".sen");
        fs::create_dir_all(&dir)?;
        let target = dir.join("symbol_graph.json");
        let tmp = dir.join("symbol_graph.json.tmp");
        let body = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
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
        self.symbol_index.clear();
        for (idx, sym) in self.symbols.iter().enumerate() {
            self.name_index
                .entry(sym.id.name.clone())
                .or_default()
                .push(sym.id.clone());
            self.symbol_index.insert(sym.id.clone(), idx);
        }
        self.rebuild_edge_index();
    }

    fn rebuild_edge_index(&mut self) {
        self.out_index.clear();
        self.in_index.clear();
        for (idx, e) in self.edges.iter().enumerate() {
            self.out_index.entry(e.from.clone()).or_default().push(idx);
            self.in_index.entry(e.to.clone()).or_default().push(idx);
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

        self.symbols.retain(|s| !dirty_rel.contains(&s.id.file));

        self.edges
            .retain(|e| !dirty_rel.contains(&e.from.file) && !removed_rel.contains(&e.to.file));

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
    for (rel, src, entries) in per_file {
        let ranges = compute_symbol_ranges(entries, src);
        for (placeholder, body) in ranges {
            let sym = SymbolId {
                file: rel.clone(),
                name: placeholder.name,
                line: placeholder.line,
            };
            let sym_is_test = test_syms.contains(&sym);
            for target in detect_calls(&body, view) {
                if target == sym {
                    continue;
                }
                edge_set.insert((sym.clone(), target.clone(), EdgeKind::Calls));
                if sym_is_test {
                    edge_set.insert((sym.clone(), target, EdgeKind::TestedBy));
                }
            }
            for target in detect_implements(&body, view) {
                if target == sym {
                    continue;
                }
                edge_set.insert((sym.clone(), target, EdgeKind::Implements));
            }
            for target in detect_uses(&body, view) {
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

fn detect_calls(body: &str, graph: &SymbolGraph) -> Vec<SymbolId> {
    let mut name_to: HashMap<&str, &SymbolId> = HashMap::new();
    for entry in &graph.symbols {
        if entry.id.name.is_empty() {
            continue;
        }
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
