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
        let cwd = std::env::current_dir().unwrap_or_default();
        let mut out = Vec::with_capacity(focus.len());
        for path in focus {
            let abs = if path.is_absolute() {
                path.clone()
            } else {
                cwd.join(path)
            };
            let diagnostics = all
                .get(&abs)
                .or_else(|| all.get(path))
                .cloned()
                .unwrap_or_default();
            let summary = diagnostics
                .iter()
                .take(3)
                .map(|d| d.message.clone())
                .collect::<Vec<_>>()
                .join("; ");
            let hover = services.lsp.hover(path, 0, 0).await;
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

struct SymbolGraphAdapter {
    graph: Arc<SymbolGraph>,
    workspace_root: PathBuf,
    per_file_cap: usize,
    dependents_cap: usize,
}

impl SymbolGraphLookup for SymbolGraphAdapter {
    fn snapshot_for_focus(&self, paths: &[PathBuf]) -> Vec<SymbolSnapshot> {
        if paths.is_empty() || self.graph.symbols.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for path in paths {
            let rel = relativize_to_workspace(path, &self.workspace_root);
            let mut for_file = 0usize;
            for sym in self.graph.symbols.iter() {
                if for_file >= self.per_file_cap {
                    break;
                }
                if !same_file(&sym.id.file, &rel) {
                    continue;
                }
                let dependents = collect_dependents(&self.graph, &sym.id, self.dependents_cap);
                let signature =
                    read_signature_line(&self.workspace_root, &sym.id.file, sym.id.line);
                out.push(SymbolSnapshot {
                    name: sym.id.name.clone(),
                    kind: sym.kind.clone(),
                    path: rebuild_display_path(&self.workspace_root, &sym.id.file),
                    line: sym.id.line,
                    signature,
                    dependents,
                });
                for_file += 1;
            }
        }
        out
    }
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
        let limit = top_k.max(1).min(self.top_k_default.max(top_k.max(1)));
        let lexical = {
            let index = self.index.clone();
            let q = query.to_string();
            let search_limit = limit.saturating_mul(2);
            tokio::task::spawn_blocking(move || {
                index.search(&q, search_limit).unwrap_or_default()
            })
            .await
            .unwrap_or_default()
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
            let mut out = lexical;
            out.truncate(limit);
            return out;
        }
        let lexical_keys: Vec<(PathBuf, u32)> =
            lexical.iter().map(|h| (h.path.clone(), h.line)).collect();
        let dense_keys: Vec<(PathBuf, u32)> = dense
            .iter()
            .map(|h| (h.path.clone(), h.start_line))
            .collect();
        let fused = reciprocal_rank_fusion(&[lexical_keys, dense_keys], 60);
        let mut out: Vec<SearchHit> = Vec::with_capacity(fused.len().min(limit));
        for ((path, line), _score) in fused.into_iter().take(limit) {
            if let Some(hit) = lexical.iter().find(|h| h.path == path && h.line == line) {
                out.push(hit.clone());
                continue;
            }
            if let Some(d) = dense
                .iter()
                .find(|d| d.path == path && d.start_line == line)
            {
                out.push(SearchHit {
                    path: d.path.clone(),
                    line: d.start_line,
                    snippet: d.snippet.clone(),
                });
            }
        }
        out
    }
}

static SYMBOL_GRAPH_CACHE: OnceLock<RwLock<std::collections::HashMap<PathBuf, Arc<SymbolGraph>>>> =
    OnceLock::new();

static CODE_RAG_CACHE: OnceLock<
    RwLock<std::collections::HashMap<PathBuf, Arc<dyn IncrementalIndex>>>,
> = OnceLock::new();

static CODE_RAG_VECTOR_CACHE: OnceLock<
    RwLock<std::collections::HashMap<PathBuf, SharedVectorCodeIndex>>,
> = OnceLock::new();

fn symbol_graph_cache() -> &'static RwLock<std::collections::HashMap<PathBuf, Arc<SymbolGraph>>> {
    SYMBOL_GRAPH_CACHE.get_or_init(|| {
        tracing::debug!(
            target: "agent.loop_services",
            cache = "symbol_graph",
            kind = "workspace-bucketed-global",
            reason = "context-builder call-sites are not handed an AgentLoopServices instance",
            "initialising workspace-bucketed global cache"
        );
        RwLock::new(std::collections::HashMap::new())
    })
}

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
) -> &'static RwLock<std::collections::HashMap<PathBuf, SharedVectorCodeIndex>> {
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

static VECTOR_SEED_STARTED: OnceLock<RwLock<std::collections::HashSet<PathBuf>>> =
    OnceLock::new();

fn vector_seed_started() -> &'static RwLock<std::collections::HashSet<PathBuf>> {
    VECTOR_SEED_STARTED.get_or_init(|| RwLock::new(std::collections::HashSet::new()))
}

const VECTOR_SEED_CHUNK_LINES: usize = 80;
const VECTOR_SEED_MAX_FILE_BYTES: u64 = 512 * 1024;
const VECTOR_SEED_MAX_CHUNKS: usize = 4000;
const VECTOR_SEED_EMBED_BATCH: usize = 16;

fn is_seedable_source_file(path: &Path) -> bool {
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
    let mut out = Vec::new();
    let mut start = 0usize;
    while start < lines.len() {
        let end = (start + VECTOR_SEED_CHUNK_LINES).min(lines.len());
        let body = lines[start..end].join("\n");
        if !body.trim().is_empty() {
            out.push(crate::rag::vector_code_index::CodeChunk {
                id: format!("{rel_display}#{}", start + 1),
                path: path.to_path_buf(),
                start_line: (start + 1) as u32,
                end_line: end as u32,
                content: body,
            });
        }
        start = end;
    }
    out
}

