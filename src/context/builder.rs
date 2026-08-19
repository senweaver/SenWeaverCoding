// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;

use super::git::GitContext;
use super::lsp_ctx::LspSnapshot;
use super::open_files::{NoOpenFilesSource, OpenFile, OpenFilesSource};
use super::outline_ctx::OutlineNode;
use super::rag_ctx::SearchHit;
use super::symbols_ctx::SymbolSnapshot;

pub trait SymbolGraphLookup: Send + Sync {

    fn snapshot_for_focus(&self, paths: &[PathBuf], query: Option<&str>) -> Vec<SymbolSnapshot>;

    fn repo_map(&self, _max_files: usize, _max_symbols_per_file: usize, _max_chars: usize) -> String {
        String::new()
    }
}

#[async_trait::async_trait]
pub trait LspContextSource: Send + Sync {
    async fn collect(&self, focus: &[PathBuf]) -> Vec<LspSnapshot>;
}

#[async_trait::async_trait]
pub trait RagSource: Send + Sync {
    async fn retrieve(&self, query: &str, top_k: usize) -> Vec<SearchHit>;
}

#[derive(Debug, Clone)]
pub struct QueryContext {
    pub git: Option<GitContext>,
    pub cwd: PathBuf,
    pub date: String,
    pub additional_instructions: Vec<String>,

    pub open_files: Vec<OpenFile>,
    pub focus_files: Vec<PathBuf>,
    pub symbols: Vec<SymbolSnapshot>,
    pub outline: Vec<OutlineNode>,
    pub lsp_info: Vec<LspSnapshot>,
    pub rag_hits: Vec<SearchHit>,

    pub repo_map: String,

    pub symbol_graph_building: bool,
}

impl QueryContext {

    #[must_use]
    pub fn render_injection_block(&self) -> String {
        if self.open_files.is_empty()
            && self.focus_files.is_empty()
            && self.symbols.is_empty()
            && self.outline.is_empty()
            && self.lsp_info.is_empty()
            && self.rag_hits.is_empty()
            && self.repo_map.is_empty()
            && self.git.is_none()
        {
            return String::new();
        }
        let mut out = String::from("[Query context]\n");
        if self.symbol_graph_building {
            out.push_str("symbol_graph: building (first workspace index in progress)\n");
        }
        if !self.repo_map.is_empty() {
            out.push_str(&self.repo_map);
        }
        if let Some(ref git) = self.git {
            let branch_line = match git.default_branch.as_deref() {
                Some(def) if !def.is_empty() && def != git.branch => {
                    format!("git branch: {} (default: {def})\n", git.branch)
                }
                _ => format!("git branch: {}\n", git.branch),
            };
            out.push_str(&branch_line);
            if git.is_dirty {
                let changed: Vec<&str> = git.status_short.lines().take(10).collect();
                let total = git.status_short.lines().count();
                out.push_str(&format!("git status ({total} changed):\n"));
                for line in &changed {
                    out.push_str(&format!("  {line}\n"));
                }
                if total > changed.len() {
                    out.push_str(&format!("  ... {} more\n", total - changed.len()));
                }
            }
            if !git.recent_log.is_empty() {
                let commits: Vec<&str> = git.recent_log.lines().take(5).collect();
                out.push_str("recent commits:\n");
                for line in &commits {
                    out.push_str(&format!("  {line}\n"));
                }
            }
        }
        if !self.focus_files.is_empty() {
            out.push_str("focus_files:\n");
            for p in &self.focus_files {
                out.push_str(&format!("- {}\n", p.display()));
            }
        }
        if !self.open_files.is_empty() {
            out.push_str("open_files:\n");
            for f in &self.open_files {
                out.push_str(&format!("- {}\n", f.path.display()));
            }
        }
        if !self.symbols.is_empty() {
            out.push_str("symbols:\n");
            for s in &self.symbols {
                if s.line_end > s.line {
                    out.push_str(&format!(
                        "- {} ({}) @ {}:{}-{}",
                        s.name,
                        s.kind,
                        s.path.display(),
                        s.line,
                        s.line_end
                    ));
                } else {
                    out.push_str(&format!(
                        "- {} ({}) @ {}:{}",
                        s.name,
                        s.kind,
                        s.path.display(),
                        s.line
                    ));
                }
                if let Some(ref sig) = s.signature {
                    let short = if sig.len() > 120 {
                        format!("{}…", crate::util::truncate_str_bytes(sig, 120))
                    } else {
                        sig.clone()
                    };
                    out.push_str(&format!(" | {short}"));
                }
                if !s.dependents.is_empty() {
                    let deps: Vec<&str> = s.dependents.iter().take(4).map(String::as_str).collect();
                    out.push_str(&format!(" | deps: {}", deps.join(", ")));
                }
                if !s.imports.is_empty() {
                    let imports: Vec<&str> =
                        s.imports.iter().take(6).map(String::as_str).collect();
                    out.push_str(&format!(" | imports: {}", imports.join(", ")));
                }
                out.push('\n');
            }
        }
        if !self.outline.is_empty() {
            out.push_str("outline:\n");
            for o in &self.outline {
                out.push_str(&format!(
                    "- {} {} @ {}:{}\n",
                    o.kind,
                    o.name,
                    o.path.display(),
                    o.line
                ));
            }
        }
        if !self.lsp_info.is_empty() {
            out.push_str("lsp:\n");
            for l in &self.lsp_info {
                let hover_part = l
                    .hover
                    .as_deref()
                    .map(|h| format!(" | {h}"))
                    .unwrap_or_default();
                out.push_str(&format!(
                    "- {} [{} diag] {}{}\n",
                    l.path.display(),
                    l.diagnostics,
                    l.summary,
                    hover_part
                ));
            }
        }
        if !self.rag_hits.is_empty() {
            out.push_str("rag_hits:\n");
            let rag_section_max_bytes =
                crate::agent::token::budget::InjectionBudget::current().rag_section_bytes;
            let section_start = out.len();
            for h in &self.rag_hits {
                if out.len().saturating_sub(section_start) >= rag_section_max_bytes {
                    out.push_str("- … (more hits elided)\n");
                    break;
                }
                match h.end_line {
                    Some(end) if h.snippet.contains('\n') => {
                        out.push_str(&format!(
                            "- {}:{}-{}\n",
                            h.path.display(),
                            h.line,
                            end
                        ));
                        for line in h.snippet.lines() {
                            out.push_str("    ");
                            out.push_str(line);
                            out.push('\n');
                        }
                    }
                    _ => {
                        out.push_str(&format!(
                            "- {}:{} {}\n",
                            h.path.display(),
                            h.line,
                            h.snippet.replace('\n', " ")
                        ));
                    }
                }
            }
        }
        out.push_str("[/Query context]\n");
        out
    }
}

