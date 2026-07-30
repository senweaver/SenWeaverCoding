// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use crate::code_intel::search::{IncrementalIndex, heuristic::Search as HeuristicSearch};
use crate::code_intel::symbol_graph::{EdgeKind, SymbolGraph, SymbolId};
use crate::context::builder::{LspContextSource, RagSource, SymbolGraphLookup};
use crate::context::lsp_ctx::LspSnapshot;
use crate::context::rag_ctx::SearchHit;
use crate::context::symbols_ctx::SymbolSnapshot;
use crate::rag::vector_code_index::{SharedVectorCodeIndex, reciprocal_rank_fusion};

struct StaticLspContextSource;

#[async_trait::async_trait]
impl LspContextSource for StaticLspContextSource {
    async fn collect(&self, focus: &[PathBuf]) -> Vec<LspSnapshot> {
        let services = match crate::services::try_get_services() {
            Some(s) => s,
            None => return Vec::new(),
        };
        let all = services.lsp.get_all_diagnostics().await;
        let cwd = crate::session::current_session_context()
            .map(|c| PathBuf::from(c.workspace_dir))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();
        let mut out = Vec::with_capacity(focus.len());
        for path in focus {
            let abs = if path.is_absolute() {
                path.clone()
            } else {
                cwd.join(path)
            };
            let key = crate::services::lsp::canonical_diag_key(&abs);
            let diagnostics = all
                .get(&key)
                .or_else(|| all.get(&abs))
                .or_else(|| all.get(path))
                .cloned()
                .unwrap_or_default();
            let summary = diagnostics
                .iter()
                .take(3)
                .map(|d| d.message.clone())
                .collect::<Vec<_>>()
                .join("; ");
            let (hover_line, hover_char) = if let Some(diagnostic) = diagnostics.first() {
                (
                    diagnostic.range.start_line,
                    diagnostic.range.start_character,
                )
            } else {
                let abs_for_outline = abs.clone();
                tokio::task::spawn_blocking(move || {
                    crate::code_intel::outline::extract_outline(&abs_for_outline, None)
                        .ok()
                        .and_then(|entries| entries.into_iter().next())
                        .map(|entry| {
                            let line0 = entry.line.saturating_sub(1);
                            let col = symbol_name_column(&abs_for_outline, line0, &entry.name);
                            (line0, col)
                        })
                })
                .await
                .ok()
                .flatten()
                .unwrap_or((0, 0))
            };
            let hover = services
                .lsp
                .hover_if_running(&abs, hover_line, hover_char)
                .await;
            out.push(LspSnapshot {
                path: path.clone(),
                diagnostics: diagnostics.len(),
                summary,
                hover,
            });
        }
        out
    }
}

fn symbol_name_column(abs: &Path, line0: u32, name: &str) -> u32 {
    let Ok(content) = std::fs::read_to_string(abs) else {
        return 0;
    };
    let Some(line) = content.lines().nth(line0 as usize) else {
        return 0;
    };
    if !name.is_empty() {
        if let Some(byte_idx) = line.find(name) {
            return line[..byte_idx].chars().count() as u32;
        }
    }
    line.chars().take_while(|c| c.is_whitespace()).count() as u32
}

struct SymbolGraphAdapter {
    writer: Arc<crate::code_intel::symbol_graph::incremental::SymbolGraphWriter>,
    workspace_root: PathBuf,
    per_file_cap: usize,
    dependents_cap: usize,
    repo_map_cache: parking_lot::Mutex<Option<(u64, usize, usize, usize, String)>>,
}

