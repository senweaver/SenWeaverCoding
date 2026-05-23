// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use once_cell::sync::Lazy;

use super::git::GitContext;
use super::lsp_ctx::LspSnapshot;
use super::memory_files::MemoryFileContext;
use super::open_files::{NoOpenFilesSource, OpenFile, OpenFilesSource};
use super::outline_ctx::OutlineNode;
use super::rag_ctx::RagHit;
use super::symbols_ctx::SymbolSnapshot;
use super::system_prompt::SystemPromptParts;

pub trait SymbolGraphLookup: Send + Sync {

    fn snapshot_for_focus(&self, paths: &[PathBuf]) -> Vec<SymbolSnapshot>;
}

#[async_trait::async_trait]
pub trait LspContextSource: Send + Sync {
    async fn collect(&self, focus: &[PathBuf]) -> Vec<LspSnapshot>;
}

#[async_trait::async_trait]
pub trait RagSource: Send + Sync {
    async fn retrieve(&self, query: &str, top_k: usize) -> Vec<RagHit>;
}

#[derive(Debug, Clone)]
pub struct QueryContext {
    pub system_prompt: SystemPromptParts,
    pub git: Option<GitContext>,
    pub memory: MemoryFileContext,
    pub cwd: PathBuf,
    pub date: String,
    pub additional_instructions: Vec<String>,

    pub open_files: Vec<OpenFile>,
    pub focus_files: Vec<PathBuf>,
    pub symbols: Vec<SymbolSnapshot>,
    pub outline: Vec<OutlineNode>,
    pub lsp_info: Vec<LspSnapshot>,
    pub rag_hits: Vec<RagHit>,
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
        {
            return String::new();
        }
        let mut out = String::from("[Query context]\n");
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
                out.push_str(&format!(
                    "- {} ({}) @ {}:{}\n",
                    s.name,
                    s.kind,
                    s.path.display(),
                    s.line
                ));
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
            for h in &self.rag_hits {
                out.push_str(&format!("- {}:{} {}\n", h.path.display(), h.line, h.snippet));
            }
        }
        out.push_str("[/Query context]\n");
        out
    }
}

pub struct ContextBuilder {
    cwd: PathBuf,
    additional_dirs: Vec<PathBuf>,
    system_prompt_injection: Option<String>,

    open_files_source: Option<Arc<dyn OpenFilesSource>>,
    focus_files: Vec<PathBuf>,
    symbol_graph: Option<Arc<dyn SymbolGraphLookup>>,
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
            additional_dirs: Vec::new(),
            system_prompt_injection: None,
            open_files_source: None,
            focus_files: Vec::new(),
            symbol_graph: None,
            lsp_source: None,
            rag_source: None,
            rag_query: None,
            rag_top_k: 5,
            lsp_timeout: Duration::from_secs(3),
        }
    }

    pub fn with_additional_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.additional_dirs = dirs;
        self
    }

    pub fn with_system_prompt_injection(mut self, injection: Option<String>) -> Self {
        self.system_prompt_injection = injection;
        self
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

        let mut memory_search_dirs = vec![self.cwd.clone()];
        memory_search_dirs.extend(self.additional_dirs.clone());
        let memory_fut = MemoryFileContext::load(&memory_search_dirs);

        let focus_for_outline = self.focus_files.clone();
        let outline_fut = async move {
            tokio::task::spawn_blocking(move || collect_outline_for_focus(&focus_for_outline))
                .await
                .unwrap_or_default()
        };

        let symbol_graph_ref = self.symbol_graph.clone();
        let focus_for_symbols = self.focus_files.clone();
        let symbols_fut = async move {
            match symbol_graph_ref {
                Some(g) => tokio::task::spawn_blocking(move || g.snapshot_for_focus(&focus_for_symbols))
                    .await
                    .unwrap_or_default(),
                None => Vec::new(),
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

        let rag_hits = match (self.rag_source.as_ref(), self.rag_query.as_ref()) {
            (Some(rag), Some(q)) => rag.retrieve(q, self.rag_top_k).await,
            _ => Vec::new(),
        };

        let (git_res, memory, outline, symbols, open_files, lsp_info) = tokio::join!(
            git_fut,
            memory_fut,
            outline_fut,
            symbols_fut,
            open_files_fut,
            lsp_fut
        );
        let git = git_res.ok();

        let mut system_prompt = SystemPromptParts::default();
        if let Some(ref injection) = self.system_prompt_injection {
            system_prompt.injections.push(injection.clone());
        }

        let date = chrono::Local::now().format("%Y-%m-%d").to_string();

        let ctx = QueryContext {
            system_prompt,
            git,
            memory,
            cwd: self.cwd.clone(),
            date,
            additional_instructions: Vec::new(),
            open_files,
            focus_files: self.focus_files.clone(),
            symbols,
            outline,
            lsp_info,
            rag_hits,
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

static FOCUS_REGISTRY: Lazy<RwLock<Vec<PathBuf>>> = Lazy::new(|| RwLock::new(Vec::new()));

impl FocusPathRegistry {

    pub fn set(paths: Vec<PathBuf>) {
        if let Ok(mut guard) = FOCUS_REGISTRY.write() {
            *guard = paths;
        }
    }

    pub fn push(paths: &[PathBuf]) {
        if paths.is_empty() {
            return;
        }
        if let Ok(mut guard) = FOCUS_REGISTRY.write() {
            for p in paths {
                if !guard.contains(p) {
                    guard.push(p.clone());
                }
            }
        }
    }

    #[must_use]
    pub fn current() -> Vec<PathBuf> {
        FOCUS_REGISTRY
            .read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn clear() {
        if let Ok(mut guard) = FOCUS_REGISTRY.write() {
            guard.clear();
        }
    }
}

fn collect_outline_for_focus(focus: &[PathBuf]) -> Vec<OutlineNode> {
    let mut out = Vec::new();
    for path in focus {
        let entries = match crate::code_intel::outline::extract_outline(path, None) {
            Ok(v) => v,
            Err(_) => continue,
        };
        for entry in entries {
            out.push(OutlineNode::leaf(
                path.clone(),
                entry.kind,
                entry.name,
                entry.line,
            ));
        }
    }
    out
}