pub struct ContextBuilder {
    cwd: PathBuf,

    open_files_source: Option<Arc<dyn OpenFilesSource>>,
    focus_files: Vec<PathBuf>,
    symbol_graph: Option<Arc<dyn SymbolGraphLookup>>,
    symbol_graph_building: bool,
    lsp_source: Option<Arc<dyn LspContextSource>>,
    rag_source: Option<Arc<dyn RagSource>>,
    rag_query: Option<String>,
    rag_top_k: usize,

    lsp_timeout: Duration,
}

impl ContextBuilder {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            open_files_source: None,
            focus_files: Vec::new(),
            symbol_graph: None,
            symbol_graph_building: false,
            lsp_source: None,
            rag_source: None,
            rag_query: None,
            rag_top_k: 5,
            lsp_timeout: Duration::from_secs(3),
        }
    }

    #[must_use]
    pub fn with_open_files_source(mut self, src: Arc<dyn OpenFilesSource>) -> Self {
        self.open_files_source = Some(src);
        self
    }

    #[must_use]
    pub fn with_focus_files(mut self, paths: Vec<PathBuf>) -> Self {
        FocusPathRegistry::push(&paths);
        self.focus_files = paths;
        self
    }

    #[must_use]
    pub fn with_symbol_graph(mut self, graph: Arc<dyn SymbolGraphLookup>) -> Self {
        self.symbol_graph = Some(graph);
        self
    }

    #[must_use]
    pub fn with_symbol_graph_building(mut self, building: bool) -> Self {
        self.symbol_graph_building = building;
        self
    }

    #[must_use]
    pub fn with_lsp(mut self, lsp: Arc<dyn LspContextSource>) -> Self {
        self.lsp_source = Some(lsp);
        self
    }

    #[must_use]
    pub fn with_lsp_timeout(mut self, timeout: Duration) -> Self {
        self.lsp_timeout = timeout;
        self
    }

    #[must_use]
    pub fn with_rag(mut self, rag: Arc<dyn RagSource>, query: impl Into<String>) -> Self {
        self.rag_source = Some(rag);
        self.rag_query = Some(query.into());
        self
    }

    #[must_use]
    pub fn with_rag_top_k(mut self, top_k: usize) -> Self {
        self.rag_top_k = top_k;
        self
    }

    pub async fn build(&self) -> anyhow::Result<QueryContext> {

        let git_fut = GitContext::gather(&self.cwd);

        let focus_for_outline = self.focus_files.clone();
        let outline_query = self.rag_query.clone();
        let outline_fut = async move {
            tokio::task::spawn_blocking(move || {
                collect_outline_for_focus(&focus_for_outline, outline_query.as_deref())
            })
            .await
            .unwrap_or_default()
        };

        let symbol_graph_ref = self.symbol_graph.clone();
        let focus_for_symbols = self.focus_files.clone();
        let symbols_query = self.rag_query.clone();
        let symbols_fut = async move {
            match symbol_graph_ref {
                Some(g) => tokio::task::spawn_blocking(move || {
                    g.snapshot_for_focus(&focus_for_symbols, symbols_query.as_deref())
                })
                .await
                .unwrap_or_default(),
                None => Vec::new(),
            }
        };

        let repo_map_graph = self.symbol_graph.clone();
        let repo_map_needed = self.focus_files.is_empty();
        let repo_map_fut = async move {
            match (repo_map_graph, repo_map_needed) {
                (Some(g), true) => tokio::task::spawn_blocking(move || {
                    let repo_map_bytes =
                        crate::agent::token::budget::InjectionBudget::current().repo_map_bytes;
                    g.repo_map(12, 6, repo_map_bytes)
                })
                .await
                .unwrap_or_default(),
                _ => String::new(),
            }
        };

        let open_files_fut = async {
            match self.open_files_source.as_ref() {
                Some(src) => src.list().await,
                None => Vec::new(),
            }
        };

        let focus_for_lsp = self.focus_files.clone();
        let lsp_source = self.lsp_source.clone();
        let lsp_timeout = self.lsp_timeout;
        let lsp_fut = async move {
            match lsp_source {
                Some(src) => {
                    match tokio::time::timeout(lsp_timeout, src.collect(&focus_for_lsp)).await {
                        Ok(v) => {
                            crate::observability::code_intel_metrics::incr_lsp_hover_fetched();
                            v
                        }
                        Err(_) => {
                            crate::observability::code_intel_metrics::incr_lsp_hover_timeout();
                            Vec::new()
                        }
                    }
                }
                None => Vec::new(),
            }
        };

        let rag_source = self.rag_source.clone();
        let rag_query = self.rag_query.clone();
        let rag_top_k = self.rag_top_k;
        let rag_fut = async move {
            match (rag_source, rag_query) {
                (Some(rag), Some(q)) => {
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(8),
                        rag.retrieve(&q, rag_top_k),
                    )
                    .await
                    {
                        Ok(hits) => hits,
                        Err(_) => {
                            tracing::warn!(
                                "RAG retrieval timed out after 8s; continuing without RAG context"
                            );
                            Vec::new()
                        }
                    }
                }
                _ => Vec::new(),
            }
        };

        let (git_res, outline, symbols, open_files, lsp_info, rag_hits, repo_map) = tokio::join!(
            git_fut,
            outline_fut,
            symbols_fut,
            open_files_fut,
            lsp_fut,
            rag_fut,
            repo_map_fut
        );
        let git = git_res.ok();

        let date = chrono::Local::now().format("%Y-%m-%d").to_string();

        let ctx = QueryContext {
            git,
            cwd: self.cwd.clone(),
            date,
            additional_instructions: Vec::new(),
            open_files,
            focus_files: self.focus_files.clone(),
            symbols,
            outline,
            lsp_info,
            rag_hits,
            repo_map,
            symbol_graph_building: self.symbol_graph_building,
        };

        crate::observability::code_intel_metrics::incr_context_build_success();
        if ctx.open_files.is_empty()
            && ctx.focus_files.is_empty()
            && ctx.symbols.is_empty()
            && ctx.outline.is_empty()
            && ctx.lsp_info.is_empty()
            && ctx.rag_hits.is_empty()
        {
            crate::observability::code_intel_metrics::incr_context_build_empty_sources();
        }

        Ok(ctx)
    }
}