impl SymbolGraphLookup for SymbolGraphAdapter {
    fn snapshot_for_focus(&self, paths: &[PathBuf], query: Option<&str>) -> Vec<SymbolSnapshot> {
        if paths.is_empty() {
            return Vec::new();
        }
        let graph_lock = self.writer.graph();
        let graph = graph_lock.read();
        if graph.symbols.is_empty() {
            return Vec::new();
        }
        let query_terms: Vec<String> = query
            .map(|q| {
                q.split(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .filter(|t| t.len() >= 3)
                    .map(|t| t.to_ascii_lowercase())
                    .collect()
            })
            .unwrap_or_default();
        let mut out = Vec::new();
        for path in paths {
            let rel = relativize_to_workspace(path, &self.workspace_root);
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let file_name_key = rel
                .file_name()
                .map(|n| n.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            let candidate_indices = graph.symbol_indices_for_file_name(&file_name_key);
            let mut scored: Vec<(u32, usize)> = Vec::new();
            for &sym_idx in candidate_indices {
                let sym = &graph.symbols[sym_idx];
                if !same_file(&sym.id.file, &rel) {
                    continue;
                }
                let name_lc = sym.id.name.to_ascii_lowercase();
                let mut score = 1u32;
                if !stem.is_empty() && name_lc.contains(&stem) {
                    score += 8;
                }
                if stem.contains(&name_lc) && name_lc.len() >= 3 {
                    score += 6;
                }
                for term in &query_terms {
                    if name_lc == *term {
                        score += 12;
                    } else if name_lc.contains(term.as_str()) {
                        score += 6;
                    }
                }
                if matches!(
                    sym.kind.as_str(),
                    "function" | "method" | "class" | "struct" | "trait" | "interface"
                ) {
                    score += 3;
                }
                scored.push((score, sym_idx));
            }
            scored.sort_by(|a, b| {
                b.0.cmp(&a.0).then_with(|| {
                    graph.symbols[a.1].id.line.cmp(&graph.symbols[b.1].id.line)
                })
            });
            let mut file_lines: Option<Vec<String>> = None;
            let mut file_imports = collect_file_imports(&graph, &rel, self.dependents_cap);
            for (_, sym_idx) in scored.into_iter().take(self.per_file_cap) {
                let sym = &graph.symbols[sym_idx];
                if file_lines.is_none() {
                    file_lines = Some(read_file_lines(&self.workspace_root, &sym.id.file));
                }
                let signature = file_lines
                    .as_ref()
                    .and_then(|lines| {
                        lines
                            .get((sym.id.line as usize).saturating_sub(1))
                            .filter(|_| sym.id.line > 0)
                    })
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                let dependents = collect_dependents(&graph, &sym.id, self.dependents_cap);
                out.push(SymbolSnapshot {
                    name: sym.id.name.clone(),
                    kind: sym.kind.clone(),
                    path: rebuild_display_path(&self.workspace_root, &sym.id.file),
                    line: sym.id.line,
                    line_end: sym.line_end,
                    signature,
                    dependents,
                    imports: std::mem::take(&mut file_imports),
                });
            }
        }
        out
    }

    fn repo_map(
        &self,
        max_files: usize,
        max_symbols_per_file: usize,
        max_chars: usize,
    ) -> String {
        let graph_lock = self.writer.graph();
        let graph = graph_lock.read();
        if graph.symbols.is_empty() {
            return String::new();
        }
        let cache_key = self.writer.generation();
        {
            let cache = self.repo_map_cache.lock();
            if let Some((key, files, symbols, chars, rendered)) = cache.as_ref() {
                if *key == cache_key
                    && *files == max_files
                    && *symbols == max_symbols_per_file
                    && *chars == max_chars
                {
                    return rendered.clone();
                }
            }
        }
        let map = crate::code_intel::repo_map::build_repo_map(
            &graph,
            max_files,
            max_symbols_per_file,
        );
        let rendered = map.render(max_chars);
        *self.repo_map_cache.lock() = Some((
            cache_key,
            max_files,
            max_symbols_per_file,
            max_chars,
            rendered.clone(),
        ));
        rendered
    }
}

fn read_file_lines(root: &Path, rel: &Path) -> Vec<String> {
    let abs = if rel.is_absolute() {
        rel.to_path_buf()
    } else {
        root.join(rel)
    };
    std::fs::read_to_string(&abs)
        .map(|body| body.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

const DENSE_SNIPPET_MAX_LINES: usize = 12;
const DENSE_SNIPPET_MAX_BYTES: usize = 800;

fn truncate_dense_snippet(snippet: &str) -> String {
    let mut out = String::new();
    let mut lines = 0usize;
    for line in snippet.lines() {
        if lines >= DENSE_SNIPPET_MAX_LINES || out.len() >= DENSE_SNIPPET_MAX_BYTES {
            out.push_str("\n…");
            break;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        let remaining = DENSE_SNIPPET_MAX_BYTES.saturating_sub(out.len());
        if line.len() > remaining {
            out.push_str(crate::util::truncate_str_bytes(line, remaining));
            out.push_str(" …");
            break;
        }
        out.push_str(line.trim_end());
        lines += 1;
    }
    out
}

struct CodeRagSource {
    index: Arc<dyn IncrementalIndex>,
    vector_index: Option<SharedVectorCodeIndex>,
    top_k_default: usize,
}

#[async_trait::async_trait]
impl RagSource for CodeRagSource {
    async fn retrieve(&self, query: &str, top_k: usize) -> Vec<SearchHit> {
        if query.trim().is_empty() {
            return Vec::new();
        }
        let limit = if top_k == 0 {
            self.top_k_default.max(1)
        } else {
            top_k.min(self.top_k_default.max(1))
        };
        let lexical = {
            let index = self.index.clone();
            let q = query.to_string();
            let focus = crate::context::builder::FocusPathRegistry::current();
            let search_limit = limit.saturating_mul(2);
            match tokio::task::spawn_blocking(move || {
                index.search_with_focus(&q, search_limit, &focus)
            })
            .await
            {
                Ok(Ok(hits)) => hits,
                Ok(Err(err)) => {
                    tracing::warn!(
                        target: "rag.code_index",
                        error = %err,
                        "lexical retrieval failed; continuing without lexical hits"
                    );
                    Vec::new()
                }
                Err(join_err) => {
                    tracing::warn!(
                        target: "rag.code_index",
                        error = %join_err,
                        "lexical retrieval task panicked; continuing without lexical hits"
                    );
                    Vec::new()
                }
            }
        };
        let Some(vector) = self.vector_index.as_ref() else {
            let mut out = lexical;
            out.truncate(limit);
            return out;
        };
        let dense = match vector.search(query, limit * 2).await {
            Ok(hits) => hits,
            Err(err) => {
                tracing::warn!(
                    target: "rag.code_index",
                    error = %err,
                    "dense retrieval failed; falling back to lexical-only"
                );
                let mut out = lexical;
                out.truncate(limit);
                return out;
            }
        };
        if dense.is_empty() {
            if !vector.is_ready().await {
                let reason = if vector.dimensions() == 0 {
                    "embedder unavailable (no usable embedding backend)"
                } else {
                    "vector index is empty (seeding pending or failed)"
                };
                tracing::warn!(
                    target: "rag.code_index",
                    reason,
                    "dense retrieval unavailable; returning lexical-only results"
                );
            }
            let mut out = lexical;
            out.truncate(limit);
            return out;
        }
        let mut chunk_ranges: std::collections::HashMap<PathBuf, Vec<(u32, u32)>> =
            std::collections::HashMap::new();
        for d in &dense {
            chunk_ranges
                .entry(d.path.clone())
                .or_default()
                .push((d.start_line, d.end_line));
        }
        let bucket_for = |path: &PathBuf, line: u32| -> u32 {
            chunk_ranges
                .get(path)
                .and_then(|ranges| {
                    ranges
                        .iter()
                        .find(|(start, end)| line >= *start && line <= *end)
                        .map(|(start, _)| *start)
                })
                .unwrap_or(line)
        };
        let lexical_keys: Vec<(PathBuf, u32)> = lexical
            .iter()
            .map(|h| (h.path.clone(), bucket_for(&h.path, h.line)))
            .collect();
        let dense_keys: Vec<(PathBuf, u32)> = dense
            .iter()
            .map(|h| (h.path.clone(), h.start_line))
            .collect();
        let fused = reciprocal_rank_fusion(&[lexical_keys, dense_keys], 60);
        let mut out: Vec<SearchHit> = Vec::with_capacity(fused.len().min(limit));
        for ((path, bucket), _score) in fused.into_iter().take(limit) {
            if let Some(hit) = lexical
                .iter()
                .find(|h| h.path == path && bucket_for(&h.path, h.line) == bucket)
            {
                out.push(hit.clone());
                continue;
            }
            if let Some(d) = dense
                .iter()
                .find(|d| d.path == path && d.start_line == bucket)
            {
                out.push(SearchHit {
                    path: d.path.clone(),
                    line: d.start_line,
                    snippet: truncate_dense_snippet(&d.snippet),
                    end_line: Some(d.end_line),
                });
            }
        }
        out
    }
}

static CODE_RAG_CACHE: OnceLock<
    RwLock<std::collections::HashMap<PathBuf, Arc<dyn IncrementalIndex>>>,
> = OnceLock::new();

static VECTOR_INDEX_LOCKS: OnceLock<
    parking_lot::Mutex<std::collections::HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>,
> = OnceLock::new();

fn vector_index_lock(root: &Path) -> Arc<tokio::sync::Mutex<()>> {
    let map = VECTOR_INDEX_LOCKS.get_or_init(|| {
        parking_lot::Mutex::new(std::collections::HashMap::new())
    });
    let mut guard = map.lock();
    guard
        .entry(root.to_path_buf())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

static CODE_RAG_VECTOR_CACHE: OnceLock<
    RwLock<std::collections::HashMap<PathBuf, (u64, SharedVectorCodeIndex)>>,
> = OnceLock::new();

fn code_rag_cache(
) -> &'static RwLock<std::collections::HashMap<PathBuf, Arc<dyn IncrementalIndex>>> {
    CODE_RAG_CACHE.get_or_init(|| {
        tracing::debug!(
            target: "agent.loop_services",
            cache = "code_rag",
            kind = "workspace-bucketed-global",
            reason = "context-builder call-sites are not handed an AgentLoopServices instance",
            "initialising workspace-bucketed global cache"
        );
        RwLock::new(std::collections::HashMap::new())
    })
}

fn code_rag_vector_cache(
) -> &'static RwLock<std::collections::HashMap<PathBuf, (u64, SharedVectorCodeIndex)>> {
    CODE_RAG_VECTOR_CACHE.get_or_init(|| {
        tracing::debug!(
            target: "agent.loop_services",
            cache = "code_rag_vector",
            kind = "workspace-bucketed-global",
            reason = "context-builder call-sites are not handed an AgentLoopServices instance",
            "initialising workspace-bucketed global cache"
        );
        RwLock::new(std::collections::HashMap::new())
    })
}

static VECTOR_SEED_STARTED: OnceLock<RwLock<std::collections::HashSet<(PathBuf, u64)>>> =
    OnceLock::new();

fn vector_seed_started() -> &'static RwLock<std::collections::HashSet<(PathBuf, u64)>> {
    VECTOR_SEED_STARTED.get_or_init(|| RwLock::new(std::collections::HashSet::new()))
}

fn clear_vector_seed_marker(root: &Path, fingerprint: u64) {
    if let Ok(mut started) = vector_seed_started().write() {
        started.remove(&(root.to_path_buf(), fingerprint));
    }
}

const VECTOR_SEED_CHUNK_LINES: usize = 80;
const VECTOR_SEED_MAX_FILE_BYTES: u64 = 512 * 1024;
const VECTOR_SEED_MAX_CHUNKS: usize = 4000;
const VECTOR_SEED_CHUNK_CAP_CEILING: usize = 24_000;
const VECTOR_SEED_EMBED_BATCH: usize = 16;
static VECTOR_CHUNK_CAPS: OnceLock<parking_lot::RwLock<std::collections::HashMap<PathBuf, usize>>> =
    OnceLock::new();

fn adaptive_vector_chunk_cap(total_files: usize) -> usize {
    total_files
        .saturating_mul(6)
        .clamp(VECTOR_SEED_MAX_CHUNKS, VECTOR_SEED_CHUNK_CAP_CEILING)
}

fn remember_vector_chunk_cap(root: &Path, cap: usize) {
    VECTOR_CHUNK_CAPS
        .get_or_init(|| parking_lot::RwLock::new(std::collections::HashMap::new()))
        .write()
        .insert(root.to_path_buf(), cap);
}

fn vector_chunk_cap(root: &Path) -> usize {
    VECTOR_CHUNK_CAPS
        .get_or_init(|| parking_lot::RwLock::new(std::collections::HashMap::new()))
        .read()
        .get(root)
        .copied()
        .unwrap_or(VECTOR_SEED_MAX_CHUNKS)
}

pub(crate) fn is_seedable_source_file(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(
        ext,
        "rs" | "py" | "js" | "ts" | "tsx" | "jsx" | "go" | "java" | "c" | "h" | "cpp" | "hpp"
            | "cc" | "cs" | "rb" | "php" | "swift" | "kt" | "scala" | "sh" | "ps1" | "lua"
            | "zig" | "dart" | "vue" | "svelte" | "sql" | "toml" | "yaml" | "yml" | "md"
    )
}

fn chunk_source_for_vector_index(
    root: &Path,
    path: &Path,
    content: &str,
) -> Vec<crate::rag::vector_code_index::CodeChunk> {
    let rel = relativize_to_workspace(path, root);
    let rel_display = rel.to_string_lossy().replace('\\', "/");
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }

    let mut boundaries: Vec<usize> = vec![0];
    if let Ok(entries) =
        crate::code_intel::outline::extract_outline_from_source(path, content, None)
    {
        for entry in entries {
            let line0 = entry.line.saturating_sub(1) as usize;
            if line0 > 0 && line0 < lines.len() {
                boundaries.push(line0);
            }
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    boundaries.push(lines.len());

    let mut out = Vec::new();
    let emit = |start: usize, end: usize, out: &mut Vec<_>| {
        let body = lines[start..end].join("\n");
        if body.trim().is_empty() {
            return;
        }
        out.push(crate::rag::vector_code_index::CodeChunk {
            id: format!("{rel_display}#{}", start + 1),
            path: path.to_path_buf(),
            start_line: (start + 1) as u32,
            end_line: end as u32,
            content: body,
        });
    };

    const MIN_CHUNK_LINES: usize = 24;
    let mut seg_start = boundaries[0];
    for (i, &seg_end) in boundaries.iter().enumerate().skip(1) {
        let is_last = i == boundaries.len() - 1;
        if seg_end <= seg_start {
            continue;
        }
        if seg_end - seg_start < MIN_CHUNK_LINES && !is_last {
            continue;
        }
        let mut start = seg_start;
        while start < seg_end {
            let end = (start + VECTOR_SEED_CHUNK_LINES).min(seg_end);
            emit(start, end, &mut out);
            start = end;
        }
        seg_start = seg_end;
    }
    out
}

fn collect_vector_seed_files(root: &Path) -> Vec<PathBuf> {
    let ignore = crate::code_intel::search::build_gitignore_set(root);
    let is_ignored = |path: &Path| -> bool {
        ignore
            .as_ref()
            .is_some_and(|set| crate::code_intel::search::path_is_gitignored(set, root, path))
    };
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(it) => it,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if crate::util::is_index_skip_dir(name) {
                    continue;
                }
            }
            if is_ignored(&path) {
                continue;
            }
            match entry.file_type() {
                Ok(ft) if ft.is_dir() => stack.push(path),
                Ok(ft) if ft.is_file() => {
                    if is_seedable_source_file(&path) {
                        if let Ok(meta) = path.metadata() {
                            if meta.len() <= VECTOR_SEED_MAX_FILE_BYTES {
                                files.push(path);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    files
}

fn vector_snapshot_dir(root: &Path) -> PathBuf {
    root.join(".sen").join("rag")
}

const VECTOR_SNAPSHOT_MIN_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

static VECTOR_SNAPSHOT_LAST_SAVE: OnceLock<
    parking_lot::Mutex<std::collections::HashMap<PathBuf, std::time::Instant>>,
> = OnceLock::new();

fn vector_snapshot_save_due(root: &Path) -> bool {
    let map = VECTOR_SNAPSHOT_LAST_SAVE
        .get_or_init(|| parking_lot::Mutex::new(std::collections::HashMap::new()));
    let mut guard = map.lock();
    let now = std::time::Instant::now();
    match guard.get(root) {
        Some(last) if now.duration_since(*last) < VECTOR_SNAPSHOT_MIN_INTERVAL => false,
        _ => {
            guard.insert(root.to_path_buf(), now);
            true
        }
    }
}

fn spawn_vector_index_seed(root: PathBuf, fingerprint: u64, index: SharedVectorCodeIndex) {
    {
        let Ok(mut started) = vector_seed_started().write() else {
            return;
        };
        if !started.insert((root.clone(), fingerprint)) {
            return;
        }
    }
    crate::runtime::spawn_supervised("rag.vector_seed", async move {
        let seed_lock = vector_index_lock(&root);
        let _seed_guard = seed_lock.lock().await;
        let snapshot_dir = vector_snapshot_dir(&root);
        match index.load_snapshot(&snapshot_dir).await {
            Ok(restored) if restored > 0 => {
                tracing::info!(
                    target: "rag.code_index",
                    restored,
                    root = %root.display(),
                    "vector code index snapshot restored; unchanged chunks skip re-embedding"
                );
            }
            Ok(_) => {}
            Err(err) => {
                tracing::debug!(
                    target: "rag.code_index",
                    error = %err,
                    "vector snapshot unusable; seeding from scratch"
                );
            }
        }
        let walk_root = root.clone();
        let files = tokio::task::spawn_blocking(move || {
            let mut files = collect_vector_seed_files(&walk_root);
            let mtimes: std::collections::HashMap<
                std::path::PathBuf,
                std::time::SystemTime,
            > = files
                .iter()
                .map(|file| {
                    let modified = std::fs::metadata(file)
                        .and_then(|metadata| metadata.modified())
                        .unwrap_or(std::time::UNIX_EPOCH);
                    (file.clone(), modified)
                })
                .collect();
            files.sort_by(|left, right| {
                mtimes
                    .get(right)
                    .cmp(&mtimes.get(left))
                    .then_with(|| left.cmp(right))
            });
            files
        })
        .await
        .unwrap_or_default();
        let total_seed_files = files.len();
        let chunk_cap = adaptive_vector_chunk_cap(total_seed_files);
        remember_vector_chunk_cap(&root, chunk_cap);
        let mut total_chunks = 0usize;
        let mut reused_chunks = 0usize;
        let mut embedded_chunks = 0usize;
        let mut cap_truncated = false;
        let mut files_indexed = 0usize;
        let mut batch: Vec<crate::rag::vector_code_index::CodeChunk> = Vec::new();
        let mut retained_paths = std::collections::HashSet::new();
        for file in files {
            if total_chunks >= chunk_cap {
                cap_truncated = true;
                break;
            }
            files_indexed += 1;
            let read_path = file.clone();
            let chunk_root = root.clone();
            let chunks = match tokio::task::spawn_blocking(move || {
                let content = std::fs::read_to_string(&read_path)?;
                Ok::<_, std::io::Error>(chunk_source_for_vector_index(
                    &chunk_root,
                    &read_path,
                    &content,
                ))
            })
            .await
            {
                Ok(Ok(chunks)) => chunks,
                _ => continue,
            };
            let mut chunks = chunks;
            let remaining = chunk_cap.saturating_sub(total_chunks);
            if chunks.len() > remaining {
                chunks.truncate(remaining);
                cap_truncated = true;
            }
            retained_paths.insert(file.clone());
            let current_ids: std::collections::HashSet<String> =
                chunks.iter().map(|c| c.id.clone()).collect();
            let stale: Vec<String> = index
                .chunk_ids_for_path(&file)
                .await
                .into_iter()
                .filter(|id| !current_ids.contains(id))
                .collect();
            index.remove_ids(&stale).await;
            for chunk in chunks {
                total_chunks += 1;
                if index.contains_same_content(&chunk.id, &chunk.content).await {
                    reused_chunks += 1;
                    continue;
                }
                embedded_chunks += 1;
                batch.push(chunk);
                if batch.len() >= VECTOR_SEED_EMBED_BATCH {
                    if let Err(err) = index.upsert_chunks(std::mem::take(&mut batch)).await {
                        tracing::warn!(
                            target: "rag.code_index",
                            error = %err,
                            root = %root.display(),
                            "vector index seeding aborted; seeding will be retried on the \
                             next retrieval access"
                        );
                        clear_vector_seed_marker(&root, fingerprint);
                        return;
                    }
                }
            }
        }
        if !batch.is_empty() {
            if let Err(err) = index.upsert_chunks(batch).await {
                tracing::warn!(
                    target: "rag.code_index",
                    error = %err,
                    root = %root.display(),
                    "vector index seeding aborted; seeding will be retried on the \
                     next retrieval access"
                );
                clear_vector_seed_marker(&root, fingerprint);
                return;
            }
        }
        let mut ghost_paths = 0usize;
        for stale_path in index.indexed_paths().await {
            if !retained_paths.contains(&stale_path)
                || !stale_path.exists()
                || !is_seedable_source_file(&stale_path)
            {
                index.remove_path(&stale_path).await;
                ghost_paths += 1;
            }
        }
        if ghost_paths > 0 {
            tracing::info!(
                target: "rag.code_index",
                ghost_paths,
                "removed vector chunks for files that no longer exist on disk"
            );
        }
        if let Err(err) = index.save_snapshot(&snapshot_dir).await {
            tracing::debug!(
                target: "rag.code_index",
                error = %err,
                "vector snapshot persist failed; cold start will re-embed"
            );
        }
        if cap_truncated {
            tracing::warn!(
                target: "rag.code_index",
                chunk_cap,
                files_indexed,
                total_seed_files,
                root = %root.display(),
                "vector code index hit the chunk cap; only the most-recently-modified \
                 files were embedded. Dense retrieval covers a subset of this repo; \
                 raise the cap or rely on lexical/symbol search for the remainder"
            );
        }
        tracing::info!(
            target: "rag.code_index",
            chunks = total_chunks,
            reused = reused_chunks,
            embedded = embedded_chunks,
            files_indexed,
            total_seed_files,
            root = %root.display(),
            "vector code index seeding complete"
        );
    });
}

fn vector_embedder_fingerprint(
    backend: &str,
    model: &str,
    endpoint: Option<&str>,
    api_key: Option<&str>,
    dims: usize,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    backend.hash(&mut hasher);
    model.hash(&mut hasher);
    endpoint.hash(&mut hasher);
    api_key.hash(&mut hasher);
    dims.hash(&mut hasher);
    hasher.finish()
}

fn build_vector_index_for(root: &Path) -> Option<SharedVectorCodeIndex> {
    use crate::config::domain::CodeRagEmbedderConfig;
    use crate::rag::embedding::{
        CodeEmbedderBackend, CodeEmbedderConfig, build_code_embedder,
    };
    use crate::rag::vector_code_index::{VectorCodeIndex, VectorCodeIndexConfig};

    let services = crate::services::try_get_services()?;
    let cfg_arc = services.shared_config.load();
    if !cfg_arc.code_rag.enabled || !cfg_arc.code_rag.dense_enabled {
        return None;
    }
    let CodeRagEmbedderConfig {
        backend,
        model,
        endpoint,
        api_key,
        dims,
    } = cfg_arc.code_rag.embedder.clone();
    let main_provider_api_key = cfg_arc
        .api_key
        .clone()
        .filter(|k| !k.trim().is_empty());
    drop(cfg_arc);

    const GEMINI_OPENAI_COMPAT_EMBEDDINGS_URL: &str =
        "https://generativelanguage.googleapis.com/v1beta/openai";

    let normalized_backend = backend.to_ascii_lowercase();
    let (backend, default_endpoint) = match normalized_backend.as_str() {
        "openai" => (CodeEmbedderBackend::OpenAi, None),
        "gemini" => (
            CodeEmbedderBackend::OpenAi,
            Some(GEMINI_OPENAI_COMPAT_EMBEDDINGS_URL.to_string()),
        ),
        "openai_compatible" | "openai-compatible" | "compatible" => {
            if endpoint.as_deref().map(str::trim).filter(|e| !e.is_empty()).is_none() {
                tracing::warn!(
                    target: "rag.code_index",
                    "code_rag.embedder.backend 'openai_compatible' requires code_rag.embedder.endpoint; dense retrieval disabled"
                );
                return None;
            }
            (CodeEmbedderBackend::OpenAi, None)
        }
        "ollama" => (CodeEmbedderBackend::Ollama, None),
        "local_bge" | "localbge" | "bge" => {
            tracing::warn!(
                target: "rag.code_index",
                "code_rag.embedder.backend 'local_bge' is not implemented; dense retrieval \
                 disabled - use 'openai', 'gemini', 'openai_compatible', or 'ollama' to enable \
                 vector retrieval (falling back to lexical-only)"
            );
            return None;
        }
        other => {
            tracing::warn!(
                target: "rag.code_index",
                backend = other,
                "unknown code_rag.embedder.backend; dense retrieval disabled"
            );
            return None;
        }
    };

    let api_key = if backend == CodeEmbedderBackend::OpenAi {
        api_key
            .filter(|k| !k.trim().is_empty())
            .or(main_provider_api_key)
    } else {
        api_key
    };

    let fingerprint = vector_embedder_fingerprint(
        &normalized_backend,
        &model,
        endpoint.as_deref(),
        api_key.as_deref(),
        dims,
    );
    if let Ok(guard) = code_rag_vector_cache().read() {
        if let Some((cached_fingerprint, existing)) = guard.get(root) {
            if *cached_fingerprint == fingerprint {
                let existing = existing.clone();
                drop(guard);
                spawn_vector_index_seed(root.to_path_buf(), fingerprint, existing.clone());
                return Some(existing);
            }
        }
    }

    let mut embedder_cfg = match backend {
        CodeEmbedderBackend::OpenAi => {
            let key = match api_key
                .as_deref()
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .map(str::to_string)
            {
                Some(key) => key,
                None if matches!(
                    normalized_backend.as_str(),
                    "openai_compatible" | "openai-compatible" | "compatible"
                ) =>
                {
                    "sk-no-key".to_string()
                }
                None => {
                    tracing::warn!(
                        target: "rag.code_index",
                        backend = %normalized_backend,
                        "code_rag semantic (dense) retrieval is unavailable: no API key configured \
                         for the embedder; set code_rag.embedder.api_key (or the main provider \
                         api_key for the 'openai' and 'gemini' backends) to enable it - falling \
                         back to lexical-only retrieval"
                    );
                    return None;
                }
            };
            CodeEmbedderConfig::openai(model, dims, key)
        }
        CodeEmbedderBackend::Ollama => CodeEmbedderConfig::ollama(model, dims),
        CodeEmbedderBackend::LocalBge => CodeEmbedderConfig::local_bge(model, dims),
    };
    if let Some(url) = endpoint
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty())
        .or(default_endpoint)
    {
        embedder_cfg = embedder_cfg.with_endpoint(url);
    }
    let embedder = build_code_embedder(&embedder_cfg);

    let index = VectorCodeIndex::new(embedder, VectorCodeIndexConfig::default());
    let arc: SharedVectorCodeIndex = Arc::new(index);
    let arc = if let Ok(mut guard) = code_rag_vector_cache().write() {
        if let Some((cached_fingerprint, existing)) = guard.get(root) {
            if *cached_fingerprint == fingerprint {
                let existing = existing.clone();
                drop(guard);
                spawn_vector_index_seed(root.to_path_buf(), fingerprint, existing.clone());
                return Some(existing);
            }
        }
        guard.insert(root.to_path_buf(), (fingerprint, arc.clone()));
        arc
    } else {
        arc
    };
    spawn_vector_index_seed(root.to_path_buf(), fingerprint, arc.clone());
    Some(arc)
}

fn cached_vector_index(root: &Path) -> Option<SharedVectorCodeIndex> {
    build_vector_index_for(root)
}

#[must_use]
pub fn lsp_context_source() -> Option<Arc<dyn LspContextSource>> {
    Some(Arc::new(StaticLspContextSource))
}

pub enum SymbolGraphSourceState {
    Ready(Arc<dyn SymbolGraphLookup>),
    Building,
    Unavailable,
}

#[must_use]
pub fn symbol_graph_source_state(workspace_root: &Path) -> SymbolGraphSourceState {
    use crate::code_intel::symbol_graph::incremental::WriterAvailability;
    let root = workspace_root.to_path_buf();
    match crate::code_intel::symbol_graph::incremental::get_writer_nonblocking(&root) {
        WriterAvailability::Ready(writer) => {
            SymbolGraphSourceState::Ready(Arc::new(SymbolGraphAdapter {
                writer,
                workspace_root: root,
                per_file_cap: 12,
                dependents_cap: 5,
                repo_map_cache: parking_lot::Mutex::new(None),
            }))
        }
        WriterAvailability::Building => SymbolGraphSourceState::Building,
        WriterAvailability::Unavailable => SymbolGraphSourceState::Unavailable,
    }
}

#[must_use]
pub fn symbol_graph_source(workspace_root: &Path) -> Option<Arc<dyn SymbolGraphLookup>> {
    match symbol_graph_source_state(workspace_root) {
        SymbolGraphSourceState::Ready(lookup) => Some(lookup),
        _ => None,
    }
}

#[must_use]
pub fn rag_source(workspace_root: &Path) -> Option<Arc<dyn RagSource>> {
    let root = workspace_root.to_path_buf();
    if let Some(svc) = crate::services::try_get_services() {
        let cfg = svc.shared_config.load();
        if !cfg.code_rag.enabled {
            return None;
        }
    }
    let index: Arc<dyn IncrementalIndex> = {
        let cached = code_rag_cache()
            .read()
            .ok()
            .and_then(|guard| guard.get(&root).cloned());
        if let Some(idx) = cached {
            idx
        } else {
            let fresh: Arc<dyn IncrementalIndex> = Arc::new(HeuristicSearch::new(root.clone()));
            if let Ok(mut guard) = code_rag_cache().write() {
                guard.insert(root.clone(), fresh.clone());
            }
            fresh
        }
    };
    let vector_index = cached_vector_index(&root);
    let top_k_default = crate::services::try_get_services()
        .map(|svc| svc.shared_config.load().code_rag.top_k.max(1))
        .unwrap_or(5);
    Some(Arc::new(CodeRagSource {
        index,
        vector_index,
        top_k_default,
    }))
}

pub fn invalidate_caches() {
    if let Ok(mut guard) = code_rag_cache().write() {
        guard.clear();
    }
    if let Ok(mut guard) = code_rag_vector_cache().write() {
        guard.clear();
    }
    if let Ok(mut guard) = vector_seed_started().write() {
        guard.clear();
    }
}

pub fn note_lexical_watcher_alive(root: &Path) {
    if let Ok(guard) = code_rag_cache().read() {
        if let Some(index) = guard.get(root) {
            index.mark_walk_fresh();
        }
    }
}

async fn enforce_vector_chunk_cap(
    root: &Path,
    index: &SharedVectorCodeIndex,
) {
    let cap = vector_chunk_cap(root);
    if index.len().await <= cap {
        return;
    }
    let paths = index.indexed_paths().await;
    let by_age = tokio::task::spawn_blocking(move || {
        let mut paths: Vec<(std::time::SystemTime, PathBuf)> = paths
            .into_iter()
            .map(|path| {
                let modified = std::fs::metadata(&path)
                    .and_then(|metadata| metadata.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                (modified, path)
            })
            .collect();
        paths.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
        });
        paths
    })
    .await
    .unwrap_or_default();
    for (_, path) in by_age {
        if index.len().await <= cap {
            break;
        }
        index.remove_path(&path).await;
    }
}

pub fn note_code_files_changed(paths: &[PathBuf]) {
    if paths.is_empty() {
        return;
    }
    let paths: Vec<PathBuf> = paths.to_vec();

    let lexical_indexes: Vec<(PathBuf, Arc<dyn IncrementalIndex>)> = code_rag_cache()
        .read()
        .ok()
        .map(|guard| {
            guard
                .iter()
                .map(|(root, index)| (root.clone(), index.clone()))
                .collect()
        })
        .unwrap_or_default();
    if !lexical_indexes.is_empty() {
        let lexical_paths = paths.clone();
        crate::runtime::spawn_supervised("rag.lexical_incremental_update", async move {
            let _ = tokio::task::spawn_blocking(move || {
                for (root, index) in lexical_indexes {
                    for path in &lexical_paths {
                        if !path.starts_with(&root) {
                            continue;
                        }
                        if path.exists() {
                            let _ = index.reindex_file(path);
                        } else {
                            let _ = index.remove_file(path);
                        }
                    }
                }
            })
            .await;
        });
    }

    let vector_indexes: Vec<(PathBuf, SharedVectorCodeIndex)> = code_rag_vector_cache()
        .read()
        .ok()
        .map(|g| {
            g.iter()
                .map(|(root, (_, idx))| (root.clone(), idx.clone()))
                .collect()
        })
        .unwrap_or_default();
    if vector_indexes.is_empty() {
        return;
    }
    crate::runtime::spawn_supervised("rag.incremental_update", async move {
        for (root, index) in vector_indexes {
            let lock = vector_index_lock(&root);
            let _guard = lock.lock().await;
            let ignore_root = root.clone();
            let ignore = tokio::task::spawn_blocking(move || {
                crate::code_intel::search::build_gitignore_set(&ignore_root)
            })
            .await
            .ok()
            .flatten();
            let mut touched = false;
            for p in &paths {
                if !p.starts_with(&root) {
                    continue;
                }
                let ignored = ignore.as_ref().is_some_and(|set| {
                    crate::code_intel::search::path_is_gitignored(set, &root, p)
                });
                if !p.exists() || !is_seedable_source_file(p) || ignored {
                    index.remove_path(p).await;
                    touched = true;
                    continue;
                }
                if let Ok(meta) = p.metadata() {
                    if meta.len() > VECTOR_SEED_MAX_FILE_BYTES {
                        index.remove_path(p).await;
                        touched = true;
                        continue;
                    }
                }
                let read_path = p.clone();
                let chunk_root = root.clone();
                let chunks = match tokio::task::spawn_blocking(move || {
                    let content = std::fs::read_to_string(&read_path)?;
                    Ok::<_, std::io::Error>(chunk_source_for_vector_index(
                        &chunk_root,
                        &read_path,
                        &content,
                    ))
                })
                .await
                {
                    Ok(Ok(chunks)) => chunks,
                    _ => continue,
                };
                let current_ids: std::collections::HashSet<String> =
                    chunks.iter().map(|c| c.id.clone()).collect();
                let stale: Vec<String> = index
                    .chunk_ids_for_path(p)
                    .await
                    .into_iter()
                    .filter(|id| !current_ids.contains(id))
                    .collect();
                let mut to_embed: Vec<crate::rag::vector_code_index::CodeChunk> = Vec::new();
                for chunk in chunks {
                    if index.contains_same_content(&chunk.id, &chunk.content).await {
                        continue;
                    }
                    to_embed.push(chunk);
                }
                if !stale.is_empty() {
                    index.remove_ids(&stale).await;
                    touched = true;
                }
                if let Err(err) = index.upsert_chunks(to_embed).await {
                    tracing::warn!(
                        target: "rag.code_index",
                        path = %p.display(),
                        error = %err,
                        "incremental vector upsert failed for this file; continuing with \
                         remaining files (missing chunks will re-embed on the next change)"
                    );
                    continue;
                }
                touched = true;
            }
            enforce_vector_chunk_cap(&root, &index).await;
            if touched && vector_snapshot_save_due(&root) {
                if let Err(err) = index.save_snapshot(&vector_snapshot_dir(&root)).await {
                    tracing::debug!(
                        target: "rag.code_index",
                        error = %err,
                        "vector snapshot persist failed after incremental update"
                    );
                }
            }
        }
    });
}

fn collect_dependents(graph: &SymbolGraph, sym: &SymbolId, cap: usize) -> Vec<String> {
    let mut out = Vec::new();
    for edge in graph.in_edges(sym) {
        if out.len() >= cap {
            break;
        }
        if !matches!(edge.kind, EdgeKind::Calls | EdgeKind::Implements) {
            continue;
        }
        if out.iter().any(|n: &String| n == &edge.from.name) {
            continue;
        }
        out.push(edge.from.name.clone());
    }
    out
}

fn collect_file_imports(graph: &SymbolGraph, rel: &Path, cap: usize) -> Vec<String> {
    let anchor = SymbolId::file_anchor(rel.to_path_buf());
    let mut out: Vec<String> = Vec::new();
    for edge in graph.out_edges(&anchor) {
        if out.len() >= cap {
            break;
        }
        if !matches!(edge.kind, EdgeKind::Imports) {
            continue;
        }
        let label = edge
            .to
            .file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| edge.to.file.to_string_lossy().into_owned());
        if label.is_empty() || out.iter().any(|n| n == &label) {
            continue;
        }
        out.push(label);
    }
    out
}

fn relativize_to_workspace(path: &Path, root: &Path) -> PathBuf {
    if path.is_absolute() {
        match path.strip_prefix(root) {
            Ok(rel) => rel.to_path_buf(),
            Err(_) => path.to_path_buf(),
        }
    } else {
        path.to_path_buf()
    }
}

fn rebuild_display_path(root: &Path, rel: &Path) -> PathBuf {
    if rel.is_absolute() {
        rel.to_path_buf()
    } else {
        root.join(rel)
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    let a_str = a.to_string_lossy().replace('\\', "/");
    let b_str = b.to_string_lossy().replace('\\', "/");
    if a_str == b_str {
        return true;
    }
    let suffix_matches = |long: &str, short: &str| -> bool {
        long.strip_suffix(short)
            .is_some_and(|prefix| prefix.is_empty() || prefix.ends_with('/'))
    };
    suffix_matches(&a_str, &b_str) || suffix_matches(&b_str, &a_str)
}
