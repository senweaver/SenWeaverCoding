// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use crate::code_intel::search::{IncrementalIndex, heuristic::Search as HeuristicSearch};
use crate::code_intel::symbol_graph::{EdgeKind, SymbolGraph, SymbolId};
use crate::context::builder::{LspContextSource, RagSource, SymbolGraphLookup};
use crate::context::lsp_ctx::LspSnapshot;
use crate::context::rag_ctx::RagHit;
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
    async fn retrieve(&self, query: &str, top_k: usize) -> Vec<RagHit> {
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
        let mut out: Vec<RagHit> = Vec::with_capacity(fused.len().min(limit));
        for ((path, line), _score) in fused.into_iter().take(limit) {
            if let Some(hit) = lexical.iter().find(|h| h.path == path && h.line == line) {
                out.push(hit.clone());
                continue;
            }
            if let Some(d) = dense
                .iter()
                .find(|d| d.path == path && d.start_line == line)
            {
                out.push(RagHit {
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

    let backend = match backend.to_ascii_lowercase().as_str() {
        "openai" => CodeEmbedderBackend::OpenAi,
        "ollama" => CodeEmbedderBackend::Ollama,
        "local_bge" | "localbge" | "bge" => CodeEmbedderBackend::LocalBge,
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
    if let Some(url) = endpoint {
        embedder_cfg = embedder_cfg.with_endpoint(url);
    }
    let embedder = build_code_embedder(&embedder_cfg);

    let index = VectorCodeIndex::new(embedder, VectorCodeIndexConfig::default());
    let arc: SharedVectorCodeIndex = Arc::new(index);
    if let Ok(mut guard) = code_rag_vector_cache().write() {
        guard.insert(root.to_path_buf(), arc.clone());
    }
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
    a_str == b_str || a_str.ends_with(&b_str) || b_str.ends_with(&a_str)
}