#[must_use]
pub fn empty_open_files_source() -> Arc<dyn OpenFilesSource> {
    Arc::new(NoOpenFilesSource)
}

pub struct FocusPathRegistry;

struct FocusEntry {
    paths: Vec<PathBuf>,
    last_used: Instant,
}

static FOCUS_REGISTRY: Lazy<RwLock<std::collections::HashMap<String, FocusEntry>>> =
    Lazy::new(|| RwLock::new(std::collections::HashMap::new()));

const FOCUS_PATHS_PER_SESSION: usize = 8;
const FOCUS_MAX_SESSIONS: usize = 64;

impl FocusPathRegistry {
    fn session_key() -> Option<String> {
        crate::session::current_session_context().map(|c| c.session_id)
    }

    pub fn set(paths: Vec<PathBuf>) {
        let Some(key) = Self::session_key() else {
            return;
        };
        if let Ok(mut guard) = FOCUS_REGISTRY.write() {
            Self::evict_if_needed(&mut guard, &key);
            guard.insert(
                key,
                FocusEntry {
                    paths,
                    last_used: Instant::now(),
                },
            );
        }
    }

    pub fn push(paths: &[PathBuf]) {
        if paths.is_empty() {
            return;
        }
        let Some(key) = Self::session_key() else {
            return;
        };
        if let Ok(mut guard) = FOCUS_REGISTRY.write() {
            Self::evict_if_needed(&mut guard, &key);
            let entry = guard.entry(key).or_insert_with(|| FocusEntry {
                paths: Vec::new(),
                last_used: Instant::now(),
            });
            entry.last_used = Instant::now();
            for p in paths {
                if !entry.paths.contains(p) {
                    entry.paths.push(p.clone());
                }
            }
        }
    }

