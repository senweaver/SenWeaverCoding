// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::budget::{TokenBudgetConfig, TokenBudgetManager};
use crate::agent::tool_handler::output_compressor::{
    CompressionResult, ToolOutputCompressor, ToolOutputCompressorConfig,
};
use parking_lot::{Mutex, RwLock};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

pub struct TokenOptimizer {
    compressor: ToolOutputCompressor,
    budget: TokenBudgetManager,
    stats: Arc<OptimizerStats>,
}

struct OptimizerStats {
    total_tool_calls: AtomicU64,
    compressed_tool_calls: AtomicU64,
    total_chars_in: AtomicU64,
    total_chars_out: AtomicU64,
}

impl OptimizerStats {
    fn new() -> Self {
        Self {
            total_tool_calls: AtomicU64::new(0),
            compressed_tool_calls: AtomicU64::new(0),
            total_chars_in: AtomicU64::new(0),
            total_chars_out: AtomicU64::new(0),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OptimizationReport {
    pub total_tool_calls: u64,
    pub compressed_tool_calls: u64,
    pub total_chars_in: u64,
    pub total_chars_out: u64,
    pub chars_saved: u64,
    pub estimated_tokens_saved: u64,
    pub savings_pct: f64,
    pub budget_utilization_pct: f64,
}

impl TokenOptimizer {
    pub fn new(
        compressor_config: ToolOutputCompressorConfig,
        budget_config: TokenBudgetConfig,
    ) -> Self {
        Self {
            compressor: ToolOutputCompressor::new(compressor_config),
            budget: TokenBudgetManager::new(budget_config),
            stats: Arc::new(OptimizerStats::new()),
        }
    }

    pub fn compress_tool_output(&self, tool_name: &str, output: &str) -> String {
        self.stats.total_tool_calls.fetch_add(1, Ordering::Relaxed);
        self.stats
            .total_chars_in
            .fetch_add(output.len() as u64, Ordering::Relaxed);

        let result: CompressionResult = self.compressor.compress(tool_name, output);

        self.stats
            .total_chars_out
            .fetch_add(result.compressed_chars as u64, Ordering::Relaxed);

        if !result.strategies_applied.is_empty() {
            self.stats
                .compressed_tool_calls
                .fetch_add(1, Ordering::Relaxed);
            self.budget.record_savings(result.estimated_tokens_saved());

            if result.savings_pct() > 5.0 {
                tracing::debug!(
                    tool = tool_name,
                    original = result.original_chars,
                    compressed = result.compressed_chars,
                    savings_pct = format!("{:.1}%", result.savings_pct()),
                    strategies = ?result.strategies_applied,
                    "tool output compressed"
                );
            }
        } else if self.compressor.is_disabled() {

            tracing::trace!(
                tool = tool_name,
                "tool output compression disabled or no strategies active; passing through unchanged"
            );
        }

        result.output
    }

    pub fn should_compress_history(&self, system_prompt: &str, history_text_total: usize) -> bool {
        let sys_tokens = TokenBudgetManager::estimate_tokens(system_prompt);
        let status = self.budget.check_status(sys_tokens, history_text_total / 4);
        status.should_compress
    }

    pub fn max_tool_result_chars(&self) -> usize {
        self.budget.max_tool_result_chars()
    }

    pub fn max_rag_chars(&self) -> usize {
        self.budget.max_rag_chars()
    }

    pub fn record_api_usage(&self, input_tokens: usize, output_tokens: usize) {
        self.budget.record_usage(input_tokens, output_tokens);
    }

    pub fn suggest_max_messages(&self, current_tokens: usize, message_count: usize) -> usize {
        self.budget
            .suggest_max_messages(current_tokens, message_count)
    }

    pub fn report(&self, system_prompt_tokens: usize, history_tokens: usize) -> OptimizationReport {
        let total_in = self.stats.total_chars_in.load(Ordering::Relaxed);
        let total_out = self.stats.total_chars_out.load(Ordering::Relaxed);
        let chars_saved = total_in.saturating_sub(total_out);
        let savings_pct = if total_in > 0 {
            (chars_saved as f64 / total_in as f64) * 100.0
        } else {
            0.0
        };

        let budget_status = self
            .budget
            .check_status(system_prompt_tokens, history_tokens);

        OptimizationReport {
            total_tool_calls: self.stats.total_tool_calls.load(Ordering::Relaxed),
            compressed_tool_calls: self.stats.compressed_tool_calls.load(Ordering::Relaxed),
            total_chars_in: total_in,
            total_chars_out: total_out,
            chars_saved,
            estimated_tokens_saved: chars_saved / 4,
            savings_pct,
            budget_utilization_pct: budget_status.utilization_pct,
        }
    }

    pub fn budget(&self) -> &TokenBudgetManager {
        &self.budget
    }
}

pub fn create_optimizer(
    compressor_config: ToolOutputCompressorConfig,
    budget_config: TokenBudgetConfig,
) -> Arc<TokenOptimizer> {
    Arc::new(TokenOptimizer::new(compressor_config, budget_config))
}

static GLOBAL_OPTIMIZER: std::sync::LazyLock<arc_swap::ArcSwapOption<TokenOptimizer>> =
    std::sync::LazyLock::new(arc_swap::ArcSwapOption::empty);

static GLOBAL_PROJECT_LOC: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

type BaseConfig = (ToolOutputCompressorConfig, TokenBudgetConfig);

static BASE_CONFIG: std::sync::LazyLock<arc_swap::ArcSwapOption<BaseConfig>> =
    std::sync::LazyLock::new(arc_swap::ArcSwapOption::empty);

fn workspace_optimizers() -> &'static RwLock<HashMap<String, Arc<TokenOptimizer>>> {
    static MAP: OnceLock<RwLock<HashMap<String, Arc<TokenOptimizer>>>> = OnceLock::new();
    MAP.get_or_init(|| RwLock::new(HashMap::new()))
}

fn workspace_loc_in_flight() -> &'static Mutex<HashSet<String>> {
    static IN_FLIGHT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    IN_FLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
}

fn apply_project_loc(
    mut budget_config: TokenBudgetConfig,
    project_loc: u64,
) -> TokenBudgetConfig {
    if project_loc > 0
        && budget_config.max_tool_result_tokens
            == super::budget::default_max_tool_result_tokens()
    {
        budget_config.max_tool_result_tokens =
            super::budget::dynamic_max_tool_result_tokens(project_loc as usize);
    }
    budget_config
}

pub fn ensure_global_optimizer(
    compressor_config: ToolOutputCompressorConfig,
    budget_config: TokenBudgetConfig,
) {
    BASE_CONFIG.store(Some(Arc::new((
        compressor_config.clone(),
        budget_config.clone(),
    ))));
    let project_loc = GLOBAL_PROJECT_LOC.load(std::sync::atomic::Ordering::Relaxed);
    let budget_config = apply_project_loc(budget_config, project_loc);
    GLOBAL_OPTIMIZER.store(Some(create_optimizer(compressor_config, budget_config)));
}

pub fn ensure_global_optimizer_from_config(config: &crate::config::Config) {
    ensure_global_optimizer(
        config.tool_output_compressor.clone(),
        config.token_budget.clone(),
    );
}

pub fn ensure_global_optimizer_with_loc(
    compressor_config: ToolOutputCompressorConfig,
    budget_config: TokenBudgetConfig,
    project_loc: u64,
) {
    GLOBAL_PROJECT_LOC.store(project_loc, std::sync::atomic::Ordering::Relaxed);
    ensure_global_optimizer(compressor_config, budget_config);
}

pub fn set_workspace_project_loc(workspace_key: &str, project_loc: u64) {
    let Some(base) = BASE_CONFIG.load_full() else {
        return;
    };
    let (compressor_config, budget_config) = (*base).clone();
    let budget_config = apply_project_loc(budget_config, project_loc);
    let optimizer = create_optimizer(compressor_config, budget_config);
    workspace_optimizers()
        .write()
        .insert(workspace_key.to_string(), optimizer);
}

pub fn ensure_workspace_optimizer(workspace_dir: PathBuf) {
    if workspace_dir.as_os_str().is_empty() {
        return;
    }
    if BASE_CONFIG.load_full().is_none() {
        return;
    }
    let workspace_key = crate::session::workspace_key_from_path(&workspace_dir, "");
    if workspace_optimizers().read().contains_key(&workspace_key) {
        return;
    }
    {
        let mut in_flight = workspace_loc_in_flight().lock();
        if !in_flight.insert(workspace_key.clone()) {
            return;
        }
    }
    if tokio::runtime::Handle::try_current().is_err() {
        workspace_loc_in_flight().lock().remove(&workspace_key);
        return;
    }
    crate::runtime::spawn_supervised("workspace_token_budget", async move {
        let scan_dir = workspace_dir.clone();
        let cache_dir = workspace_dir.join(".senweavercoding");
        let project_loc = tokio::task::spawn_blocking(move || {
            super::budget::count_source_loc_cached(&scan_dir, &cache_dir)
        })
        .await
        .unwrap_or(0);
        set_workspace_project_loc(&workspace_key, project_loc);
        workspace_loc_in_flight().lock().remove(&workspace_key);
    });
}

pub fn active_optimizer() -> Option<Arc<TokenOptimizer>> {
    if let Some(ctx) = crate::session::current_session_context() {
        if let Some(opt) = workspace_optimizers().read().get(&ctx.workspace_key).cloned() {
            return Some(opt);
        }
    }
    GLOBAL_OPTIMIZER.load_full()
}

pub fn global_optimizer() -> Option<Arc<TokenOptimizer>> {
    active_optimizer()
}

pub fn compress_output(tool_name: &str, output: &str) -> String {
    match active_optimizer() {
        Some(opt) => opt.compress_tool_output(tool_name, output),
        None => output.to_string(),
    }
}