fn collect_vector_seed_files(root: &Path) -> Vec<PathBuf> {
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
                if name.starts_with('.')
                    || matches!(
                        name,
                        "target" | "node_modules" | "__pycache__" | "dist" | "build" | "vendor"
                    )
                {
                    continue;
                }
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

fn spawn_vector_index_seed(root: PathBuf, index: SharedVectorCodeIndex) {
    {
        let Ok(mut started) = vector_seed_started().write() else {
            return;
        };
        if !started.insert(root.clone()) {
            return;
        }
    }
    crate::runtime::spawn_supervised("rag.vector_seed", async move {
        let walk_root = root.clone();
        let files = tokio::task::spawn_blocking(move || collect_vector_seed_files(&walk_root))
            .await
            .unwrap_or_default();
        let mut total_chunks = 0usize;
        let mut batch: Vec<crate::rag::vector_code_index::CodeChunk> = Vec::new();
        for file in files {
            if total_chunks >= VECTOR_SEED_MAX_CHUNKS {
                break;
            }
            let read_path = file.clone();
            let content = match tokio::task::spawn_blocking(move || {
                std::fs::read_to_string(&read_path)
            })
            .await
            {
                Ok(Ok(s)) => s,
                _ => continue,
            };
            for chunk in chunk_source_for_vector_index(&root, &file, &content) {
                if total_chunks >= VECTOR_SEED_MAX_CHUNKS {
                    break;
                }
                batch.push(chunk);
                total_chunks += 1;
                if batch.len() >= VECTOR_SEED_EMBED_BATCH {
                    if let Err(err) = index.upsert_chunks(std::mem::take(&mut batch)).await {
                        tracing::warn!(
                            target: "rag.code_index",
                            error = %err,
                            "vector index seeding aborted (embedder unavailable)"
                        );
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
                    "vector index seeding aborted (embedder unavailable)"
                );
                return;
            }
        }
        tracing::info!(
            target: "rag.code_index",
            chunks = total_chunks,
            root = %root.display(),
            "vector code index seeding complete"
        );
    });
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
        "local_bge" | "localbge" | "bge" => (CodeEmbedderBackend::LocalBge, None),
        other => {
            tracing::warn!(
                target: "rag.code_index",
                backend = other,
                "unknown code_rag.embedder.backend; dense retrieval disabled"
            );
            return None;
        }
    };
    let mut embedder_cfg = match backend {
        CodeEmbedderBackend::OpenAi => CodeEmbedderConfig::openai(
            model,
            dims,
            api_key.unwrap_or_default(),
        ),
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
    if let Ok(mut guard) = code_rag_vector_cache().write() {
        guard.insert(root.to_path_buf(), arc.clone());
    }
    spawn_vector_index_seed(root.to_path_buf(), arc.clone());
    Some(arc)
}

fn cached_vector_index(root: &Path) -> Option<SharedVectorCodeIndex> {
    if let Ok(guard) = code_rag_vector_cache().read() {
        if let Some(idx) = guard.get(root) {
            return Some(idx.clone());
        }
    }
    build_vector_index_for(root)
}

#[must_use]
pub fn lsp_context_source() -> Option<Arc<dyn LspContextSource>> {
    Some(Arc::new(StaticLspContextSource))
}

#[must_use]
pub fn symbol_graph_source(workspace_root: &Path) -> Option<Arc<dyn SymbolGraphLookup>> {
    let root = workspace_root.to_path_buf();
    if let Ok(guard) = symbol_graph_cache().read() {
        if let Some(graph) = guard.get(&root) {
            return Some(Arc::new(SymbolGraphAdapter {
                graph: graph.clone(),
                workspace_root: root,
                per_file_cap: 12,
                dependents_cap: 5,
            }));
        }
    }

    let loaded = match SymbolGraph::load(&root) {
        Ok(Some(g)) => Some(g),
        Ok(None) => None,
        Err(_) => None,
    }?;
    let arc = Arc::new(loaded);
    if let Ok(mut guard) = symbol_graph_cache().write() {
        guard.insert(root.clone(), arc.clone());
    }
    Some(Arc::new(SymbolGraphAdapter {
        graph: arc,
        workspace_root: root,
        per_file_cap: 12,
        dependents_cap: 5,
    }))
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
    if let Ok(mut guard) = symbol_graph_cache().write() {
        guard.clear();
    }
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

fn collect_dependents(graph: &SymbolGraph, sym: &SymbolId, cap: usize) -> Vec<String> {
    let mut out = Vec::new();
    for edge in &graph.edges {
        if out.len() >= cap {
            break;
        }
        if edge.to != *sym {
            continue;
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

fn read_signature_line(root: &Path, rel: &Path, line: u32) -> Option<String> {
    if line == 0 {
        return None;
    }
    let abs = if rel.is_absolute() {
        rel.to_path_buf()
    } else {
        root.join(rel)
    };
    let body = std::fs::read_to_string(&abs).ok()?;
    let target = (line as usize).saturating_sub(1);
    body.lines().nth(target).map(|s| s.trim().to_string())
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
    // Suffix match must align on a path separator so `lib.rs` does not match
    // `b.rs` (which would attribute one file's symbols to another).
    let suffix_matches = |long: &str, short: &str| -> bool {
        long.strip_suffix(short)
            .is_some_and(|prefix| prefix.is_empty() || prefix.ends_with('/'))
    };
    suffix_matches(&a_str, &b_str) || suffix_matches(&b_str, &a_str)
}