    pub fn note(paths: &[PathBuf]) {
        if paths.is_empty() {
            return;
        }
        let Some(key) = Self::session_key() else {
            return;
        };
        if let Ok(mut guard) = FOCUS_REGISTRY.write() {
            Self::evict_if_needed(&mut guard, &key);
            let entry = guard.entry(key).or_insert_with(|| FocusEntry {
                paths: Vec::new(),
                last_used: Instant::now(),
            });
            entry.last_used = Instant::now();
            for p in paths {
                entry.paths.retain(|existing| existing != p);
                entry.paths.insert(0, p.clone());
            }
            entry.paths.truncate(FOCUS_PATHS_PER_SESSION);
        }
    }

    fn evict_if_needed(
        guard: &mut std::collections::HashMap<String, FocusEntry>,
        incoming_key: &str,
    ) {
        if guard.len() >= FOCUS_MAX_SESSIONS && !guard.contains_key(incoming_key) {
            let victim = guard
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| key.clone());
            if let Some(victim) = victim {
                guard.remove(&victim);
            }
        }
    }

    #[must_use]
    pub fn current() -> Vec<PathBuf> {
        let Some(key) = Self::session_key() else {
            return Vec::new();
        };
        if let Ok(mut guard) = FOCUS_REGISTRY.write() {
            if let Some(entry) = guard.get_mut(&key) {
                entry.last_used = Instant::now();
                return entry.paths.clone();
            }
        }
        Vec::new()
    }

    pub fn clear() {
        let Some(key) = Self::session_key() else {
            return;
        };
        if let Ok(mut guard) = FOCUS_REGISTRY.write() {
            guard.remove(&key);
        }
    }
}

fn collect_outline_for_focus(focus: &[PathBuf], query: Option<&str>) -> Vec<OutlineNode> {
    const MAX_OUTLINE_NODES: usize = 80;
    let max_outline_bytes =
        crate::agent::token::budget::InjectionBudget::current().outline_bytes;
    let query_terms: Vec<String> = query
        .map(|q| {
            q.split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .filter(|t| t.len() >= 3)
                .map(|t| t.to_ascii_lowercase())
                .collect()
        })
        .unwrap_or_default();

    let mut scored: Vec<(u32, usize, OutlineNode)> = Vec::new();
    for path in focus {
        let entries = match crate::code_intel::outline::extract_outline(path, None) {
            Ok(v) => v,
            Err(_) => continue,
        };
        for entry in entries {
            let name_lc = entry.name.to_ascii_lowercase();
            let mut score = 0u32;
            for term in &query_terms {
                if name_lc == *term {
                    score += 2;
                } else if name_lc.contains(term.as_str()) {
                    score += 1;
                }
            }
            let approx = entry.kind.len() + entry.name.len() + 16;
            scored.push((
                score,
                approx,
                OutlineNode::leaf(path.clone(), entry.kind, entry.name, entry.line),
            ));
        }
    }

    if !query_terms.is_empty() {
        scored.sort_by(|a, b| b.0.cmp(&a.0));
    }

    let mut out = Vec::new();
    let mut used = 0usize;
    for (_, approx, node) in scored {
        if out.len() >= MAX_OUTLINE_NODES || used + approx > max_outline_bytes {
            break;
        }
        used += approx;
        out.push(node);
    }
    out
}
