// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::fmt;

use crate::agent::token_budget::TokenBudgetManager;
use crate::context::builder::QueryContext;
use crate::observability::code_intel_metrics;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BudgetTier {

    SystemPrompt,

    FocusFiles,

    SymbolsOfFocus,

    OpenFiles,

    RagHits,

    Memory,
}

impl BudgetTier {

    #[must_use]
    pub fn all_in_compression_order() -> [BudgetTier; 5] {
        [
            BudgetTier::Memory,
            BudgetTier::RagHits,
            BudgetTier::OpenFiles,
            BudgetTier::SymbolsOfFocus,
            BudgetTier::FocusFiles,
        ]
    }

    #[must_use]
    pub fn default_ratio(self) -> f32 {
        match self {
            BudgetTier::SystemPrompt => 1.0,
            BudgetTier::FocusFiles => 1.0,
            BudgetTier::SymbolsOfFocus => 0.8,
            BudgetTier::OpenFiles => 0.6,
            BudgetTier::RagHits => 0.4,
            BudgetTier::Memory => 0.2,
        }
    }
}

impl fmt::Display for BudgetTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            BudgetTier::SystemPrompt => "system_prompt",
            BudgetTier::FocusFiles => "focus_files",
            BudgetTier::SymbolsOfFocus => "symbols_of_focus",
            BudgetTier::OpenFiles => "open_files",
            BudgetTier::RagHits => "rag_hits",
            BudgetTier::Memory => "memory",
        };
        f.write_str(name)
    }
}

#[derive(Debug, Clone)]
pub struct BudgetedSection {
    pub tier: BudgetTier,
    pub content: String,
    pub tokens: usize,

    pub degraded: bool,
}

#[derive(Debug, Clone)]
pub struct BudgetedQueryContext {
    pub sections: Vec<BudgetedSection>,
    pub total_tokens: usize,
    pub dropped: Vec<BudgetTier>,
}

impl BudgetedQueryContext {

    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        for s in &self.sections {
            if s.content.is_empty() {
                continue;
            }
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&s.content);
        }
        out
    }
}

#[derive(Debug, Clone)]
pub struct BudgetConfig {
    pub total_tokens: usize,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            total_tokens: 32_000,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ContextBudgetManager {
    config: BudgetConfig,
}

impl ContextBudgetManager {
    #[must_use]
    pub fn new(config: BudgetConfig) -> Self {
        Self { config }
    }

    #[must_use]
    pub fn with_total_tokens(total_tokens: usize) -> Self {
        Self {
            config: BudgetConfig { total_tokens },
        }
    }

    #[must_use]
    pub fn allocate(&self, qc: &QueryContext) -> BudgetedQueryContext {
        let mut sections = Vec::new();
        let total_cap = self.config.total_tokens.max(1);

        sections.push(render_section(
            BudgetTier::SystemPrompt,
            qc.system_prompt.assemble(),
        ));

        sections.push(render_section(
            BudgetTier::FocusFiles,
            render_focus_files(qc),
        ));
        sections.push(render_section(
            BudgetTier::SymbolsOfFocus,
            render_symbols(qc),
        ));
        sections.push(render_section(
            BudgetTier::OpenFiles,
            render_open_files(qc),
        ));
        sections.push(render_section(BudgetTier::RagHits, render_rag(qc)));
        sections.push(render_section(BudgetTier::Memory, render_memory(qc)));

        let mut dropped = Vec::new();
        let mut total: usize = sections.iter().map(|s| s.tokens).sum();

        for tier in BudgetTier::all_in_compression_order() {
            if total <= total_cap {
                break;
            }
            if let Some(pos) = sections.iter().position(|s| s.tier == tier) {
                let removed = sections.remove(pos);
                if removed.tokens > 0 {
                    total = total.saturating_sub(removed.tokens);
                    dropped.push(tier);
                    code_intel_metrics::incr_context_budget_compression_step();
                }
            }
        }

        if total > total_cap {
            if let Some(focus) = sections
                .iter_mut()
                .find(|s| s.tier == BudgetTier::FocusFiles)
            {
                let outline = render_outline(qc);
                focus.content = outline;
                focus.tokens = TokenBudgetManager::estimate_tokens(&focus.content);
                focus.degraded = true;
                code_intel_metrics::incr_context_budget_compression_step();
                total = sections.iter().map(|s| s.tokens).sum();
            }
        }

        BudgetedQueryContext {
            sections,
            total_tokens: total,
            dropped,
        }
    }
}

fn render_section(tier: BudgetTier, content: String) -> BudgetedSection {
    let tokens = TokenBudgetManager::estimate_tokens(&content);
    BudgetedSection {
        tier,
        content,
        tokens,
        degraded: false,
    }
}

fn render_focus_files(qc: &QueryContext) -> String {
    if qc.focus_files.is_empty() {
        return String::new();
    }
    let mut out = String::from("[Focus files]\n");
    for p in &qc.focus_files {
        out.push_str(&format!("- {}\n", p.display()));
    }
    out
}

fn render_symbols(qc: &QueryContext) -> String {
    if qc.symbols.is_empty() {
        return String::new();
    }
    let mut out = String::from("[Symbols]\n");
    for s in &qc.symbols {
        out.push_str(&format!(
            "- {} ({}) {}:{}\n",
            s.name,
            s.kind,
            s.path.display(),
            s.line
        ));
    }
    out
}

fn render_open_files(qc: &QueryContext) -> String {
    if qc.open_files.is_empty() {
        return String::new();
    }
    let mut out = String::from("[Open files]\n");
    for f in &qc.open_files {
        out.push_str(&format!("- {}\n", f.path.display()));
    }
    out
}

fn render_rag(qc: &QueryContext) -> String {
    if qc.rag_hits.is_empty() {
        return String::new();
    }
    let mut out = String::from("[RAG hits]\n");
    for h in &qc.rag_hits {
        out.push_str(&format!("- {}:{} {}\n", h.path.display(), h.line, h.snippet));
    }
    out
}

fn render_memory(qc: &QueryContext) -> String {
    let agents = qc.memory.agents_md.len();
    let claude = qc.memory.claude_md.len();
    let memory = qc.memory.memory_files.len();
    if agents + claude + memory == 0 {
        return String::new();
    }
    format!(
        "[Memory] agents_md={} claude_md={} memory_files={}\n",
        agents, claude, memory
    )
}

fn render_outline(qc: &QueryContext) -> String {
    if qc.outline.is_empty() {
        return render_focus_files(qc);
    }
    let mut out = String::from("[Focus outline]\n");
    for o in &qc.outline {
        out.push_str(&format!(
            "- {} {} @ {}:{}\n",
            o.kind,
            o.name,
            o.path.display(),
            o.line
        ));
    }
    out
}
