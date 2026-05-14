// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Centralized token budget management and allocation.
//!
//! Provides real-time token estimation, budget allocation per component
//! (system prompt, history, tool results, RAG context), and auto-triggers
//! compression when approaching limits. Inspired by RTK's token tracking
//! combined with SenWeaverCoding's existing context window management.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TokenBudgetConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_context_window")]
    pub context_window: usize,

    #[serde(default = "default_system_prompt_ratio")]
    pub system_prompt_ratio: f32,

    #[serde(default = "default_output_ratio")]
    pub output_ratio: f32,

    #[serde(default = "default_compression_threshold")]
    pub compression_threshold: f32,

    #[serde(default = "default_max_tool_result_tokens")]
    pub max_tool_result_tokens: usize,

    #[serde(default = "default_max_rag_tokens")]
    pub max_rag_tokens: usize,
}

fn default_context_window() -> usize {
    128_000
}
fn default_system_prompt_ratio() -> f32 {
    0.15
}
fn default_output_ratio() -> f32 {
    0.15
}
fn default_compression_threshold() -> f32 {
    0.75
}
pub fn default_max_tool_result_tokens() -> usize {
    12_000
}
fn default_max_rag_tokens() -> usize {
    8_000
}

#[must_use]
pub fn dynamic_max_tool_result_tokens(project_loc: usize) -> usize {
    const LOW_LOC: f64 = 5_000.0;
    const HIGH_LOC: f64 = 50_000.0;
    const LOW_BUDGET: f64 = 12_000.0;
    const HIGH_BUDGET: f64 = 32_000.0;
    const MIN_BUDGET: usize = 8_000;
    const MAX_BUDGET: usize = 64_000;

    if project_loc == 0 {
        return default_max_tool_result_tokens();
    }
    let loc = project_loc as f64;
    let scaled = if loc <= LOW_LOC {
        LOW_BUDGET
    } else if loc >= HIGH_LOC {
        HIGH_BUDGET
    } else {
        let t = (loc - LOW_LOC) / (HIGH_LOC - LOW_LOC);
        LOW_BUDGET + t * (HIGH_BUDGET - LOW_BUDGET)
    };
    (scaled as usize).clamp(MIN_BUDGET, MAX_BUDGET)
}

