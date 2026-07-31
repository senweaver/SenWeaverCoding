// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::LazyLock;

use dashmap::DashMap;

static CALIBRATION: LazyLock<DashMap<String, u64>> = LazyLock::new(DashMap::new);

pub fn family_from_model(model: &str) -> String {
    let id = model.rsplit('/').next().unwrap_or(model).to_ascii_lowercase();
    const FAMILIES: &[&str] = &[
        "claude", "gpt", "o1", "o3", "o4", "gemini", "qwen", "kimi", "moonshot",
        "deepseek", "glm", "doubao", "llama", "mistral", "grok", "command", "yi",
    ];
    for fam in FAMILIES {
        if id.contains(fam) {
            return (*fam).to_string();
        }
    }
    "default".to_string()
}

pub fn record_usage_calibration(model: &str, estimated_tokens: usize, reported_tokens: u64) {
    if estimated_tokens == 0 || reported_tokens == 0 {
        return;
    }
    let family = family_from_model(model);
    let base_estimate = (estimated_tokens as f64).max(1.0);
    let target_millis = ((reported_tokens as f64) / base_estimate * 1000.0).clamp(250.0, 4000.0);
    CALIBRATION
        .entry(family)
        .and_modify(|prev| {
            *prev = ((*prev as f64) * 0.8 + target_millis * 0.2).clamp(250.0, 4000.0) as u64;
        })
        .or_insert_with(|| (1000.0_f64 * 0.8 + target_millis * 0.2).clamp(250.0, 4000.0) as u64);
}

pub fn calibration_factor_for(model: &str) -> f64 {
    let family = family_from_model(model);
    CALIBRATION
        .get(family.as_str())
        .map(|v| *v as f64 / 1000.0)
        .unwrap_or(1.0)
}

struct PromptAnchor {
    reported: u64,
    estimated_calibrated: usize,
    at: std::time::Instant,
}

static PROMPT_ANCHORS: LazyLock<DashMap<String, PromptAnchor>> = LazyLock::new(DashMap::new);

const PROMPT_ANCHOR_MAX_ENTRIES: usize = 512;
const PROMPT_ANCHOR_TTL: std::time::Duration = std::time::Duration::from_secs(6 * 3600);

pub fn record_prompt_anchor(session_key: &str, reported: u64, estimated_calibrated: usize) {
    if session_key.is_empty() || reported == 0 || estimated_calibrated == 0 {
        return;
    }
    if PROMPT_ANCHORS.len() > PROMPT_ANCHOR_MAX_ENTRIES {
        PROMPT_ANCHORS.retain(|_, anchor| anchor.at.elapsed() < PROMPT_ANCHOR_TTL);
    }
    PROMPT_ANCHORS.insert(
        session_key.to_string(),
        PromptAnchor {
            reported,
            estimated_calibrated,
            at: std::time::Instant::now(),
        },
    );
}

pub fn anchored_history_tokens(session_key: &str, current_estimate: usize) -> usize {
    let Some(anchor) = PROMPT_ANCHORS.get(session_key) else {
        return current_estimate;
    };
    if anchor.at.elapsed() > PROMPT_ANCHOR_TTL || anchor.estimated_calibrated == 0 {
        return current_estimate;
    }
    let ratio = (anchor.reported as f64 / anchor.estimated_calibrated as f64).clamp(0.25, 4.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        ((current_estimate as f64) * ratio).round() as usize
    }
}

#[must_use]
pub fn estimate_history_tokens_calibrated(
    messages: &[crate::providers::ChatMessage],
    model: &str,
) -> usize {
    let raw: usize = messages
        .iter()
        .map(crate::providers::traits::estimate_message_tokens)
        .sum();
    let factor = calibration_factor_for(model);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        ((raw as f64 * 1.05) * factor).round() as usize
    }
}

#[must_use]
pub fn estimate_tokens_calibrated(text: &str, model: &str) -> usize {
    let base = crate::providers::traits::estimate_content_tokens(text).saturating_add(4);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let scaled = (base as f64 * calibration_factor_for(model)).round() as usize;
    scaled.max(1)
}

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
        crate::providers::traits::estimate_content_tokens(text)
            .saturating_add(4)
            .max(1)
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

    pub fn context_window(&self) -> usize {
        self.config.context_window
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
    const MAX_DEPTH: usize = 24;
    const MAX_SCAN_TIME: std::time::Duration = std::time::Duration::from_secs(3);

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

    let started = std::time::Instant::now();
    let mut total_lines: u64 = 0;
    let mut files_checked: u64 = 0;

    let mut dirs: Vec<(std::path::PathBuf, usize)> = vec![(workspace.to_path_buf(), 0)];

    'outer: while let Some((dir, depth)) = dirs.pop() {
        if started.elapsed() >= MAX_SCAN_TIME {
            break;
        }

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
                if depth >= MAX_DEPTH {
                    continue;
                }
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                if !SKIP_DIRS.contains(&name) {
                    dirs.push((path, depth + 1));
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
            if files_checked % 256 == 0 && started.elapsed() >= MAX_SCAN_TIME {
                break 'outer;
            }
        }
    }

    total_lines
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocCacheEntry {
    loc: u64,
    root_mtime_secs: u64,
}

fn workspace_root_mtime_secs(workspace: &std::path::Path) -> u64 {
    std::fs::metadata(workspace)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn loc_cache_path(cache_dir: &std::path::Path) -> std::path::PathBuf {
    cache_dir.join("project_loc_cache.json")
}

fn read_loc_cache(cache_dir: &std::path::Path) -> Option<LocCacheEntry> {
    let raw = std::fs::read_to_string(loc_cache_path(cache_dir)).ok()?;
    serde_json::from_str::<LocCacheEntry>(&raw).ok()
}

fn write_loc_cache(cache_dir: &std::path::Path, entry: &LocCacheEntry) {
    if std::fs::create_dir_all(cache_dir).is_err() {
        return;
    }
    if let Ok(serialized) = serde_json::to_string(entry) {
        let _ = std::fs::write(loc_cache_path(cache_dir), serialized);
    }
}

pub fn count_source_loc_cached(workspace: &std::path::Path, cache_dir: &std::path::Path) -> u64 {
    let current_mtime = workspace_root_mtime_secs(workspace);

    if let Some(entry) = read_loc_cache(cache_dir) {
        if entry.root_mtime_secs == current_mtime {
            return entry.loc;
        }

        let workspace_owned = workspace.to_path_buf();
        let cache_dir_owned = cache_dir.to_path_buf();
        std::thread::spawn(move || {
            let fresh = count_source_loc(&workspace_owned);
            write_loc_cache(
                &cache_dir_owned,
                &LocCacheEntry {
                    loc: fresh,
                    root_mtime_secs: workspace_root_mtime_secs(&workspace_owned),
                },
            );
        });

        return entry.loc;
    }

    let loc = count_source_loc(workspace);
    write_loc_cache(
        cache_dir,
        &LocCacheEntry {
            loc,
            root_mtime_secs: current_mtime,
        },
    );
    loc
}
