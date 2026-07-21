// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;
use std::sync::OnceLock;

pub mod dispatcher;
pub mod embedded_filters;
pub mod fast_paths;
pub mod mute;
pub mod pipeline;
pub mod read_level;
pub mod tee;
pub mod toml_dsl;
pub mod tracking;

pub use dispatcher::{classify, HandlerKind, RuleMatch};
pub use mute::{is_disabled_by_env, should_skip_command};
pub use read_level::ReadLevel;

pub const TEE_MARKER_PREFIX: &str = "[full output: ";

pub fn extract_tee_path(output: &str) -> Option<PathBuf> {
    let start = output.rfind(TEE_MARKER_PREFIX)?;
    let after = &output[start + TEE_MARKER_PREFIX.len()..];
    let end = after.find(']')?;
    let raw = after[..end].trim();
    if raw.is_empty() {
        return None;
    }
    Some(PathBuf::from(raw))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactLevel {
    Conservative,
    Balanced,
    Aggressive,
}

impl Default for CompactLevel {
    fn default() -> Self {
        Self::Conservative
    }
}

#[derive(Debug, Clone)]
pub struct CompactContext {
    pub level: CompactLevel,
    pub tee_enabled: bool,
    pub tracking_enabled: bool,
    pub data_dir: PathBuf,

    pub custom_filters_dir: Option<PathBuf>,

    pub raw_byte_cap: usize,
}

static GLOBAL: OnceLock<arc_swap::ArcSwap<CompactContext>> = OnceLock::new();

pub fn set_global(ctx: CompactContext) {
    let cell = GLOBAL.get_or_init(|| arc_swap::ArcSwap::from_pointee(CompactContext::anchored_default()));
    cell.store(std::sync::Arc::new(ctx));
}

pub fn global() -> std::sync::Arc<CompactContext> {
    let cell = GLOBAL.get_or_init(|| arc_swap::ArcSwap::from_pointee(CompactContext::anchored_default()));
    cell.load_full()
}

pub fn is_enabled() -> bool {
    ENABLED_FLAG.load(std::sync::atomic::Ordering::Relaxed)
}

static ENABLED_FLAG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn set_enabled(enabled: bool) {
    ENABLED_FLAG.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

impl CompactContext {

    pub fn anchored_default() -> Self {
        let data_dir = directories::ProjectDirs::from("", "", "sen")
            .map(|d| d.data_dir().to_path_buf())
            .unwrap_or_else(|| std::env::temp_dir().join("sen"));
        Self {
            level: CompactLevel::default(),
            tee_enabled: true,
            tracking_enabled: true,
            data_dir,
            custom_filters_dir: None,
            raw_byte_cap: 1_048_576,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CompactedOutput {
    pub stdout: String,
    pub stderr: String,

    pub tee_path: Option<PathBuf>,

    pub tokens_saved: u64,

    pub category: Option<&'static str>,
}

impl CompactedOutput {

    pub fn passthrough(stdout: String, stderr: String) -> Self {
        Self {
            stdout,
            stderr,
            tee_path: None,
            tokens_saved: 0,
            category: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GrepHit {
    pub file: String,
    pub line_no: u64,
    pub line: String,
}

#[derive(Debug, Clone)]
pub struct GrepOpts {
    pub level: CompactLevel,

    pub per_file_cap: usize,

    pub total_cap: usize,
}

impl Default for GrepOpts {
    fn default() -> Self {
        Self {
            level: CompactLevel::default(),
            per_file_cap: 5,
            total_cap: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub is_hidden: bool,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct ListOpts {
    pub level: CompactLevel,

    pub group_by_ext: bool,
}

impl Default for ListOpts {
    fn default() -> Self {
        Self {
            level: CompactLevel::default(),
            group_by_ext: true,
        }
    }
}

pub fn compact_command_output(
    command: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    exit_code: i32,
    ctx: &CompactContext,
) -> CompactedOutput {
    if is_disabled_by_env() {
        return CompactedOutput::passthrough(raw_stdout.to_string(), raw_stderr.to_string());
    }
    if should_skip_command(command) {
        return CompactedOutput::passthrough(raw_stdout.to_string(), raw_stderr.to_string());
    }

    let raw_total = raw_stdout.len() + raw_stderr.len();
    let stdout_capped = cap_raw(raw_stdout, ctx.raw_byte_cap);
    let stderr_capped = cap_raw(raw_stderr, ctx.raw_byte_cap);

    let (compacted_stdout, compacted_stderr, category) = match classify(command) {
        Some(RuleMatch {
            handler: HandlerKind::FastPath(handler),
            category,
            ..
        }) => {
            let out = handler(command, &stdout_capped, &stderr_capped, exit_code, ctx);
            (out.0, out.1, Some(category))
        }
        Some(RuleMatch {
            handler: HandlerKind::Toml(name),
            category,
            ..
        }) => {
            let rule = toml_dsl::lookup(name, ctx);
            let stdout = rule
                .as_ref()
                .map(|r| pipeline::apply(r, &stdout_capped, ctx.level))
                .unwrap_or_else(|| pipeline::strip_ansi_only(&stdout_capped));
            (stdout, pipeline::strip_ansi_only(&stderr_capped), Some(category))
        }
        Some(RuleMatch {
            handler: HandlerKind::Passthrough,
            category,
            ..
        }) => (
            pipeline::strip_ansi_only(&stdout_capped),
            pipeline::strip_ansi_only(&stderr_capped),
            Some(category),
        ),
        None => (
            pipeline::strip_ansi_only(&stdout_capped),
            pipeline::strip_ansi_only(&stderr_capped),
            None,
        ),
    };

    let mut stdout = compacted_stdout;
    let stderr = compacted_stderr;

    let tee_path = if ctx.tee_enabled && exit_code != 0 {
        match tee::write_failure_log(command, raw_stdout, raw_stderr, &ctx.data_dir) {
            Ok(path) => {
                let hint = format!("\n[full output: {}]", path.display());
                stdout.push_str(&hint);
                Some(path)
            }
            Err(_) => None,
        }
    } else {
        None
    };

    let compacted_total = stdout.len() + stderr.len();
    let tokens_saved = estimate_tokens_saved(raw_total, compacted_total);

    if ctx.tracking_enabled {
        let _ = tracking::record(
            command,
            category.unwrap_or("passthrough"),
            raw_total,
            compacted_total,
            exit_code,
            &ctx.data_dir,
        );
    }

    crate::observability::token_saver_metrics::record_compaction(
        raw_total as u64,
        compacted_total as u64,
        tokens_saved,
        category.is_none(),
        tee_path.is_some(),
    );

    CompactedOutput {
        stdout,
        stderr,
        tee_path,
        tokens_saved,
        category,
    }
}

pub fn compact_file_content(path: &str, content: &str, level: ReadLevel) -> String {
    let raw_bytes = content.len() as u64;
    let out = read_level::compact(path, content, level);
    let compacted_bytes = out.len() as u64;
    let tokens_saved = estimate_tokens_saved(content.len(), out.len());
    crate::observability::token_saver_metrics::record_compaction(
        raw_bytes,
        compacted_bytes,
        tokens_saved,
        matches!(level, ReadLevel::Default),
        false,
    );
    out
}

pub fn compact_grep_results(matches: &[GrepHit], opts: &GrepOpts) -> String {
    let raw_bytes: usize = matches
        .iter()
        .map(|h| h.file.len() + h.line.len() + 16)
        .sum();
    let out = fast_paths::system::compact_grep(matches, opts);
    let compacted_bytes = out.len();
    let tokens_saved = estimate_tokens_saved(raw_bytes, compacted_bytes);
    crate::observability::token_saver_metrics::record_compaction(
        raw_bytes as u64,
        compacted_bytes as u64,
        tokens_saved,
        matches!(opts.level, CompactLevel::Conservative),
        false,
    );
    out
}

pub fn compact_dir_listing(entries: &[DirEntry], opts: &ListOpts) -> String {
    let raw_bytes: usize = entries.iter().map(|e| e.name.len() + 2).sum();
    let out = fast_paths::system::compact_listing(entries, opts);
    let compacted_bytes = out.len();
    let tokens_saved = estimate_tokens_saved(raw_bytes, compacted_bytes);
    crate::observability::token_saver_metrics::record_compaction(
        raw_bytes as u64,
        compacted_bytes as u64,
        tokens_saved,
        matches!(opts.level, CompactLevel::Conservative),
        false,
    );
    out
}

pub fn compact_tool_output(label: &str, content: &str, ctx: &CompactContext) -> String {
    if is_disabled_by_env() {
        return content.to_string();
    }

    let raw_total = content.len();
    let stripped = pipeline::strip_ansi_only(&cap_raw(content, ctx.raw_byte_cap));

    let budget = match ctx.level {
        CompactLevel::Conservative => 16_384,
        CompactLevel::Balanced => 8_192,
        CompactLevel::Aggressive => 4_096,
    };

    if stripped.len() <= budget {
        return stripped;
    }

    let tee_path = if ctx.tee_enabled {
        tee::write_failure_log(label, content, "", &ctx.data_dir).ok()
    } else {
        None
    };

    let mut out = middle_out_trim(&stripped, budget);
    if let Some(path) = tee_path.as_ref() {
        out.push_str(&format!("\n[full output: {}]", path.display()));
    }

    let compacted_total = out.len();
    let tokens_saved = estimate_tokens_saved(raw_total, compacted_total);
    if ctx.tracking_enabled {
        let _ = tracking::record(
            label,
            "tool_output",
            raw_total,
            compacted_total,
            0,
            &ctx.data_dir,
        );
    }
    crate::observability::token_saver_metrics::record_compaction(
        raw_total as u64,
        compacted_total as u64,
        tokens_saved,
        false,
        tee_path.is_some(),
    );
    out
}

pub fn estimate_tokens(text: &str) -> u64 {
    crate::agent::token::budget::TokenBudgetManager::estimate_tokens(text) as u64
}

fn middle_out_trim(text: &str, budget: usize) -> String {
    let half = budget / 2;
    let head = char_boundary_prefix(text, half);
    let tail = char_boundary_suffix(text, half);
    let omitted = text.len().saturating_sub(head.len() + tail.len());
    format!(
        "{head}\n... [{omitted} bytes trimmed by token_saver; see full output below] ...\n{tail}"
    )
}

fn char_boundary_prefix(text: &str, max: usize) -> &str {
    let mut b = max.min(text.len());
    while b > 0 && !text.is_char_boundary(b) {
        b -= 1;
    }
    &text[..b]
}

fn char_boundary_suffix(text: &str, max: usize) -> &str {
    let mut b = text.len().saturating_sub(max);
    while b < text.len() && !text.is_char_boundary(b) {
        b += 1;
    }
    &text[b..]
}

fn estimate_tokens_saved(raw_bytes: usize, compacted_bytes: usize) -> u64 {
    let raw_tokens = (raw_bytes.div_ceil(4)).saturating_add(4) as u64;
    let compacted_tokens = (compacted_bytes.div_ceil(4)).saturating_add(4) as u64;
    raw_tokens.saturating_sub(compacted_tokens)
}

fn cap_raw(text: &str, cap: usize) -> String {
    match crate::util::truncate_head_tail(text, cap, 25) {
        Some(clipped) => clipped,
        None => text.to_string(),
    }
}