impl Default for TokenBudgetConfig {
    fn default() -> Self {
        Self {

            enabled: true,
            context_window: default_context_window(),
            system_prompt_ratio: default_system_prompt_ratio(),
            output_ratio: default_output_ratio(),
            compression_threshold: default_compression_threshold(),
            max_tool_result_tokens: default_max_tool_result_tokens(),
            max_rag_tokens: default_max_rag_tokens(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BudgetAllocation {
    pub total_tokens: usize,
    pub system_prompt_budget: usize,
    pub output_budget: usize,
    pub history_budget: usize,
    pub tool_result_budget: usize,
    pub rag_budget: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct BudgetStatus {
    pub allocation: BudgetAllocation,
    pub system_prompt_used: usize,
    pub history_used: usize,
    pub available_for_history: usize,
    pub utilization_pct: f64,
    pub should_compress: bool,
    pub cumulative_tokens_saved: usize,
}

pub struct TokenBudgetManager {
    config: TokenBudgetConfig,
    allocation: BudgetAllocation,
    cumulative_saved: Arc<AtomicUsize>,
    cumulative_input: Arc<AtomicUsize>,
    cumulative_output: Arc<AtomicUsize>,
}

impl TokenBudgetManager {
    pub fn new(config: TokenBudgetConfig) -> Self {
        let allocation = Self::compute_allocation(&config);
        Self {
            config,
            allocation,
            cumulative_saved: Arc::new(AtomicUsize::new(0)),
            cumulative_input: Arc::new(AtomicUsize::new(0)),
            cumulative_output: Arc::new(AtomicUsize::new(0)),
        }
    }

    #[must_use]
    pub fn with_project_loc(project_loc: usize) -> Self {
        let mut config = TokenBudgetConfig::default();
        if project_loc > 0 {
            config.max_tool_result_tokens = dynamic_max_tool_result_tokens(project_loc);
        }
        Self::new(config)
    }

    fn compute_allocation(config: &TokenBudgetConfig) -> BudgetAllocation {
        let total = config.context_window;
        let system_prompt_budget = (total as f64 * config.system_prompt_ratio as f64) as usize;
        let output_budget = (total as f64 * config.output_ratio as f64) as usize;
        let rag_budget = config.max_rag_tokens.min(total / 10);
        let history_budget = total
            .saturating_sub(system_prompt_budget)
            .saturating_sub(output_budget)
            .saturating_sub(rag_budget);

        BudgetAllocation {
            total_tokens: total,
            system_prompt_budget,
            output_budget,
            history_budget,
            tool_result_budget: config.max_tool_result_tokens,
            rag_budget,
        }
    }

    pub fn estimate_tokens(text: &str) -> usize {
        text.len().div_ceil(4).saturating_add(4)
    }

    pub fn estimate_messages_tokens(messages: &[impl AsRef<str>]) -> usize {
        messages
            .iter()
            .map(|m| Self::estimate_tokens(m.as_ref()))
            .sum()
    }

    pub fn check_status(&self, system_prompt_tokens: usize, history_tokens: usize) -> BudgetStatus {
        let available = self.allocation.history_budget.saturating_sub(
            system_prompt_tokens.saturating_sub(self.allocation.system_prompt_budget),
        );

        let utilization = if available > 0 {
            history_tokens as f64 / available as f64
        } else {
            1.0
        };

        let should_compress = utilization > self.config.compression_threshold as f64;

        BudgetStatus {
            allocation: self.allocation.clone(),
            system_prompt_used: system_prompt_tokens,
            history_used: history_tokens,
            available_for_history: available,
            utilization_pct: utilization * 100.0,
            should_compress,
            cumulative_tokens_saved: self.cumulative_saved.load(Ordering::Relaxed),
        }
    }

    pub fn record_savings(&self, tokens_saved: usize) {
        self.cumulative_saved
            .fetch_add(tokens_saved, Ordering::Relaxed);
    }

    pub fn record_usage(&self, input_tokens: usize, output_tokens: usize) {
        self.cumulative_input
            .fetch_add(input_tokens, Ordering::Relaxed);
        self.cumulative_output
            .fetch_add(output_tokens, Ordering::Relaxed);
    }

    pub fn max_tool_result_chars(&self) -> usize {
        self.allocation.tool_result_budget * 4
    }

    pub fn max_rag_chars(&self) -> usize {
        self.allocation.rag_budget * 4
    }

    pub fn usage_stats(&self) -> TokenUsageStats {
        TokenUsageStats {
            cumulative_input_tokens: self.cumulative_input.load(Ordering::Relaxed),
            cumulative_output_tokens: self.cumulative_output.load(Ordering::Relaxed),
            cumulative_tokens_saved: self.cumulative_saved.load(Ordering::Relaxed),
            context_window: self.config.context_window,
        }
    }

    pub fn allocation(&self) -> &BudgetAllocation {
        &self.allocation
    }

    pub fn suggest_max_messages(&self, current_tokens: usize, message_count: usize) -> usize {
        if message_count == 0 || current_tokens == 0 {
            return message_count;
        }

        let avg_per_message = current_tokens / message_count;
        if avg_per_message == 0 {
            return message_count;
        }

        let budget = self.allocation.history_budget;
        let target = (budget as f64 * self.config.compression_threshold as f64) as usize;
        let suggested = target / avg_per_message;

        suggested.max(4).min(message_count)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TokenUsageStats {
    pub cumulative_input_tokens: usize,
    pub cumulative_output_tokens: usize,
    pub cumulative_tokens_saved: usize,
    pub context_window: usize,
}

impl TokenUsageStats {
    pub fn total_tokens(&self) -> usize {
        self.cumulative_input_tokens + self.cumulative_output_tokens
    }

    pub fn savings_pct(&self) -> f64 {
        let total_possible = self.total_tokens() + self.cumulative_tokens_saved;
        if total_possible == 0 {
            return 0.0;
        }
        (self.cumulative_tokens_saved as f64 / total_possible as f64) * 100.0
    }
}

pub fn count_source_loc(workspace: &std::path::Path) -> u64 {
    const SOURCE_EXTENSIONS: &[&str] = &[
        "rs", "py", "ts", "go", "java", "c", "cpp", "h",
    ];

    const MAX_LOC_FILES: u64 = 10_000;

    const SKIP_DIRS: &[&str] = &[
        ".git",
        "target",
        "node_modules",
        "__pycache__",
        ".venv",
        "venv",
        ".mypy_cache",
        "dist",
        "build",
        ".cache",
    ];

    let mut total_lines: u64 = 0;
    let mut files_checked: u64 = 0;

    let mut dirs: Vec<std::path::PathBuf> = vec![workspace.to_path_buf()];

    'outer: while let Some(dir) = dirs.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        let mut children: Vec<std::path::PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .collect();
        children.sort_unstable();

        for path in children {
            if path.is_dir() {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                if !SKIP_DIRS.contains(&name) {
                    dirs.push(path);
                }
                continue;
            }

            let ext = match path.extension().and_then(|e| e.to_str()) {
                Some(e) => e,
                None => continue,
            };
            if !SOURCE_EXTENSIONS.contains(&ext) {
                continue;
            }

            if let Ok(content) = std::fs::read_to_string(&path) {
                total_lines +=
                    content.lines().filter(|l| !l.trim().is_empty()).count() as u64;
            }

            files_checked += 1;
            if files_checked >= MAX_LOC_FILES {
                break 'outer;
            }
        }
    }

    total_lines
}
