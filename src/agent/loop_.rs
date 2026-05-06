// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub use crate::agent::loop_ctx::LoopContext;

use crate::approval::{ApprovalManager, ApprovalRequest, ApprovalResponse};
use crate::config::Config;
use crate::config::schema::ModelPricing;
use crate::cost::CostTracker;
use crate::cost::types::{BudgetCheck, TokenUsage as CostTokenUsage};
use crate::i18n::ToolDescriptions;
use crate::memory::{self, Memory, MemoryCategory, decay};
use crate::multimodal;
use crate::observability::{self, Observer, ObserverEvent, runtime_trace};
use crate::providers::traits::StreamEvent;
use crate::providers::{
    self, ChatMessage, ChatRequest, Provider, ProviderCapabilityError, ToolCall,
};
use crate::runtime;
use crate::security::{AutonomyLevel, SecurityPolicy};
use crate::tools::{self, Tool, ToolRegistry};
use crate::util::truncate_with_ellipsis;
use anyhow::Result;
use futures_util::StreamExt;
use regex::{Regex, RegexSet};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt::Write;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone)]
pub struct ToolLoopCostTrackingContext {
    pub tracker: Arc<CostTracker>,
    pub prices: Arc<std::collections::HashMap<String, ModelPricing>>,

    pub chat_session_id: Option<String>,
}

impl ToolLoopCostTrackingContext {
    pub fn new(
        tracker: Arc<CostTracker>,
        prices: Arc<std::collections::HashMap<String, ModelPricing>>,
    ) -> Self {
        Self {
            tracker,
            prices,
            chat_session_id: None,
        }
    }

    pub fn with_chat_session_id(mut self, chat_session_id: impl Into<String>) -> Self {
        self.chat_session_id = Some(chat_session_id.into());
        self
    }
}

tokio::task_local! {
    pub static TOOL_LOOP_COST_TRACKING_CONTEXT: Option<ToolLoopCostTrackingContext>;
}

pub async fn scope_tool_loop_cost_tracking<F, R>(
    ctx: Option<ToolLoopCostTrackingContext>,
    f: F,
) -> R
where
    F: std::future::Future<Output = R>,
{
    TOOL_LOOP_COST_TRACKING_CONTEXT.scope(ctx, f).await
}

fn lookup_model_pricing<'a>(
    prices: &'a std::collections::HashMap<String, ModelPricing>,
    provider_name: &str,
    model: &str,
) -> Option<&'a ModelPricing> {
    prices
        .get(model)
        .or_else(|| prices.get(&format!("{provider_name}/{model}")))
        .or_else(|| {
            model
                .rsplit_once('/')
                .and_then(|(_, suffix)| prices.get(suffix))
        })
}

pub(crate) fn record_tool_loop_cost_usage(
    provider_name: &str,
    model: &str,
    usage: &crate::providers::traits::TokenUsage,
) -> Option<(u64, f64)> {
    let input_tokens = usage.input_tokens.unwrap_or(0);
    let output_tokens = usage.output_tokens.unwrap_or(0);
    let total_tokens = input_tokens.saturating_add(output_tokens);
    if total_tokens == 0 {
        return None;
    }

    let ctx = TOOL_LOOP_COST_TRACKING_CONTEXT
        .try_with(Clone::clone)
        .ok()
        .flatten()?;
    let pricing = lookup_model_pricing(&ctx.prices, provider_name, model);
    let cost_usage = CostTokenUsage::new(
        model,
        input_tokens,
        output_tokens,
        pricing.map_or(0.0, |entry| entry.input),
        pricing.map_or(0.0, |entry| entry.output),
    );

    if pricing.is_none() {
        tracing::debug!(
            provider = provider_name,
            model,
            "Cost tracking recorded token usage with zero pricing (no pricing entry found)"
        );
    }

    if let Err(error) = ctx
        .tracker
        .record_usage_for_session(ctx.chat_session_id.as_deref(), cost_usage.clone())
    {
        tracing::warn!(
            provider = provider_name,
            model,
            "Failed to record cost tracking usage: {error}"
        );
    }

    Some((cost_usage.total_tokens, cost_usage.cost_usd))
}

pub(crate) fn check_tool_loop_budget(estimated_cost_usd: Option<f64>) -> Option<BudgetCheck> {
    TOOL_LOOP_COST_TRACKING_CONTEXT
        .try_with(Clone::clone)
        .ok()
        .flatten()
        .map(|ctx| {
            let cost = estimated_cost_usd.unwrap_or(0.01);
            ctx.tracker
                .check_budget(cost)
                .unwrap_or(BudgetCheck::Allowed)
        })
}

const STREAM_CHUNK_MIN_CHARS: usize = 80;

const STREAM_TOOL_MARKER_WINDOW_CHARS: usize = 512;

const DEFAULT_MAX_TOOL_ITERATIONS: usize = 2000;

const AUTOSAVE_MIN_MESSAGE_CHARS: usize = 20;

#[allow(clippy::type_complexity)]
pub type ModelSwitchCallback = Arc<parking_lot::Mutex<Option<(String, String)>>>;

#[derive(Clone, Default)]
pub(crate) struct ModelSwitchState {
    pub switch: Arc<parking_lot::Mutex<Option<(String, String)>>>,
}

tokio::task_local! {
    static MODEL_SWITCH_STATE: ModelSwitchState;
}

pub fn get_model_switch_state() -> ModelSwitchCallback {
    MODEL_SWITCH_STATE
        .try_with(|s| Arc::clone(&s.switch))
        .unwrap_or_else(|_| Arc::new(parking_lot::Mutex::new(None)))
}

pub fn clear_model_switch_request() {
    if let Ok(state) = MODEL_SWITCH_STATE.try_with(|s| Arc::clone(&s.switch)) {
        let mut guard = state.lock();
        *guard = None;
    }
}

pub async fn scope_model_switch<F, R>(f: F) -> R
where
    F: std::future::Future<Output = R>,
{
    let state = ModelSwitchState::default();
    MODEL_SWITCH_STATE.scope(state, f).await
}

fn glob_match(pattern: &str, name: &str) -> bool {
    match pattern.find('*') {
        None => pattern == name,
        Some(star) => {
            let prefix = &pattern[..star];
            let suffix = &pattern[star + 1..];
            name.starts_with(prefix)
                && name.ends_with(suffix)
                && name.len() >= prefix.len() + suffix.len()
        }
    }
}

use crate::security::permissions::is_mcp_tool_name;

pub(crate) fn filter_tool_specs_for_turn(
    tool_specs: Vec<crate::tools::ToolSpec>,
    groups: &[crate::config::schema::ToolFilterGroup],
    user_message: &str,
) -> Vec<crate::tools::ToolSpec> {
    use crate::config::schema::ToolFilterGroupMode;

    if groups.is_empty() {
        return tool_specs;
    }

    let msg_lower = user_message.to_ascii_lowercase();

    tool_specs
        .into_iter()
        .filter(|spec| {

            if !is_mcp_tool_name(&spec.name) {
                return true;
            }

            groups.iter().any(|group| {
                let pattern_matches = group.tools.iter().any(|pat| glob_match(pat, &spec.name));
                if !pattern_matches {
                    return false;
                }
                match group.mode {
                    ToolFilterGroupMode::Always => true,
                    ToolFilterGroupMode::Dynamic => group
                        .keywords
                        .iter()
                        .any(|kw| msg_lower.contains(&kw.to_ascii_lowercase())),
                }
            })
        })
        .collect()
}

pub(crate) fn filter_by_allowed_tools(
    specs: Vec<crate::tools::ToolSpec>,
    allowed: Option<&[String]>,
) -> Vec<crate::tools::ToolSpec> {
    match allowed {
        None => specs,
        Some(list) => specs
            .into_iter()
            .filter(|spec| list.iter().any(|name| name == &spec.name))
            .collect(),
    }
}

fn is_plan_mode_allowed(tool_name: &str) -> bool {
    crate::security::permissions::is_plan_mode_allowed_tool(tool_name)
}

fn compute_excluded_mcp_tools(
    tools_registry: &[Box<dyn Tool>],
    groups: &[crate::config::schema::ToolFilterGroup],
    user_message: &str,
) -> Vec<String> {
    if groups.is_empty() {
        return Vec::new();
    }
    let filtered_specs = filter_tool_specs_for_turn(
        tools_registry.iter().map(|t| t.spec()).collect(),
        groups,
        user_message,
    );
    let included: HashSet<&str> = filtered_specs.iter().map(|s| s.name.as_str()).collect();
    tools_registry
        .iter()
        .filter(|t| is_mcp_tool_name(t.name()) && !included.contains(t.name()))
        .map(|t| t.name().to_string())
        .collect()
}

static SENSITIVE_KEY_PATTERNS: LazyLock<RegexSet> = LazyLock::new(|| {
    RegexSet::new([
        r"(?i)token",
        r"(?i)api[_-]?key",
        r"(?i)password",
        r"(?i)secret",
        r"(?i)user[_-]?key",
        r"(?i)bearer",
        r"(?i)credential",
    ])
    .unwrap()
});

static SENSITIVE_KV_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(token|api[_-]?key|password|secret|user[_-]?key|bearer|credential)["']?\s*[:=]\s*(?:"([^"]{8,})"|'([^']{8,})'|([a-zA-Z0-9_\-\.]{8,}))"#).unwrap()
});

pub(crate) fn scrub_credentials(input: &str) -> String {
    SENSITIVE_KV_REGEX
        .replace_all(input, |caps: &regex::Captures| {
            let full_match = &caps[0];
            let key = &caps[1];
            let val = caps
                .get(2)
                .or(caps.get(3))
                .or(caps.get(4))
                .map(|m| m.as_str())
                .unwrap_or("");

            let prefix = if val.len() > 4 {
                val.char_indices()
                    .nth(4)
                    .map(|(byte_idx, _)| &val[..byte_idx])
                    .unwrap_or(val)
            } else {
                ""
            };

            if full_match.contains(':') {
                if full_match.contains('"') {
                    format!("\"{}\": \"{}*[REDACTED]\"", key, prefix)
                } else {
                    format!("{}: {}*[REDACTED]", key, prefix)
                }
            } else if full_match.contains('=') {
                if full_match.contains('"') {
                    format!("{}=\"{}*[REDACTED]\"", key, prefix)
                } else {
                    format!("{}={}*[REDACTED]", key, prefix)
                }
            } else {
                format!("{}: {}*[REDACTED]", key, prefix)
            }
        })
        .to_string()
}

const DEFAULT_MAX_HISTORY_MESSAGES: usize = 50;

const COMPACTION_KEEP_RECENT_MESSAGES: usize = 20;

const COMPACTION_MAX_SOURCE_CHARS: usize = 12_000;

const COMPACTION_MAX_SUMMARY_CHARS: usize = 2_000;

pub fn estimate_history_tokens(history: &[ChatMessage]) -> usize {
    history
        .iter()
        .map(|m| {

            m.content.len().div_ceil(4) + 4
        })
        .sum()
}

fn estimate_tokens_filtered(history: &[ChatMessage], is_system: bool) -> usize {
    history
        .iter()
        .filter(|m| (m.role == "system") == is_system)
        .map(|m| m.content.len().div_ceil(4) + 4)
        .sum()
}

pub(crate) const PROGRESS_MIN_INTERVAL_MS: u64 = 500;

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum DraftEvent {

    Clear,

    Progress(String),

    Content(String),

    Thinking(String),

    ToolCall {
        name: String,
        args: serde_json::Value,
    },

    ToolResult { name: String, output: String },

    FileEdit {
        path: String,
        additions: i32,
        deletions: i32,
        diff: Option<String>,
        edit_batch_id: Option<String>,
    },

    ProgressTick {
        iteration: usize,
        max_iterations: usize,
        tokens_used: u64,
    },

    ContextCompressed {
        tokens_before: usize,
        tokens_after: usize,
    },

    Cancelling { reason: String },

    Error { message: String },

    UsageUpdate {
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        cached_input_tokens: Option<u64>,
    },

    Subagent {
        task_id: String,
        agent_id: String,
        kind: crate::agent::SubagentChunkKind,
        delta: String,
    },
}

tokio::task_local! {
    pub(crate) static TOOL_CHOICE_OVERRIDE: Option<String>;
}

tokio::task_local! {

    pub(crate) static PARENT_DRAFT_CHANNEL:
        Option<tokio::sync::mpsc::Sender<DraftEvent>>;
}

pub fn take_parent_draft_channel() -> Option<tokio::sync::mpsc::Sender<DraftEvent>> {
    PARENT_DRAFT_CHANNEL.try_with(|c| c.clone()).ok().flatten()
}

fn truncate_tool_args_for_progress(name: &str, args: &serde_json::Value, max_len: usize) -> String {
    let hint = match name {
        "shell" => args.get("command").and_then(|v| v.as_str()),
        "file_read" | "file_write" => args.get("path").and_then(|v| v.as_str()),
        _ => args
            .get("action")
            .and_then(|v| v.as_str())
            .or_else(|| args.get("query").and_then(|v| v.as_str())),
    };
    match hint {
        Some(s) => truncate_with_ellipsis(s, max_len),
        None => String::new(),
    }
}

fn tools_to_openai_format(tools_registry: &[Box<dyn Tool>]) -> Vec<serde_json::Value> {
    let mut seen: std::collections::HashSet<String> =
        std::collections::HashSet::with_capacity(tools_registry.len());
    tools_registry
        .iter()
        .filter(|tool| seen.insert(tool.name().to_string()))
        .map(|tool| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": tool.name(),
                    "description": tool.description(),
                    "parameters": tool.parameters_schema()
                }
            })
        })
        .collect()
}

fn autosave_memory_key(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4())
}

fn memory_session_id_from_state_file(path: &Path) -> Option<String> {
    let raw = path.to_string_lossy().trim().to_string();
    if raw.is_empty() {
        return None;
    }

    Some(format!("cli:{raw}"))
}

fn trim_history(history: &mut Vec<ChatMessage>, max_history: usize) {

    let has_system = history.first().map_or(false, |m| m.role == "system");
    let non_system_count = if has_system {
        history.len() - 1
    } else {
        history.len()
    };

    if non_system_count <= max_history {
        return;
    }

    let start = usize::from(has_system);
    let to_remove = non_system_count - max_history;
    history.drain(start..start + to_remove);
}

fn build_compaction_transcript(messages: &[ChatMessage]) -> String {
    let mut transcript = String::new();
    for msg in messages {
        let role = msg.role.to_uppercase();
        let _ = writeln!(transcript, "{role}: {}", msg.content.trim());
    }

    if transcript.chars().count() > COMPACTION_MAX_SOURCE_CHARS {
        truncate_with_ellipsis(&transcript, COMPACTION_MAX_SOURCE_CHARS)
    } else {
        transcript
    }
}

fn apply_compaction_summary(
    history: &mut Vec<ChatMessage>,
    start: usize,
    compact_end: usize,
    summary: &str,
) {
    let summary_msg = ChatMessage::assistant(format!("[Compaction summary]\n{}", summary.trim()));
    history.splice(start..compact_end, std::iter::once(summary_msg));
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InteractiveSessionState {
    version: u32,
    history: Vec<ChatMessage>,
}

impl InteractiveSessionState {
    fn from_history(history: &[ChatMessage]) -> Self {
        Self {
            version: 1,
            history: history.to_vec(),
        }
    }
}

fn load_interactive_session_history(path: &Path, system_prompt: &str) -> Result<Vec<ChatMessage>> {
    if !path.exists() {
        return Ok(vec![ChatMessage::system(system_prompt)]);
    }

    let raw = std::fs::read_to_string(path)?;
    let mut state: InteractiveSessionState = serde_json::from_str(&raw)?;
    if state.history.is_empty() {
        state.history.push(ChatMessage::system(system_prompt));
    } else if state.history.first().map(|msg| msg.role.as_str()) != Some("system") {
        state.history.insert(0, ChatMessage::system(system_prompt));
    }

    Ok(state.history)
}

fn save_interactive_session_history(path: &Path, history: &[ChatMessage]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let payload = serde_json::to_string_pretty(&InteractiveSessionState::from_history(history))?;
    std::fs::write(path, payload)?;
    Ok(())
}

fn apply_theme_formatting(text: &str) -> String {
    let theme = std::env::var("SEN_THEME").unwrap_or_else(|_| "default".into());
    match theme.as_str() {
        "concise" => {
            let mut result = String::new();
            let mut in_thinking = false;
            for line in text.lines() {
                if line.contains("<thinking>") || line.contains("<antThinking>") {
                    in_thinking = true;
                    continue;
                }
                if line.contains("</thinking>") || line.contains("</antThinking>") {
                    in_thinking = false;
                    continue;
                }
                if !in_thinking {
                    result.push_str(line);
                    result.push('\n');
                }
            }
            result.trim_end().to_string()
        }
        "code-only" => {
            let mut result = String::new();
            let mut in_code = false;
            for line in text.lines() {
                if line.starts_with("```") {
                    in_code = !in_code;
                    result.push_str(line);
                    result.push('\n');
                } else if in_code {
                    result.push_str(line);
                    result.push('\n');
                }
            }
            if result.is_empty() {
                text.to_string()
            } else {
                result.trim_end().to_string()
            }
        }
        "formal" => {
            format!(
                "── Agent Response ──────────────────────────────────\n\
                 {text}\n\
                 ────────────────────────────────────────────────────"
            )
        }
        _ => text.to_string(),
    }
}

#[deprecated(
    since = "0.1.0",
    note = "Use crate::agent::context_expansion::expand_input instead — the unified resolver handles @file:/@folder:/@symbol:/@codebase: and falls back here for legacy tokens."
)]
pub fn expand_at_file_references(input: &str, workspace: &std::path::Path) -> String {

    let at_re =
        regex::Regex::new(r#"@((?:\./|[a-zA-Z0-9_])[^\s,;!?'"()\[\]{}:]+)(?::(\d+)(?::(\d+))?)?"#)
            .unwrap();

    let mut result = input.to_string();
    let mut replacements: Vec<(String, String)> = Vec::new();

    for cap in at_re.captures_iter(input) {
        let full_match = cap[0].to_string();
        let path_str = &cap[1];
        let line_num: Option<usize> = cap.get(2).and_then(|m| m.as_str().parse().ok());
        let _col_num: Option<usize> = cap.get(3).and_then(|m| m.as_str().parse().ok());

        if replacements.iter().any(|(m, _)| m == &full_match) {
            continue;
        }

        let path = workspace.join(path_str);

        if path_str.contains('*') || path_str.contains('?') {
            if let Ok(entries) = glob::glob(&path.to_string_lossy()) {
                let mut files_content = Vec::new();
                for (i, entry) in entries.flatten().enumerate() {
                    if i >= 10 {
                        files_content.push("(... truncated, showing first 10 matches)".to_string());
                        break;
                    }
                    if let Ok(content) = std::fs::read_to_string(&entry) {
                        let rel = entry.strip_prefix(workspace).unwrap_or(&entry);
                        let truncated = truncate_content(&content, 20_000);
                        files_content.push(format!(
                            "<file path=\"{path}\">\n{truncated}\n</file>",
                            path = rel.display(),
                        ));
                    }
                }
                if !files_content.is_empty() {
                    replacements.push((full_match, files_content.join("\n\n")));
                }
            }
        } else if path.is_file() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                let expanded = if let Some(target_line) = line_num {

                    extract_line_window(&content, path_str, target_line, 10)
                } else {
                    let truncated = truncate_content(&content, 50_000);
                    format!("<file path=\"{path_str}\">\n{truncated}\n</file>")
                };
                replacements.push((full_match, expanded));
            }
        } else if path.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&path) {
                let listing: Vec<String> = entries
                    .flatten()
                    .take(50)
                    .map(|e| {
                        let ft = if e.path().is_dir() { "dir" } else { "file" };
                        format!("  [{ft}] {}", e.file_name().to_string_lossy())
                    })
                    .collect();
                replacements.push((
                    full_match,
                    format!(
                        "<directory path=\"{path_str}\">\n{listing}\n</directory>",
                        listing = listing.join("\n")
                    ),
                ));
            }
        }
    }

    for (pattern, replacement) in replacements {
        result = result.replacen(&pattern, &replacement, 1);
    }

    result
}

fn extract_line_window(content: &str, path: &str, target: usize, radius: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();

    if target == 0 || target > total {
        return format!(
            "<file path=\"{path}\" line=\"{target}\">\nLine {target} is out of range \
             (file has {total} lines)\n</file>"
        );
    }

    let start = target.saturating_sub(radius).max(1);
    let end = (target + radius).min(total);

    let numbered: Vec<String> = (start..=end)
        .map(|i| {
            let marker = if i == target { ">" } else { " " };
            format!("{marker}{i:>6}| {}", lines[i - 1])
        })
        .collect();

    format!(
        "<file path=\"{path}\" line=\"{target}\" range=\"{start}-{end}\">\n{}\n</file>",
        numbered.join("\n")
    )
}

fn truncate_content(content: &str, max_bytes: usize) -> String {
    if content.len() > max_bytes {
        let mut end = max_bytes;
        while end > 0 && !content.is_char_boundary(end) {
            end -= 1;
        }
        format!(
            "{}...\n(truncated, {} bytes total)",
            &content[..end],
            content.len()
        )
    } else {
        content.to_string()
    }
}

fn parse_slash_command_line(input: &str) -> Option<(String, Vec<String>)> {
    let s = input.trim();
    if !s.starts_with('/') {
        return None;
    }
    let rest = s[1..].trim_start();
    if rest.is_empty() {
        return None;
    }
    let mut parts = rest.split_whitespace();
    let cmd = parts.next()?.to_string();
    let args = parts.map(String::from).collect();
    Some((cmd, args))
}

async fn build_context(
    mem: &dyn Memory,
    user_msg: &str,
    min_relevance_score: f64,
    session_id: Option<&str>,
) -> String {
    let mut context = String::new();

    if let Ok(mut entries) = mem.recall(user_msg, 5, session_id, None, None).await {

        decay::apply_time_decay(&mut entries, decay::DEFAULT_HALF_LIFE_DAYS);

        let relevant: Vec<_> = entries
            .iter()
            .filter(|e| match e.score {
                Some(score) => score >= min_relevance_score,
                None => true,
            })
            .collect();

        if !relevant.is_empty() {
            context.push_str("[Memory context]\n");
            for entry in &relevant {
                if memory::is_assistant_autosave_key(&entry.key) {
                    continue;
                }
                if memory::should_skip_autosave_content(&entry.content) {
                    continue;
                }

                if entry.content.contains("<tool_result") {
                    continue;
                }
                let _ = writeln!(context, "- {}: {}", entry.key, entry.content);
            }
            if context == "[Memory context]\n" {
                context.clear();
            } else {
                context.push_str("[/Memory context]\n\n");
            }
        }
    }

    if let Some(block) = code_intel_injection_block(user_msg).await {
        if !block.is_empty() {
            context.push_str(&block);
        }
    }

    context
}

async fn code_intel_injection_block(user_msg: &str) -> Option<String> {
    use crate::agent::loop_services;

    let cwd = std::env::current_dir().ok()?;
    let registry_focus = crate::context::builder::FocusPathRegistry::current();
    let query = user_msg.trim();
    if registry_focus.is_empty() && query.is_empty() {
        return Some(String::new());
    }

    let mut builder = crate::context::builder::ContextBuilder::new(cwd.clone());
    if !registry_focus.is_empty() {
        builder = builder.with_focus_files(registry_focus);
    }
    if let Some(lsp) = loop_services::lsp_context_source() {
        builder = builder.with_lsp(lsp);
    }
    if let Some(graph) = loop_services::symbol_graph_source(&cwd) {
        builder = builder.with_symbol_graph(graph);
    }
    if !query.is_empty()
        && let Some(rag) = loop_services::rag_source(&cwd)
    {
        builder = builder.with_rag(rag, query.to_string());
    }
    match builder.build().await {
        Ok(qc) => {
            let qc: crate::context::builder::QueryContext = qc;
            Some(qc.render_injection_block())
        }
        Err(_) => None,
    }
}

fn build_hardware_context(
    rag: &crate::rag::HardwareRag,
    user_msg: &str,
    boards: &[String],
    chunk_limit: usize,
) -> String {
    if rag.is_empty() || boards.is_empty() {
        return String::new();
    }

    let mut context = String::new();

    let pin_ctx = rag.pin_alias_context(user_msg, boards);
    if !pin_ctx.is_empty() {
        context.push_str(&pin_ctx);
    }

    let chunks = rag.retrieve(user_msg, boards, chunk_limit);
    if chunks.is_empty() && pin_ctx.is_empty() {
        return String::new();
    }

    if !chunks.is_empty() {
        context.push_str("[Hardware documentation]\n");
    }
    for chunk in chunks {
        let board_tag = chunk.board.as_deref().unwrap_or("generic");
        let _ = writeln!(
            context,
            "--- {} ({}) ---\n{}\n",
            chunk.source, board_tag, chunk.content
        );
    }
    context.push('\n');
    context
}

fn find_tool<'a>(
    tools: &'a [Box<dyn Tool>],
    name: &str,
    tool_registry: Option<&ToolRegistry>,
) -> Option<crate::tools::handle::ToolHandle<'a>> {
    if let Some(reg) = tool_registry {
        if let Some(arc) = reg.get(name) {
            return Some(crate::tools::handle::ToolHandle::Owned(arc));
        }
    }
    tools
        .iter()
        .find(|t| t.name() == name)
        .map(|t| crate::tools::handle::ToolHandle::Borrowed(t.as_ref()))
}

fn parse_arguments_value(raw: Option<&serde_json::Value>) -> serde_json::Value {
    match raw {
        Some(serde_json::Value::String(s)) => serde_json::from_str::<serde_json::Value>(s)
            .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
        Some(value) => value.clone(),
        None => serde_json::Value::Object(serde_json::Map::new()),
    }
}

fn parse_tool_call_id(
    root: &serde_json::Value,
    function: Option<&serde_json::Value>,
) -> Option<String> {
    function
        .and_then(|func| func.get("id"))
        .or_else(|| root.get("id"))
        .or_else(|| root.get("tool_call_id"))
        .or_else(|| root.get("call_id"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(ToString::to_string)
}

pub fn canonicalize_json_for_tool_signature(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<String> = map.keys().cloned().collect();
            keys.sort_unstable();
            let mut ordered = serde_json::Map::new();
            for key in keys {
                if let Some(child) = map.get(&key) {
                    ordered.insert(key, canonicalize_json_for_tool_signature(child));
                }
            }
            serde_json::Value::Object(ordered)
        }
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .iter()
                .map(canonicalize_json_for_tool_signature)
                .collect(),
        ),
        _ => value.clone(),
    }
}

pub fn tool_call_signature(name: &str, arguments: &serde_json::Value) -> (String, String) {
    let canonical_args = canonicalize_json_for_tool_signature(arguments);
    let args_json = serde_json::to_string(&canonical_args).unwrap_or_else(|_| "{}".to_string());
    (name.trim().to_ascii_lowercase(), args_json)
}

fn parse_tool_call_value(value: &serde_json::Value) -> Option<ParsedToolCall> {
    if let Some(function) = value.get("function") {
        let tool_call_id = parse_tool_call_id(value, Some(function));
        let name = function
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if !name.is_empty() {
            let arguments = parse_arguments_value(
                function
                    .get("arguments")
                    .or_else(|| function.get("parameters")),
            );
            return Some(ParsedToolCall {
                name,
                arguments,
                tool_call_id,
                parse_error: false,
            });
        }
    }

    let tool_call_id = parse_tool_call_id(value, None);
    let name = value
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    if name.is_empty() {
        return None;
    }

    let arguments =
        parse_arguments_value(value.get("arguments").or_else(|| value.get("parameters")));
    Some(ParsedToolCall {
        name,
        arguments,
        tool_call_id,
        parse_error: false,
    })
}

fn parse_tool_calls_from_json_value(value: &serde_json::Value) -> Vec<ParsedToolCall> {
    let mut calls = Vec::new();

    if let Some(tool_calls) = value.get("tool_calls").and_then(|v| v.as_array()) {
        for call in tool_calls {
            if let Some(parsed) = parse_tool_call_value(call) {
                calls.push(parsed);
            }
        }

        if !calls.is_empty() {
            return calls;
        }
    }

    if let Some(array) = value.as_array() {
        for item in array {
            if let Some(parsed) = parse_tool_call_value(item) {
                calls.push(parsed);
            }
        }
        return calls;
    }

    if let Some(parsed) = parse_tool_call_value(value) {
        calls.push(parsed);
    }

    calls
}

fn is_xml_meta_tag(tag: &str) -> bool {
    let normalized = tag.to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "tool_call"
            | "toolcall"
            | "tool-call"
            | "invoke"
            | "thinking"
            | "thought"
            | "analysis"
            | "reasoning"
            | "reflection"
    )
}

static XML_OPEN_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<([a-zA-Z_][a-zA-Z0-9_-]*)>").unwrap());

static MINIMAX_INVOKE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<invoke\b[^>]*\bname\s*=\s*(?:"([^"]+)"|'([^']+)')[^>]*>(.*?)</invoke>"#)
        .unwrap()
});

static MINIMAX_PARAMETER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?is)<parameter\b[^>]*\bname\s*=\s*(?:"([^"]+)"|'([^']+)')[^>]*>(.*?)</parameter>"#,
    )
    .unwrap()
});

fn extract_xml_pairs(input: &str) -> Vec<(&str, &str)> {
    let mut results = Vec::new();
    let mut search_start = 0;
    while let Some(open_cap) = XML_OPEN_TAG_RE.captures(&input[search_start..]) {
        let Some(full_open) = open_cap.get(0) else {
            break;
        };
        let Some(tag_match) = open_cap.get(1) else {
            break;
        };
        let tag_name = tag_match.as_str();
        let open_end = search_start + full_open.end();

        let closing_tag = format!("</{tag_name}>");
        if let Some(close_pos) = input[open_end..].find(&closing_tag) {
            let inner = &input[open_end..open_end + close_pos];
            results.push((tag_name, inner.trim()));
            search_start = open_end + close_pos + closing_tag.len();
        } else {
            search_start = open_end;
        }
    }
    results
}

fn parse_xml_tool_calls(xml_content: &str) -> Option<Vec<ParsedToolCall>> {
    let mut calls = Vec::new();
    let trimmed = xml_content.trim();

    if !trimmed.starts_with('<') || !trimmed.contains('>') {
        return None;
    }

    for (tool_name_str, inner_content) in extract_xml_pairs(trimmed) {
        let tool_name = tool_name_str.to_string();
        if is_xml_meta_tag(&tool_name) {
            continue;
        }

        if inner_content.is_empty() {
            continue;
        }

        let mut args = serde_json::Map::new();

        if let Some(first_json) = extract_json_values(inner_content).into_iter().next() {
            match first_json {
                serde_json::Value::Object(object_args) => {
                    args = object_args;
                }
                other => {
                    args.insert("value".to_string(), other);
                }
            }
        } else {
            for (key_str, value) in extract_xml_pairs(inner_content) {
                let key = key_str.to_string();
                if is_xml_meta_tag(&key) {
                    continue;
                }
                if !value.is_empty() {
                    args.insert(key, serde_json::Value::String(value.to_string()));
                }
            }

            if args.is_empty() {
                args.insert(
                    "content".to_string(),
                    serde_json::Value::String(inner_content.to_string()),
                );
            }
        }

        calls.push(ParsedToolCall {
            name: tool_name,
            arguments: serde_json::Value::Object(args),
            tool_call_id: None,
            parse_error: false,
        });
    }

    if calls.is_empty() { None } else { Some(calls) }
}

fn parse_minimax_invoke_calls(response: &str) -> Option<(String, Vec<ParsedToolCall>)> {
    let mut calls = Vec::new();
    let mut text_parts = Vec::new();
    let mut last_end = 0usize;

    for cap in MINIMAX_INVOKE_RE.captures_iter(response) {
        let Some(full_match) = cap.get(0) else {
            continue;
        };

        let before = response[last_end..full_match.start()].trim();
        if !before.is_empty() {
            text_parts.push(before.to_string());
        }

        let name = cap
            .get(1)
            .or_else(|| cap.get(2))
            .map(|m| m.as_str().trim())
            .filter(|v| !v.is_empty());
        let body = cap.get(3).map(|m| m.as_str()).unwrap_or("").trim();
        last_end = full_match.end();

        let Some(name) = name else {
            continue;
        };

        let mut args = serde_json::Map::new();
        for param_cap in MINIMAX_PARAMETER_RE.captures_iter(body) {
            let key = param_cap
                .get(1)
                .or_else(|| param_cap.get(2))
                .map(|m| m.as_str().trim())
                .unwrap_or_default();
            if key.is_empty() {
                continue;
            }
            let value = param_cap
                .get(3)
                .map(|m| m.as_str().trim())
                .unwrap_or_default();
            if value.is_empty() {
                continue;
            }

            let parsed = extract_json_values(value).into_iter().next();
            args.insert(
                key.to_string(),
                parsed.unwrap_or_else(|| serde_json::Value::String(value.to_string())),
            );
        }

        if args.is_empty() {
            if let Some(first_json) = extract_json_values(body).into_iter().next() {
                match first_json {
                    serde_json::Value::Object(obj) => args = obj,
                    other => {
                        args.insert("value".to_string(), other);
                    }
                }
            } else if !body.is_empty() {
                args.insert(
                    "content".to_string(),
                    serde_json::Value::String(body.to_string()),
                );
            }
        }

        calls.push(ParsedToolCall {
            name: name.to_string(),
            arguments: serde_json::Value::Object(args),
            tool_call_id: None,
            parse_error: false,
        });
    }

    if calls.is_empty() {
        return None;
    }

    let after = response[last_end..].trim();
    if !after.is_empty() {
        text_parts.push(after.to_string());
    }

    let text = text_parts
        .join("\n")
        .replace("<minimax:tool_call>", "")
        .replace("</minimax:tool_call>", "")
        .replace("<minimax:toolcall>", "")
        .replace("</minimax:toolcall>", "")
        .trim()
        .to_string();

    Some((text, calls))
}

const TOOL_CALL_OPEN_TAGS: [&str; 6] = [
    "<tool_call>",
    "<toolcall>",
    "<tool-call>",
    "<invoke>",
    "<minimax:tool_call>",
    "<minimax:toolcall>",
];

const TOOL_CALL_CLOSE_TAGS: [&str; 6] = [
    "</tool_call>",
    "</toolcall>",
    "</tool-call>",
    "</invoke>",
    "</minimax:tool_call>",
    "</minimax:toolcall>",
];

fn find_first_tag<'a>(haystack: &str, tags: &'a [&'a str]) -> Option<(usize, &'a str)> {
    tags.iter()
        .filter_map(|tag| haystack.find(tag).map(|idx| (idx, *tag)))
        .min_by_key(|(idx, _)| *idx)
}

fn matching_tool_call_close_tag(open_tag: &str) -> Option<&'static str> {
    match open_tag {
        "<tool_call>" => Some("</tool_call>"),
        "<toolcall>" => Some("</toolcall>"),
        "<tool-call>" => Some("</tool-call>"),
        "<invoke>" => Some("</invoke>"),
        "<minimax:tool_call>" => Some("</minimax:tool_call>"),
        "<minimax:toolcall>" => Some("</minimax:toolcall>"),
        _ => None,
    }
}

fn extract_first_json_value_with_end(input: &str) -> Option<(serde_json::Value, usize)> {
    let trimmed = input.trim_start();
    let trim_offset = input.len().saturating_sub(trimmed.len());

    for (byte_idx, ch) in trimmed.char_indices() {
        if ch != '{' && ch != '[' {
            continue;
        }

        let slice = &trimmed[byte_idx..];
        let mut stream = serde_json::Deserializer::from_str(slice).into_iter::<serde_json::Value>();
        if let Some(Ok(value)) = stream.next() {
            let consumed = stream.byte_offset();
            if consumed > 0 {
                return Some((value, trim_offset + byte_idx + consumed));
            }
        }
    }

    None
}

fn strip_leading_close_tags(mut input: &str) -> &str {
    loop {
        let trimmed = input.trim_start();
        if !trimmed.starts_with("</") {
            return trimmed;
        }

        let Some(close_end) = trimmed.find('>') else {
            return "";
        };
        input = &trimmed[close_end + 1..];
    }
}

fn extract_json_values(input: &str) -> Vec<serde_json::Value> {
    let mut values = Vec::new();
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return values;
    }

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        values.push(value);
        return values;
    }

    let char_positions: Vec<(usize, char)> = trimmed.char_indices().collect();
    let mut idx = 0;
    while idx < char_positions.len() {
        let (byte_idx, ch) = char_positions[idx];
        if ch == '{' || ch == '[' {
            let slice = &trimmed[byte_idx..];
            let mut stream =
                serde_json::Deserializer::from_str(slice).into_iter::<serde_json::Value>();
            if let Some(Ok(value)) = stream.next() {
                let consumed = stream.byte_offset();
                if consumed > 0 {
                    values.push(value);
                    let next_byte = byte_idx + consumed;
                    while idx < char_positions.len() && char_positions[idx].0 < next_byte {
                        idx += 1;
                    }
                    continue;
                }
            }
        }
        idx += 1;
    }

    values
}

fn find_json_end(input: &str) -> Option<usize> {
    let trimmed = input.trim_start();
    let offset = input.len() - trimmed.len();

    if !trimmed.starts_with('{') {
        return None;
    }

    let mut depth = 0;
    let mut in_string = false;
    let mut escape_next = false;

    for (i, ch) in trimmed.char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }

        match ch {
            '\\' if in_string => escape_next = true,
            '"' => in_string = !in_string,
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some(offset + i + ch.len_utf8());
                }
            }
            _ => {}
        }
    }

    None
}

fn parse_xml_attribute_tool_calls(response: &str) -> Vec<ParsedToolCall> {
    let mut calls = Vec::new();

    static INVOKE_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?s)<invoke\s+name="([^"]+)"[^>]*>(.*?)</invoke>"#).unwrap()
    });

    static PARAM_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"<parameter\s+name="([^"]+)"[^>]*>([^<]*)</parameter>"#).unwrap()
    });

    for cap in INVOKE_RE.captures_iter(response) {
        let tool_name = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let inner = cap.get(2).map(|m| m.as_str()).unwrap_or("");

        if tool_name.is_empty() {
            continue;
        }

        let mut arguments = serde_json::Map::new();

        for param_cap in PARAM_RE.captures_iter(inner) {
            let param_name = param_cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let param_value = param_cap.get(2).map(|m| m.as_str()).unwrap_or("");

            if !param_name.is_empty() {
                arguments.insert(
                    param_name.to_string(),
                    serde_json::Value::String(param_value.to_string()),
                );
            }
        }

        if !arguments.is_empty() {
            calls.push(ParsedToolCall {
                name: map_tool_name_alias(tool_name).to_string(),
                arguments: serde_json::Value::Object(arguments),
                tool_call_id: None,
                parse_error: false,
            });
        }
    }

    calls
}

fn parse_perl_style_tool_calls(response: &str) -> Vec<ParsedToolCall> {
    let mut calls = Vec::new();

    static PERL_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?s)(?:\[TOOL_CALL\]|TOOL_CALL)\s*\{(.+?)\}\}\s*(?:\[/TOOL_CALL\]|/TOOL_CALL)")
            .unwrap()
    });

    static TOOL_NAME_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"tool\s*=>\s*"([^"]+)""#).unwrap());

    static ARGS_BLOCK_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)args\s*=>\s*\{(.+?)(?:\}|$)").unwrap());

    static ARGS_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"--(\w+)\s+"([^"]+)""#).unwrap());

    for cap in PERL_RE.captures_iter(response) {
        let content = cap.get(1).map(|m| m.as_str()).unwrap_or("");

        let tool_name = TOOL_NAME_RE
            .captures(content)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str())
            .unwrap_or("");

        if tool_name.is_empty() {
            continue;
        }

        let args_block = ARGS_BLOCK_RE
            .captures(content)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str())
            .unwrap_or("");

        let mut arguments = serde_json::Map::new();

        for arg_cap in ARGS_RE.captures_iter(args_block) {
            let key = arg_cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let value = arg_cap.get(2).map(|m| m.as_str()).unwrap_or("");

            if !key.is_empty() {
                arguments.insert(
                    key.to_string(),
                    serde_json::Value::String(value.to_string()),
                );
            }
        }

        if !arguments.is_empty() {
            calls.push(ParsedToolCall {
                name: map_tool_name_alias(tool_name).to_string(),
                arguments: serde_json::Value::Object(arguments),
                tool_call_id: None,
                parse_error: false,
            });
        }
    }

    calls
}

fn parse_function_call_tool_calls(response: &str) -> Vec<ParsedToolCall> {
    let mut calls = Vec::new();

    static FUNC_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?s)<FunctionCall>\s*(\w+)\s*<code>([^<]+)</code>\s*</FunctionCall>").unwrap()
    });

    for cap in FUNC_RE.captures_iter(response) {
        let tool_name = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let args_text = cap.get(2).map(|m| m.as_str()).unwrap_or("");

        if tool_name.is_empty() {
            continue;
        }

        let mut arguments = serde_json::Map::new();
        for line in args_text.lines() {
            let line = line.trim();
            if let Some(pos) = line.find('>') {
                let key = line[..pos].trim();
                let value = line[pos + 1..].trim();
                if !key.is_empty() && !value.is_empty() {
                    arguments.insert(
                        key.to_string(),
                        serde_json::Value::String(value.to_string()),
                    );
                }
            }
        }

        if !arguments.is_empty() {
            calls.push(ParsedToolCall {
                name: map_tool_name_alias(tool_name).to_string(),
                arguments: serde_json::Value::Object(arguments),
                tool_call_id: None,
                parse_error: false,
            });
        }
    }

    calls
}

fn map_tool_name_alias(tool_name: &str) -> &str {
    match tool_name {

        "shell" | "bash" | "sh" | "exec" | "command" | "cmd" => "shell",

        "browser_open" | "browser" | "web_search" => "http_request",

        "send_message" | "sendmessage" => "message_send",

        "fileread" | "file_read" | "readfile" | "read_file" | "file" => "file_read",
        "filewrite" | "file_write" | "writefile" | "write_file" => "file_write",
        "filelist" | "file_list" | "listfiles" | "list_files" => "file_list",

        "memoryrecall" | "memory_recall" | "recall" | "memrecall" => "memory_recall",
        "memorystore" | "memory_store" | "store" | "memstore" => "memory_store",
        "memoryforget" | "memory_forget" | "forget" | "memforget" => "memory_forget",

        "http_request" | "http" | "fetch" | "curl" | "wget" => "http_request",
        _ => tool_name,
    }
}

fn is_url_like(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

fn parse_glm_style_tool_calls(text: &str) -> Vec<(String, serde_json::Value, Option<String>)> {
    let mut calls = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(pos) = line.find('/') {
            let tool_part = &line[..pos];
            let rest = &line[pos + 1..];

            if tool_part.chars().all(|c| c.is_alphanumeric() || c == '_') {
                let tool_name = map_tool_name_alias(tool_part);

                if let Some(gt_pos) = rest.find('>') {
                    let param_name = rest[..gt_pos].trim();
                    let value = rest[gt_pos + 1..].trim();

                    let (final_tool, arguments) = match tool_name {
                        "shell" => {
                            if param_name == "url" || is_url_like(value) {
                                (
                                    "http_request",
                                    serde_json::json!({"url": value, "method": "GET"}),
                                )
                            } else {
                                ("shell", serde_json::json!({ "command": value }))
                            }
                        }
                        "http_request" => (
                            "http_request",
                            serde_json::json!({"url": value, "method": "GET"}),
                        ),
                        _ => (tool_name, serde_json::json!({ param_name: value })),
                    };

                    calls.push((final_tool.to_string(), arguments, Some(line.to_string())));
                    continue;
                }

                if rest.starts_with('{') {
                    if let Ok(json_args) = serde_json::from_str::<serde_json::Value>(rest) {
                        calls.push((tool_name.to_string(), json_args, Some(line.to_string())));
                    }
                }
            }
        }
    }

    calls
}

fn default_param_for_tool(tool: &str) -> &'static str {
    match tool {
        "shell" | "bash" | "sh" | "exec" | "command" | "cmd" => "command",

        "file_read" | "fileread" | "readfile" | "read_file" | "file" | "file_write"
        | "filewrite" | "writefile" | "write_file" | "file_edit" | "fileedit" | "editfile"
        | "edit_file" | "file_list" | "filelist" | "listfiles" | "list_files" => "path",

        "memory_recall" | "memoryrecall" | "recall" | "memrecall" | "memory_forget"
        | "memoryforget" | "forget" | "memforget" | "web_search_tool" | "web_search"
        | "websearch" | "search" => "query",
        "memory_store" | "memorystore" | "store" | "memstore" => "content",

        "http_request" | "http" | "fetch" | "curl" | "wget" | "browser_open" | "browser" => "url",
        _ => "input",
    }
}

fn parse_glm_shortened_body(body: &str) -> Option<ParsedToolCall> {
    let body = body.trim();
    if body.is_empty() {
        return None;
    }

    let function_style = body.find('(').and_then(|open| {
        if body.ends_with(')') && open > 0 {
            Some((body[..open].trim(), body[open + 1..body.len() - 1].trim()))
        } else {
            None
        }
    });

    let (tool_raw, value_part) = if let Some((tool, args)) = function_style {
        (tool, args)
    } else if body.contains("=\"") {

        let split_pos = body.find(|c: char| c.is_whitespace()).unwrap_or(body.len());
        let tool = body[..split_pos].trim();
        let attrs = body[split_pos..]
            .trim()
            .trim_end_matches("/>")
            .trim_end_matches('>')
            .trim_end_matches('/')
            .trim();
        (tool, attrs)
    } else if let Some(gt_pos) = body.find('>') {

        let tool = body[..gt_pos].trim();
        let value = body[gt_pos + 1..].trim();

        let value = value.trim_end_matches("/>").trim_end_matches('/').trim();
        (tool, value)
    } else {
        return None;
    };

    let tool_raw = tool_raw.trim_end_matches(|c: char| c.is_whitespace());
    if tool_raw.is_empty() || !tool_raw.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }

    let tool_name = map_tool_name_alias(tool_raw);

    if value_part.contains("=\"") {
        let mut args = serde_json::Map::new();

        let mut rest = value_part;
        while let Some(eq_pos) = rest.find("=\"") {
            let key_start = rest[..eq_pos]
                .rfind(|c: char| c.is_whitespace())
                .map(|p| p + 1)
                .unwrap_or(0);
            let key = rest[key_start..eq_pos]
                .trim()
                .trim_matches(|c: char| c == ',' || c == ';');
            let after_quote = &rest[eq_pos + 2..];
            if let Some(end_quote) = after_quote.find('"') {
                let value = &after_quote[..end_quote];
                if !key.is_empty() {
                    args.insert(
                        key.to_string(),
                        serde_json::Value::String(value.to_string()),
                    );
                }
                rest = &after_quote[end_quote + 1..];
            } else {
                break;
            }
        }
        if !args.is_empty() {
            return Some(ParsedToolCall {
                name: tool_name.to_string(),
                arguments: serde_json::Value::Object(args),
                tool_call_id: None,
                parse_error: false,
            });
        }
    }

    if value_part.contains('\n') {
        let mut args = serde_json::Map::new();
        for line in value_part.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(colon_pos) = line.find(':') {
                let key = line[..colon_pos].trim();
                let value = line[colon_pos + 1..].trim();
                if !key.is_empty() && !value.is_empty() {

                    let json_value = match value {
                        "true" | "yes" => serde_json::Value::Bool(true),
                        "false" | "no" => serde_json::Value::Bool(false),
                        _ => serde_json::Value::String(value.to_string()),
                    };
                    args.insert(key.to_string(), json_value);
                }
            }
        }
        if !args.is_empty() {
            return Some(ParsedToolCall {
                name: tool_name.to_string(),
                arguments: serde_json::Value::Object(args),
                tool_call_id: None,
                parse_error: false,
            });
        }
    }

    if !value_part.is_empty() {
        let param = default_param_for_tool(tool_raw);
        let arguments = match tool_name {
            "shell" => {
                if is_url_like(value_part) {
                    return Some(ParsedToolCall {
                        name: "http_request".to_string(),
                        arguments: serde_json::json!({"url": value_part, "method": "GET"}),
                        tool_call_id: None,
                        parse_error: false,
                    });
                }
                serde_json::json!({ "command": value_part })
            }
            "http_request" => serde_json::json!({"url": value_part, "method": "GET"}),
            _ => serde_json::json!({ param: value_part }),
        };
        return Some(ParsedToolCall {
            name: tool_name.to_string(),
            arguments,
            tool_call_id: None,
            parse_error: false,
        });
    }

    None
}

fn parse_tool_calls(response: &str) -> (String, Vec<ParsedToolCall>) {

    let cleaned = strip_think_tags(response);
    let response = cleaned.as_str();

    let mut text_parts = Vec::new();
    let mut calls = Vec::new();
    let mut remaining = response;

    if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(response.trim()) {
        calls = parse_tool_calls_from_json_value(&json_value);
        if !calls.is_empty() {

            if let Some(content) = json_value.get("content").and_then(|v| v.as_str()) {
                if !content.trim().is_empty() {
                    text_parts.push(content.trim().to_string());
                }
            }
            return (text_parts.join("\n"), calls);
        }
    }

    if let Some((minimax_text, minimax_calls)) = parse_minimax_invoke_calls(response) {
        if !minimax_calls.is_empty() {
            return (minimax_text, minimax_calls);
        }
    }

    while let Some((start, open_tag)) = find_first_tag(remaining, &TOOL_CALL_OPEN_TAGS) {

        let before = &remaining[..start];
        if !before.trim().is_empty() {
            text_parts.push(before.trim().to_string());
        }

        let Some(close_tag) = matching_tool_call_close_tag(open_tag) else {
            break;
        };

        let after_open = &remaining[start + open_tag.len()..];
        if let Some(close_idx) = after_open.find(close_tag) {
            let inner = &after_open[..close_idx];
            let mut parsed_any = false;

            let json_values = extract_json_values(inner);
            for value in json_values {
                let parsed_calls = parse_tool_calls_from_json_value(&value);
                if !parsed_calls.is_empty() {
                    parsed_any = true;
                    calls.extend(parsed_calls);
                }
            }

            if !parsed_any {
                if let Some(xml_calls) = parse_xml_tool_calls(inner) {
                    calls.extend(xml_calls);
                    parsed_any = true;
                }
            }

            if !parsed_any {

                if let Some(glm_call) = parse_glm_shortened_body(inner) {
                    calls.push(glm_call);
                    parsed_any = true;
                }
            }

            if !parsed_any {
                tracing::warn!(
                    "Malformed <tool_call>: expected tool-call object in tag body (JSON/XML/GLM)"
                );
            }

            remaining = &after_open[close_idx + close_tag.len()..];
        } else {

            let mut resolved = false;
            if let Some((cross_idx, cross_tag)) = find_first_tag(after_open, &TOOL_CALL_CLOSE_TAGS)
            {
                let inner = &after_open[..cross_idx];
                let mut parsed_any = false;

                let json_values = extract_json_values(inner);
                for value in json_values {
                    let parsed_calls = parse_tool_calls_from_json_value(&value);
                    if !parsed_calls.is_empty() {
                        parsed_any = true;
                        calls.extend(parsed_calls);
                    }
                }

                if !parsed_any {
                    if let Some(xml_calls) = parse_xml_tool_calls(inner) {
                        calls.extend(xml_calls);
                        parsed_any = true;
                    }
                }

                if !parsed_any {
                    if let Some(glm_call) = parse_glm_shortened_body(inner) {
                        calls.push(glm_call);
                        parsed_any = true;
                    }
                }

                if parsed_any {
                    remaining = &after_open[cross_idx + cross_tag.len()..];
                    resolved = true;
                }
            }

            if resolved {
                continue;
            }

            if let Some(json_end) = find_json_end(after_open) {
                if let Ok(value) =
                    serde_json::from_str::<serde_json::Value>(&after_open[..json_end])
                {
                    let parsed_calls = parse_tool_calls_from_json_value(&value);
                    if !parsed_calls.is_empty() {
                        calls.extend(parsed_calls);
                        remaining = strip_leading_close_tags(&after_open[json_end..]);
                        continue;
                    }
                }
            }

            if let Some((value, consumed_end)) = extract_first_json_value_with_end(after_open) {
                let parsed_calls = parse_tool_calls_from_json_value(&value);
                if !parsed_calls.is_empty() {
                    calls.extend(parsed_calls);
                    remaining = strip_leading_close_tags(&after_open[consumed_end..]);
                    continue;
                }
            }

            let glm_input = after_open.trim();
            if let Some(glm_call) = parse_glm_shortened_body(glm_input) {
                calls.push(glm_call);
                remaining = "";
                continue;
            }

            remaining = &remaining[start..];
            break;
        }
    }

    if calls.is_empty() {
        static MD_TOOL_CALL_RE: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(
                r"(?s)```(?:tool[_-]?call|invoke)\s*\n(.*?)(?:```|</tool[_-]?call>|</toolcall>|</invoke>|</minimax:toolcall>)",
            )
            .unwrap()
        });
        let mut md_text_parts: Vec<String> = Vec::new();
        let mut last_end = 0;

        for cap in MD_TOOL_CALL_RE.captures_iter(response) {
            let Some(full_match) = cap.get(0) else {
                continue;
            };
            let before = &response[last_end..full_match.start()];
            if !before.trim().is_empty() {
                md_text_parts.push(before.trim().to_string());
            }
            let inner = &cap[1];
            let json_values = extract_json_values(inner);
            for value in json_values {
                let parsed_calls = parse_tool_calls_from_json_value(&value);
                calls.extend(parsed_calls);
            }
            last_end = full_match.end();
        }

        if !calls.is_empty() {
            let after = &response[last_end..];
            if !after.trim().is_empty() {
                md_text_parts.push(after.trim().to_string());
            }
            text_parts = md_text_parts;
            remaining = "";
        }
    }

    if calls.is_empty() {
        static MD_TOOL_NAME_RE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"(?s)```tool\s+(\w+)\s*\n(.*?)(?:```|$)").unwrap());
        let mut md_text_parts: Vec<String> = Vec::new();
        let mut last_end = 0;

        for cap in MD_TOOL_NAME_RE.captures_iter(response) {
            let Some(full_match) = cap.get(0) else {
                continue;
            };
            let before = &response[last_end..full_match.start()];
            if !before.trim().is_empty() {
                md_text_parts.push(before.trim().to_string());
            }
            let tool_name = &cap[1];
            let inner = &cap[2];

            let json_values = extract_json_values(inner);
            if json_values.is_empty() {

                tracing::warn!(
                    tool_name = %tool_name,
                    inner = %inner.chars().take(100).collect::<String>(),
                    "Found ```tool <name> block but could not parse JSON arguments"
                );
            } else {
                for value in json_values {
                    let arguments = if value.is_object() {
                        value
                    } else {
                        serde_json::Value::Object(serde_json::Map::new())
                    };
                    calls.push(ParsedToolCall {
                        name: tool_name.to_string(),
                        arguments,
                        tool_call_id: None,
                        parse_error: false,
                    });
                }
            }
            last_end = full_match.end();
        }

        if !calls.is_empty() {
            let after = &response[last_end..];
            if !after.trim().is_empty() {
                md_text_parts.push(after.trim().to_string());
            }
            text_parts = md_text_parts;
            remaining = "";
        }
    }

    if calls.is_empty() {
        let xml_calls = parse_xml_attribute_tool_calls(remaining);
        if !xml_calls.is_empty() {
            let mut cleaned_text = remaining.to_string();
            for call in xml_calls {
                calls.push(call);

                if let Some(start) = cleaned_text.find("<minimax:toolcall>") {
                    if let Some(end) = cleaned_text.find("</minimax:toolcall>") {
                        let end_pos = end + "</minimax:toolcall>".len();
                        if end_pos <= cleaned_text.len() {
                            cleaned_text =
                                format!("{}{}", &cleaned_text[..start], &cleaned_text[end_pos..]);
                        }
                    }
                }
            }
            if !cleaned_text.trim().is_empty() {
                text_parts.push(cleaned_text.trim().to_string());
            }
            remaining = "";
        }
    }

    if calls.is_empty() {
        let perl_calls = parse_perl_style_tool_calls(remaining);
        if !perl_calls.is_empty() {
            let mut cleaned_text = remaining.to_string();
            for call in perl_calls {
                calls.push(call);

                while let Some(start) = cleaned_text.find("TOOL_CALL") {
                    if let Some(end) = cleaned_text.find("/TOOL_CALL") {
                        let end_pos = end + "/TOOL_CALL".len();
                        if end_pos <= cleaned_text.len() {
                            cleaned_text =
                                format!("{}{}", &cleaned_text[..start], &cleaned_text[end_pos..]);
                        }
                    } else {
                        break;
                    }
                }
            }
            if !cleaned_text.trim().is_empty() {
                text_parts.push(cleaned_text.trim().to_string());
            }
            remaining = "";
        }
    }

    if calls.is_empty() {
        let func_calls = parse_function_call_tool_calls(remaining);
        if !func_calls.is_empty() {
            let mut cleaned_text = remaining.to_string();
            for call in func_calls {
                calls.push(call);

                while let Some(start) = cleaned_text.find("<FunctionCall>") {
                    if let Some(end) = cleaned_text.find("</FunctionCall>") {
                        let end_pos = end + "</FunctionCall>".len();
                        if end_pos <= cleaned_text.len() {
                            cleaned_text =
                                format!("{}{}", &cleaned_text[..start], &cleaned_text[end_pos..]);
                        }
                    } else {
                        break;
                    }
                }
            }
            if !cleaned_text.trim().is_empty() {
                text_parts.push(cleaned_text.trim().to_string());
            }
            remaining = "";
        }
    }

    if calls.is_empty() {
        let glm_calls = parse_glm_style_tool_calls(remaining);
        if !glm_calls.is_empty() {
            let mut cleaned_text = remaining.to_string();
            for (name, args, raw) in &glm_calls {
                calls.push(ParsedToolCall {
                    name: name.clone(),
                    arguments: args.clone(),
                    tool_call_id: None,
                    parse_error: false,
                });
                if let Some(r) = raw {
                    cleaned_text = cleaned_text.replace(r, "");
                }
            }
            if !cleaned_text.trim().is_empty() {
                text_parts.push(cleaned_text.trim().to_string());
            }
            remaining = "";
        }
    }

    if !remaining.trim().is_empty() {
        text_parts.push(remaining.trim().to_string());
    }

    (text_parts.join("\n"), calls)
}

fn strip_think_tags(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut rest = s;
    loop {
        if let Some(start) = rest.find("<think>") {
            result.push_str(&rest[..start]);
            if let Some(end) = rest[start..].find("</think>") {
                rest = &rest[start + end + "</think>".len()..];
            } else {

                break;
            }
        } else {
            result.push_str(rest);
            break;
        }
    }
    result.trim().to_string()
}

fn strip_tool_result_blocks(text: &str) -> String {
    static TOOL_RESULT_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)<tool_result[^>]*>.*?</tool_result>").unwrap());
    static THINKING_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)<thinking>.*?</thinking>").unwrap());
    static THINK_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)<think>.*?</think>").unwrap());
    static TOOL_RESULTS_PREFIX_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?m)^\[Tool results\]\s*\n?").unwrap());
    static EXCESS_BLANK_LINES_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());

    let result = TOOL_RESULT_RE.replace_all(text, "");
    let result = THINKING_RE.replace_all(&result, "");
    let result = THINK_RE.replace_all(&result, "");
    let result = TOOL_RESULTS_PREFIX_RE.replace_all(&result, "");
    let result = EXCESS_BLANK_LINES_RE.replace_all(result.trim(), "\n\n");

    result.trim().to_string()
}

fn detect_tool_call_parse_issue(response: &str, parsed_calls: &[ParsedToolCall]) -> Option<String> {
    if !parsed_calls.is_empty() {
        return None;
    }

    let trimmed = response.trim();
    if trimmed.is_empty() {
        return None;
    }

    let looks_like_tool_payload = trimmed.contains("<tool_call")
        || trimmed.contains("<toolcall")
        || trimmed.contains("<tool-call")
        || trimmed.contains("```tool_call")
        || trimmed.contains("```toolcall")
        || trimmed.contains("```tool-call")
        || trimmed.contains("```tool file_")
        || trimmed.contains("```tool shell")
        || trimmed.contains("```tool web_")
        || trimmed.contains("```tool memory_")
        || trimmed.contains("```tool ")
        || trimmed.contains("\"tool_calls\"")
        || trimmed.contains("TOOL_CALL")
        || trimmed.contains("[TOOL_CALL]")
        || trimmed.contains("<FunctionCall>");

    if looks_like_tool_payload {
        Some("response resembled a tool-call payload but no valid tool call could be parsed".into())
    } else {
        None
    }
}

fn parse_structured_tool_calls(tool_calls: &[ToolCall]) -> Vec<ParsedToolCall> {
    tool_calls
        .iter()
        .map(|call| ParsedToolCall {
            name: call.name.clone(),
            arguments: serde_json::from_str::<serde_json::Value>(&call.arguments)
                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
            tool_call_id: Some(call.id.clone()),
            parse_error: serde_json::from_str::<serde_json::Value>(&call.arguments).is_err(),
        })
        .collect()
}

fn build_native_assistant_history(
    text: &str,
    tool_calls: &[ToolCall],
    reasoning_content: Option<&str>,
) -> String {
    let calls_json: Vec<serde_json::Value> = tool_calls
        .iter()
        .map(|tc| {
            serde_json::json!({
                "id": tc.id,
                "name": tc.name,
                "arguments": tc.arguments,
            })
        })
        .collect();

    let content = if text.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(text.trim().to_string())
    };

    let mut obj = serde_json::json!({
        "content": content,
        "tool_calls": calls_json,
    });

    if let Some(rc) = reasoning_content {
        obj.as_object_mut().unwrap().insert(
            "reasoning_content".to_string(),
            serde_json::Value::String(rc.to_string()),
        );
    }

    obj.to_string()
}

fn build_native_assistant_history_from_parsed_calls(
    text: &str,
    tool_calls: &[ParsedToolCall],
    reasoning_content: Option<&str>,
) -> Option<String> {
    let calls_json = tool_calls
        .iter()
        .map(|tc| {
            Some(serde_json::json!({
                "id": tc.tool_call_id.clone()?,
                "name": tc.name,
                "arguments": serde_json::to_string(&tc.arguments).unwrap_or_else(|_| "{}".to_string()),
            }))
        })
        .collect::<Option<Vec<_>>>()?;

    let content = if text.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(text.trim().to_string())
    };

    let mut obj = serde_json::json!({
        "content": content,
        "tool_calls": calls_json,
    });

    if let Some(rc) = reasoning_content {
        obj.as_object_mut().unwrap().insert(
            "reasoning_content".to_string(),
            serde_json::Value::String(rc.to_string()),
        );
    }

    Some(obj.to_string())
}

fn build_assistant_history_with_tool_calls(text: &str, tool_calls: &[ToolCall]) -> String {
    let mut parts = Vec::new();

    if !text.trim().is_empty() {
        parts.push(text.trim().to_string());
    }

    for call in tool_calls {
        let arguments = serde_json::from_str::<serde_json::Value>(&call.arguments)
            .unwrap_or_else(|_| serde_json::Value::String(call.arguments.clone()));
        let payload = serde_json::json!({
            "id": call.id,
            "name": call.name,
            "arguments": arguments,
        });
        parts.push(format!("<tool_call>\n{payload}\n</tool_call>"));
    }

    parts.join("\n")
}

fn resolve_display_text(
    response_text: &str,
    parsed_text: &str,
    has_tool_calls: bool,
    has_native_tool_calls: bool,
) -> String {
    if has_tool_calls {
        if !parsed_text.is_empty() {
            return parsed_text.to_string();
        }
        if has_native_tool_calls {
            return response_text.to_string();
        }
        return String::new();
    }

    if parsed_text.is_empty() {
        response_text.to_string()
    } else {
        parsed_text.to_string()
    }
}

#[derive(Debug, Clone)]
struct ParsedToolCall {
    name: String,
    arguments: serde_json::Value,
    tool_call_id: Option<String>,

    parse_error: bool,
}

#[derive(Debug)]
pub(crate) struct ToolLoopCancelled;

impl std::fmt::Display for ToolLoopCancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("tool loop cancelled")
    }
}

impl std::error::Error for ToolLoopCancelled {}

pub(crate) fn is_tool_loop_cancelled(err: &anyhow::Error) -> bool {
    err.chain().any(|source| source.is::<ToolLoopCancelled>())
}

#[derive(Debug)]
pub(crate) struct ModelSwitchRequested {
    pub provider: String,
    pub model: String,
}

impl std::fmt::Display for ModelSwitchRequested {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "model switch requested to {} {}",
            self.provider, self.model
        )
    }
}

impl std::error::Error for ModelSwitchRequested {}

pub(crate) fn is_model_switch_requested(err: &anyhow::Error) -> Option<(String, String)> {
    err.chain()
        .filter_map(|source| source.downcast_ref::<ModelSwitchRequested>())
        .map(|e| (e.provider.clone(), e.model.clone()))
        .next()
}

#[derive(Debug, Default)]
struct StreamedChatOutcome {
    response_text: String,
    tool_calls: Vec<ToolCall>,

    reasoning_content: String,

    usage: Option<crate::providers::traits::TokenUsage>,
    forwarded_live_deltas: bool,
}

fn looks_like_streamed_tool_payload(window: &str) -> bool {
    crate::agent::streaming_markers::find_tool_marker(window).is_some()
}

async fn call_provider_chat(
    provider: &dyn Provider,
    messages: &[ChatMessage],
    request_tools: Option<&[crate::tools::ToolSpec]>,
    model: &str,
    temperature: f64,
    cancellation_token: Option<&CancellationToken>,
) -> Result<crate::providers::ChatResponse> {
    let chat_future = provider.chat(
        ChatRequest {
            messages,
            tools: request_tools,
        },
        model,
        temperature,
    );

    if let Some(token) = cancellation_token {
        tokio::select! {
            () = token.cancelled() => Err(ToolLoopCancelled.into()),
            result = chat_future => result,
        }
    } else {
        chat_future.await
    }
}

async fn consume_provider_streaming_response(
    provider: &dyn Provider,
    messages: &[ChatMessage],
    request_tools: Option<&[crate::tools::ToolSpec]>,
    model: &str,
    temperature: f64,
    cancellation_token: Option<&CancellationToken>,
    on_delta: Option<&tokio::sync::mpsc::Sender<DraftEvent>>,
) -> Result<StreamedChatOutcome> {
    let mut provider_stream = provider.stream_chat(
        ChatRequest {
            messages,
            tools: request_tools,
        },
        model,
        temperature,
        crate::providers::traits::StreamOptions::new(true),
    );
    let mut outcome = StreamedChatOutcome::default();
    let mut delta_sender = on_delta;
    let mut suppress_forwarding = false;
    let mut marker_window = String::new();

    loop {
        let next_chunk = if let Some(token) = cancellation_token {
            tokio::select! {
                () = token.cancelled() => return Err(ToolLoopCancelled.into()),
                chunk = provider_stream.next() => chunk,
            }
        } else {
            provider_stream.next().await
        };

        let Some(event_result) = next_chunk else {
            break;
        };

        let event = event_result.map_err(|err| anyhow::anyhow!("provider stream error: {err}"))?;
        match event {
            StreamEvent::Final => break,
            StreamEvent::ToolCall(tool_call) => {
                outcome.tool_calls.push(tool_call);
                suppress_forwarding = true;
                if outcome.forwarded_live_deltas {
                    if let Some(tx) = delta_sender {
                        let _ = tx.send(DraftEvent::Clear).await;
                    }
                    outcome.forwarded_live_deltas = false;
                }
            }
            StreamEvent::PreExecutedToolCall { .. } | StreamEvent::PreExecutedToolResult { .. } => {

            }
            StreamEvent::Usage(usage) => {

                outcome.usage = Some(usage);
            }
            StreamEvent::TextDelta(chunk) => {

                if let Some(rc) = &chunk.reasoning {
                    if !rc.is_empty() {
                        outcome.reasoning_content.push_str(rc);
                    }
                }

                if chunk.delta.is_empty() {
                    continue;
                }

                outcome.response_text.push_str(&chunk.delta);
                marker_window.push_str(&chunk.delta);

                if marker_window.len() > STREAM_TOOL_MARKER_WINDOW_CHARS {
                    let keep_from = marker_window.len() - STREAM_TOOL_MARKER_WINDOW_CHARS;
                    let boundary = marker_window
                        .char_indices()
                        .find(|(idx, _)| *idx >= keep_from)
                        .map_or(0, |(idx, _)| idx);
                    marker_window.drain(..boundary);
                }

                if !suppress_forwarding && looks_like_streamed_tool_payload(&marker_window) {
                    suppress_forwarding = true;
                    if outcome.forwarded_live_deltas {
                        if let Some(tx) = delta_sender {
                            let _ = tx.send(DraftEvent::Clear).await;
                        }
                        outcome.forwarded_live_deltas = false;
                    }
                }

                if suppress_forwarding && !looks_like_streamed_tool_payload(&marker_window) {
                    suppress_forwarding = false;
                }

                if suppress_forwarding {
                    continue;
                }

                if let Some(tx) = delta_sender {
                    if !outcome.forwarded_live_deltas {
                        let _ = tx.send(DraftEvent::Clear).await;
                        outcome.forwarded_live_deltas = true;
                    }
                    if tx.send(DraftEvent::Content(chunk.delta)).await.is_err() {
                        delta_sender = None;
                    }
                }
            }
        }
    }

    Ok(outcome)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn agent_turn(
    provider: &dyn Provider,
    history: &mut Vec<ChatMessage>,
    tools_registry: &[Box<dyn Tool>],
    observer: &dyn Observer,
    provider_name: &str,
    model: &str,
    temperature: f64,
    silent: bool,
    channel_name: &str,
    channel_reply_target: Option<&str>,
    multimodal_config: &crate::config::MultimodalConfig,
    max_tool_iterations: usize,
    approval: Option<&ApprovalManager>,
    excluded_tools: &[String],
    dedup_exempt_tools: &[String],
    activated_tools: Option<&std::sync::Arc<parking_lot::Mutex<crate::tools::ActivatedToolSet>>>,
    model_switch_callback: Option<ModelSwitchCallback>,
) -> Result<String> {
    run_tool_call_loop(
        provider,
        history,
        tools_registry,
        observer,
        provider_name,
        model,
        temperature,
        silent,
        approval,
        channel_name,
        channel_reply_target,
        multimodal_config,
        max_tool_iterations,
        None,
        None,
        None,
        excluded_tools,
        dedup_exempt_tools,
        activated_tools,
        model_switch_callback,
        &crate::config::PacingConfig::default(),
        None,
        None,
        None,
        None,
    )
    .await
}

fn maybe_inject_channel_delivery_defaults(
    tool_name: &str,
    tool_args: &mut serde_json::Value,
    channel_name: &str,
    channel_reply_target: Option<&str>,
) {
    if tool_name != "cron_add" {
        return;
    }

    if !matches!(
        channel_name,
        "telegram" | "discord" | "slack" | "mattermost" | "matrix"
    ) {
        return;
    }

    let Some(reply_target) = channel_reply_target
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };

    let Some(args) = tool_args.as_object_mut() else {
        return;
    };

    let is_agent_job = args
        .get("job_type")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|job_type| job_type.eq_ignore_ascii_case("agent"))
        || args
            .get("prompt")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|prompt| !prompt.trim().is_empty());
    if !is_agent_job {
        return;
    }

    let default_delivery = || {
        serde_json::json!({
            "mode": "announce",
            "channel": channel_name,
            "to": reply_target,
        })
    };

    match args.get_mut("delivery") {
        None => {
            args.insert("delivery".to_string(), default_delivery());
        }
        Some(serde_json::Value::Null) => {
            *args.get_mut("delivery").expect("delivery key exists") = default_delivery();
        }
        Some(serde_json::Value::Object(delivery)) => {
            if delivery
                .get("mode")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|mode| mode.eq_ignore_ascii_case("none"))
            {
                return;
            }

            delivery
                .entry("mode".to_string())
                .or_insert_with(|| serde_json::Value::String("announce".to_string()));

            let needs_channel = delivery
                .get("channel")
                .and_then(serde_json::Value::as_str)
                .is_none_or(|value| value.trim().is_empty());
            if needs_channel {
                delivery.insert(
                    "channel".to_string(),
                    serde_json::Value::String(channel_name.to_string()),
                );
            }

            let needs_target = delivery
                .get("to")
                .and_then(serde_json::Value::as_str)
                .is_none_or(|value| value.trim().is_empty());
            if needs_target {
                delivery.insert(
                    "to".to_string(),
                    serde_json::Value::String(reply_target.to_string()),
                );
            }
        }
        Some(_) => {}
    }
}

async fn execute_one_tool(
    call_name: &str,
    call_arguments: serde_json::Value,
    tools_registry: &[Box<dyn Tool>],
    tool_registry: Option<&ToolRegistry>,
    activated_tools: Option<&std::sync::Arc<parking_lot::Mutex<crate::tools::ActivatedToolSet>>>,
    observer: &dyn Observer,
    cancellation_token: Option<&CancellationToken>,
    rbac_engine: Option<&std::sync::Arc<crate::security::rbac::RbacEngine>>,
    rbac_identity: Option<&crate::security::rbac::CallerIdentity>,
) -> Result<ToolExecutionOutcome> {

    tracing::debug!(
        target: "tool.execute",
        tool = %call_name,
        "tool call start"
    );

    let args_summary = truncate_with_ellipsis(&call_arguments.to_string(), 300);
    observer.record_event(&ObserverEvent::ToolCallStart {
        tool: call_name.to_string(),
        arguments: Some(args_summary),
    });
    let start = Instant::now();

    let static_handle = find_tool(tools_registry, call_name, tool_registry);
    let activated_arc = if static_handle.is_none() {
        activated_tools.and_then(|at| at.lock().get_resolved(call_name))
    } else {
        None
    };
    let tool_ref: Option<&dyn Tool> = match (&static_handle, &activated_arc) {
        (Some(h), _) => Some(h.as_tool()),
        (None, Some(a)) => Some(a.as_ref()),
        (None, None) => None,
    };
    let Some(tool) = tool_ref else {
        let reason = format!("Unknown tool: {call_name}");
        let duration = start.elapsed();
        observer.record_event(&ObserverEvent::ToolCall {
            tool: call_name.to_string(),
            duration,
            success: false,
        });
        return Ok(ToolExecutionOutcome {
            output: reason.clone(),
            success: false,
            error_reason: Some(scrub_credentials(&reason)),
            duration,
        });
    };

    if let (Some(engine), Some(identity)) = (rbac_engine, rbac_identity) {
        let auth = engine.authorize_tool(identity, call_name);
        if !auth.allowed {
            let duration = start.elapsed();
            let reason = auth
                .reason
                .unwrap_or_else(|| "Tool not permitted for this identity".into());
            observer.record_event(&ObserverEvent::ToolCall {
                tool: call_name.to_string(),
                duration,
                success: false,
            });
            return Ok(ToolExecutionOutcome {
                output: format!("RBAC denied: {reason}"),
                success: false,
                error_reason: Some(reason),
                duration,
            });
        }
    }

    let coding_label = crate::services::try_get_services()
        .map(|svc| svc.coding_mode.read().label().to_string());
    let coding_label_lc = coding_label.as_deref().map(str::to_ascii_lowercase);
    let perm_mode_lc =
        crate::gateway::ws_desktop::desktop_runtime_state().permission_mode();
    let tool_lc = call_name.to_ascii_lowercase();
    let guardrail_ctx = crate::guardrails::GuardrailContext {
        coding_mode: coding_label_lc.as_deref(),
        permission_mode: Some(&perm_mode_lc),
        tool_name: Some(&tool_lc),
    };
    if let Err(reason) =
        crate::guardrails::check_tool_guardrails(call_name, Some(&guardrail_ctx))
    {
        let duration = start.elapsed();
        observer.record_event(&ObserverEvent::ToolCall {
            tool: call_name.to_string(),
            duration,
            success: false,
        });
        return Ok(ToolExecutionOutcome {
            output: format!("Blocked by guardrails: {reason}"),
            success: false,
            error_reason: Some(reason),
            duration,
        });
    }

    let cache_fp = tool.fingerprint(&call_arguments);
    if let Some(fp) = cache_fp.as_ref() {
        if let Some(entry) =
            crate::agent::turn_engine::cache_bind::try_tool_cache_hit(call_name, fp)
        {
            let duration = start.elapsed();
            observer.record_event(&ObserverEvent::ToolCall {
                tool: call_name.to_string(),
                duration,
                success: true,
            });
            if let Some(svc) = crate::services::try_get_services() {
                crate::observability::agent_metrics::inc_tool_call(
                    &svc.agent_metrics,
                    call_name,
                    "cache_hit",
                );
            }
            return Ok(ToolExecutionOutcome {
                output: entry.output,
                success: true,
                error_reason: None,
                duration,
            });
        }
    }

    let tool_future = tool.execute(call_arguments);
    let tool_result = if let Some(token) = cancellation_token {
        tokio::select! {
            () = token.cancelled() => return Err(ToolLoopCancelled.into()),
            result = tool_future => result,
        }
    } else {
        tool_future.await
    };

    if let Some(svc) = crate::services::try_get_services() {
        let status = if tool_result.is_ok() {
            "success"
        } else {
            "error"
        };
        crate::observability::agent_metrics::inc_tool_call(&svc.agent_metrics, call_name, status);
    }

    match tool_result {
        Ok(r) => {
            let duration = start.elapsed();
            observer.record_event(&ObserverEvent::ToolCall {
                tool: call_name.to_string(),
                duration,
                success: r.success,
            });
            if r.success {
                let scrubbed = scrub_credentials(&r.output);
                let compressed = {
                    let call_name_owned = call_name.to_string();
                    tokio::task::spawn_blocking(move || {
                        crate::agent::token_optimizer::compress_output(
                            &call_name_owned,
                            &scrubbed,
                        )
                    })
                    .await
                    .unwrap_or_else(|_| String::new())
                };

                if let Some(fp) = cache_fp.as_ref() {
                    crate::agent::turn_engine::cache_bind::write_tool_cache(
                        call_name,
                        fp,
                        compressed.clone(),
                        tool.cache_ttl_secs(),
                    );
                }
                Ok(ToolExecutionOutcome {
                    output: compressed,
                    success: true,
                    error_reason: None,
                    duration,
                })
            } else {
                let reason = r.error.unwrap_or(r.output);
                Ok(ToolExecutionOutcome {
                    output: format!("Error: {reason}"),
                    success: false,
                    error_reason: Some(scrub_credentials(&reason)),
                    duration,
                })
            }
        }
        Err(e) => {
            let duration = start.elapsed();
            observer.record_event(&ObserverEvent::ToolCall {
                tool: call_name.to_string(),
                duration,
                success: false,
            });

            let _class =
                crate::agent::turn_engine::recovery_bind::classify_and_trace(call_name, &e);
            let reason = format!("Error executing {call_name}: {e}");
            Ok(ToolExecutionOutcome {
                output: reason.clone(),
                success: false,
                error_reason: Some(scrub_credentials(&reason)),
                duration,
            })
        }
    }
}

struct ToolExecutionOutcome {
    output: String,
    success: bool,
    error_reason: Option<String>,
    duration: Duration,
}

fn resolve_parallel_tool_cap() -> usize {
    if let Some(svc) = crate::services::try_get_services() {
        let cap = svc.config().agent_runtime.parallel_tool_max_concurrency;
        return (cap as usize).max(1);
    }
    8
}

fn resolve_self_consistency_config()
-> crate::config::domain::agent_runtime::SelfConsistencyConfig {
    if let Some(svc) = crate::services::try_get_services() {
        return svc.config().agent_runtime.self_consistency.clone();
    }
    crate::config::domain::agent_runtime::SelfConsistencyConfig::default()
}

async fn run_self_consistency_resampling(
    cfg: &crate::config::domain::agent_runtime::SelfConsistencyConfig,
    provider: &dyn Provider,
    messages: &[ChatMessage],
    request_tools: Option<&[crate::tools::traits::ToolSpec]>,
    model: &str,
    cancellation_token: Option<&CancellationToken>,
    initial_text: String,
) -> (String, f64, bool, usize) {
    let extras = (cfg.samples as usize).saturating_sub(1);
    if extras == 0 {
        return (initial_text, 1.0, false, 1);
    }
    let cap = (cfg.effective_concurrency() as usize).max(1);
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(cap));
    let temperature = cfg.temperature;

    let futures: Vec<_> = (0..extras)
        .map(|_| {
            let sem = semaphore.clone();
            async move {
                let _permit = sem.acquire_owned().await.ok()?;
                let chat_future = provider.chat(
                    ChatRequest {
                        messages,
                        tools: request_tools,
                    },
                    model,
                    temperature,
                );
                let outcome = if let Some(token) = cancellation_token {
                    tokio::select! {
                        () = token.cancelled() => return None,
                        result = chat_future => result,
                    }
                } else {
                    chat_future.await
                };
                match outcome {
                    Ok(resp) => {

                        if !resp.tool_calls.is_empty() {
                            return None;
                        }
                        let text = resp.text_or_empty().to_string();
                        if text.trim().is_empty() {
                            None
                        } else {
                            Some(text)
                        }
                    }
                    Err(err) => {
                        tracing::warn!("self-consistency extra sample failed: {err}");
                        None
                    }
                }
            }
        })
        .collect();

    let outcomes = futures_util::future::join_all(futures).await;
    let mut samples: Vec<String> = Vec::with_capacity(cfg.samples as usize);
    samples.push(initial_text.clone());
    for opt in outcomes.into_iter().flatten() {
        samples.push(opt);
    }

    if samples.len() <= 1 {

        observability::subsystem_metrics::incr_self_consistency_failure();
        return (initial_text, 1.0, false, 1);
    }

    let result = crate::agent::self_consistency::aggregate(
        &crate::agent::self_consistency::Aggregator::MajorityVote,
        samples,
    );
    let winner = result.chosen.clone();
    let overridden = winner != initial_text;
    observability::subsystem_metrics::observe_self_consistency_run(
        result.agreement as f64,
        overridden,
    );
    (winner, result.agreement as f64, overridden, result.samples)
}

fn should_execute_tools_in_parallel(
    tool_calls: &[ParsedToolCall],
    approval: Option<&ApprovalManager>,
) -> bool {

    use crate::agent::executor_core::DispatchMode;

    let names: Vec<&str> = tool_calls.iter().map(|c| c.name.as_str()).collect();
    let mode = DispatchMode::select(
        &names,
        |name| {
            approval
                .map(|mgr| mgr.needs_approval(name))
                .unwrap_or(false)
        },
        resolve_parallel_tool_cap(),
    );
    matches!(mode, DispatchMode::Parallel { .. })
}

async fn execute_tools_parallel(
    tool_calls: &[ParsedToolCall],
    tools_registry: &[Box<dyn Tool>],
    tool_registry: Option<&ToolRegistry>,
    activated_tools: Option<&std::sync::Arc<parking_lot::Mutex<crate::tools::ActivatedToolSet>>>,
    observer: &dyn Observer,
    cancellation_token: Option<&CancellationToken>,
    rbac_engine: Option<&std::sync::Arc<crate::security::rbac::RbacEngine>>,
    rbac_identity: Option<&crate::security::rbac::CallerIdentity>,
) -> Result<Vec<ToolExecutionOutcome>> {

    let configured_cap = resolve_parallel_tool_cap();
    let max_concurrency = configured_cap.min(tool_calls.len().max(1));

    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(max_concurrency));

    let futures: Vec<_> = tool_calls
        .iter()
        .map(|call| {
            let sem = semaphore.clone();
            async move {

                let _permit = match sem.acquire_owned().await {
                    Ok(p) => p,
                    Err(e) => {
                        return Err(anyhow::anyhow!("semaphore closed: {e}"));
                    }
                };
                execute_one_tool(
                    &call.name,
                    call.arguments.clone(),
                    tools_registry,
                    tool_registry,
                    activated_tools,
                    observer,
                    cancellation_token,
                    rbac_engine,
                    rbac_identity,
                )
                .await
            }
        })
        .collect();

    let results = futures_util::future::join_all(futures).await;
    results.into_iter().collect()
}

async fn execute_tools_sequential(
    tool_calls: &[ParsedToolCall],
    tools_registry: &[Box<dyn Tool>],
    tool_registry: Option<&ToolRegistry>,
    activated_tools: Option<&std::sync::Arc<parking_lot::Mutex<crate::tools::ActivatedToolSet>>>,
    observer: &dyn Observer,
    cancellation_token: Option<&CancellationToken>,
    rbac_engine: Option<&std::sync::Arc<crate::security::rbac::RbacEngine>>,
    rbac_identity: Option<&crate::security::rbac::CallerIdentity>,
) -> Result<Vec<ToolExecutionOutcome>> {
    let mut outcomes = Vec::with_capacity(tool_calls.len());

    for call in tool_calls {
        outcomes.push(
            execute_one_tool(
                &call.name,
                call.arguments.clone(),
                tools_registry,
                tool_registry,
                activated_tools,
                observer,
                cancellation_token,
                rbac_engine,
                rbac_identity,
            )
            .await?,
        );
    }

    Ok(outcomes)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_tool_call_loop(
    provider: &dyn Provider,
    history: &mut Vec<ChatMessage>,
    tools_registry: &[Box<dyn Tool>],
    observer: &dyn Observer,
    provider_name: &str,
    model: &str,
    temperature: f64,
    silent: bool,
    approval: Option<&ApprovalManager>,
    channel_name: &str,
    channel_reply_target: Option<&str>,
    multimodal_config: &crate::config::MultimodalConfig,
    max_tool_iterations: usize,
    cancellation_token: Option<CancellationToken>,
    on_delta: Option<tokio::sync::mpsc::Sender<DraftEvent>>,
    hooks: Option<&crate::hooks::HookRunner>,
    excluded_tools: &[String],
    dedup_exempt_tools: &[String],
    activated_tools: Option<&std::sync::Arc<parking_lot::Mutex<crate::tools::ActivatedToolSet>>>,
    model_switch_callback: Option<ModelSwitchCallback>,
    pacing: &crate::config::PacingConfig,
    rbac_engine: Option<&std::sync::Arc<crate::security::rbac::RbacEngine>>,
    rbac_identity: Option<&crate::security::rbac::CallerIdentity>,
    plan_mode_flag: Option<&crate::tools::PlanModeFlag>,

    tool_registry: Option<&ToolRegistry>,
) -> Result<String> {
    let mode_max = if let Some(svc) = crate::services::try_get_services() {
        let mode = *svc.coding_mode.read();
        mode.max_iterations_override()
    } else {
        0
    };
    let max_iterations = if mode_max > 0 {
        mode_max
    } else if max_tool_iterations == 0 {
        DEFAULT_MAX_TOOL_ITERATIONS
    } else {
        max_tool_iterations
    };

    let turn_id = Uuid::new_v4().to_string();
    let loop_started_at = Instant::now();

    tracing::info!(
        target: "agent.turn",
        turn_id = %turn_id,
        provider = %provider_name,
        model = %model,
        channel = %channel_name,
        max_iterations = max_iterations,
        "turn started"
    );

    let loop_ignore_tools: HashSet<&str> = pacing
        .loop_ignore_tools
        .iter()
        .map(String::as_str)
        .collect();
    let mut consecutive_identical_outputs: usize = 0;
    let mut last_tool_output_hash: Option<u64> = None;
    let identical_output_threshold: usize = {
        let raw = pacing.loop_detection_identical_output_threshold;
        if raw == 0 {
            crate::agent::loop_control::DEFAULT_IDENTICAL_OUTPUT_THRESHOLD as usize
        } else {
            raw as usize
        }
    };

    let mut loop_detector = crate::agent::loop_detector::LoopDetector::new(
        crate::agent::loop_detector::LoopDetectorConfig {
            enabled: pacing.loop_detection_enabled,
            window_size: pacing.loop_detection_window_size,
            max_repeats: pacing.loop_detection_max_repeats,
        },
    );

    let mut cached_tool_specs: Option<std::sync::Arc<Vec<crate::tools::ToolSpec>>> = None;
    let mut cached_mode_key: (u64, bool) = (0, false);

    let mut _turn_metrics = crate::agent::executor_core::TurnMetricsGuard::start();

    let mut _pacing_gov = crate::agent::executor_core::PacingGovernor::new(
        max_iterations,
        None,
        pacing
            .step_timeout_secs
            .map(|s| std::time::Duration::from_secs(s)),
    );

    let mut plan_nudge_state =
        crate::agent::plan_mode_enforcement::PlanModeNudgeState::new();

    let mut awaiting_user_input = false;

    if let Some(svc) = crate::services::try_get_services() {
        let mode = *svc.coding_mode.read();
        if let Some(reminder) = crate::agent::mode_effects::pre_turn_reminder(mode) {
            crate::agent::mode_effects::replace_or_push_system_reminder(
                history,
                reminder.to_string(),
            );
        }
    }

    for iteration in 0..max_iterations {
        if let Err(budget_exceeded) = _pacing_gov.tick() {
            tracing::warn!(
                target: "agent.pacing",
                turn_id = %turn_id,
                reason = %budget_exceeded,
                "agent turn pacing budget exceeded"
            );
        }

        tracing::debug!(
            target: "agent.iteration",
            turn_id = %turn_id,
            iter = iteration,
            "iteration start"
        );
        let mut seen_tool_signatures: HashSet<(String, String)> = HashSet::new();

        let mut plan_finalized_this_iter: bool = false;

        if cancellation_token
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            return Err(ToolLoopCancelled.into());
        }

        if let Some(ref callback) = model_switch_callback {
            let guard = callback.lock();
            if let Some((new_provider, new_model)) = guard.as_ref() {
                if new_provider != provider_name || new_model != model {
                    tracing::info!(
                        "Model switch detected: {} {} -> {} {}",
                        provider_name,
                        model,
                        new_provider,
                        new_model
                    );
                    return Err(ModelSwitchRequested {
                        provider: new_provider.clone(),
                        model: new_model.clone(),
                    }
                    .into());
                }
            }
        }

        if let Some(svc) = crate::services::try_get_services() {
            let mode = *svc.coding_mode.read();
            let max_ctx = svc.get_max_context_tokens();
            if let Some(budget_msg) =
                crate::agent::mode_effects::build_context_budget_message(mode, history, max_ctx)
            {
                crate::agent::mode_effects::replace_or_push_system_reminder(
                    history,
                    budget_msg,
                );
            }
        }

        let coding_mode_allowlist: Option<HashSet<&str>> =
            if let Some(svc) = crate::services::try_get_services() {
                let mode = *svc.coding_mode.read();
                mode.allowed_tools()
            } else {
                None
            };
        let plan_mode_active = plan_mode_flag.map_or(false, |f| *f.read());

        let mode_hash = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            if let Some(ref al) = coding_mode_allowlist {
                let mut sorted: Vec<&str> = al.iter().copied().collect();
                sorted.sort_unstable();
                sorted.hash(&mut h);
            }
            h.finish()
        };

        let deferred_builtin_names: Option<HashSet<String>> =
            if coding_mode_allowlist.is_none() && !plan_mode_active {
                crate::services::try_get_services()
                    .map(|svc| svc.deferred_builtin_names.read().clone())
                    .filter(|s| !s.is_empty())
            } else {
                None
            };
        let deferred_hash = if let Some(ref names) = deferred_builtin_names {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            let mut sorted: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
            sorted.sort_unstable();
            sorted.hash(&mut h);
            h.finish()
        } else {
            0
        };
        let current_mode_key = (mode_hash ^ deferred_hash, plan_mode_active);

        if cached_tool_specs.is_none() || current_mode_key != cached_mode_key {
            cached_mode_key = current_mode_key;
            let specs: Vec<_> = tools_registry
                .iter()
                .filter(|tool| !excluded_tools.iter().any(|ex| ex == tool.name()))
                .filter(|tool| {
                    if let Some(ref allowlist) = coding_mode_allowlist {
                        allowlist.contains(tool.name())
                    } else if plan_mode_active {
                        is_plan_mode_allowed(tool.name())
                    } else if let Some(ref deferred) = deferred_builtin_names {
                        !deferred.contains(tool.name())
                    } else {
                        true
                    }
                })
                .map(|tool| tool.spec())
                .collect();
            cached_tool_specs = Some(std::sync::Arc::new(specs));
        }

        let tool_specs_arc = cached_tool_specs.as_ref().unwrap().clone();
        let mut tool_specs = (*tool_specs_arc).clone();
        if let Some(at) = activated_tools {
            for spec in at.lock().tool_specs() {
                if !excluded_tools.iter().any(|ex| ex == &spec.name) {
                    let allowed = if let Some(ref allowlist) = coding_mode_allowlist {
                        allowlist.contains(spec.name.as_str())
                    } else if plan_mode_active {
                        is_plan_mode_allowed(&spec.name)
                    } else {
                        true
                    };
                    if allowed {
                        tool_specs.push(spec);
                    }
                }
            }
        }
        let use_native_tools = provider.supports_native_tools() && !tool_specs.is_empty();

        let image_marker_count = multimodal::count_image_markers(history);

        let vision_provider_box: Option<Box<dyn Provider>> = if image_marker_count > 0
            && !provider.supports_vision()
        {
            if let Some(ref vp) = multimodal_config.vision_provider {
                let vp_instance = providers::create_provider(vp, None)
                    .map_err(|e| anyhow::anyhow!("failed to create vision provider '{vp}': {e}"))?;
                if !vp_instance.supports_vision() {
                    return Err(ProviderCapabilityError {
                        provider: vp.clone(),
                        capability: "vision".to_string(),
                        message: format!(
                            "configured vision_provider '{vp}' does not support vision input"
                        ),
                    }
                    .into());
                }
                Some(vp_instance)
            } else {
                return Err(ProviderCapabilityError {
                        provider: provider_name.to_string(),
                        capability: "vision".to_string(),
                        message: format!(
                            "received {image_marker_count} image marker(s), but this provider does not support vision input"
                        ),
                    }
                    .into());
            }
        } else {
            None
        };

        let (active_provider, active_provider_name, active_model): (&dyn Provider, &str, &str) =
            if let Some(ref vp_box) = vision_provider_box {
                let vp_name = multimodal_config
                    .vision_provider
                    .as_deref()
                    .unwrap_or(provider_name);
                let vm = multimodal_config.vision_model.as_deref().unwrap_or(model);
                (vp_box.as_ref(), vp_name, vm)
            } else {
                (provider, provider_name, model)
            };

        let prepared_messages =
            multimodal::prepare_messages_for_provider(history, multimodal_config).await?;

        if let Some(ref tx) = on_delta {
            let phase = if iteration == 0 {
                "\u{1f914} Thinking...\n".to_string()
            } else {
                format!("\u{1f914} Thinking (round {})...\n", iteration + 1)
            };
            let _ = tx.send(DraftEvent::Progress(phase)).await;
        }

        observer.record_event(&ObserverEvent::LlmRequest {
            provider: active_provider_name.to_string(),
            model: active_model.to_string(),
            messages_count: history.len(),
        });
        runtime_trace::record_event(
            "llm_request",
            Some(channel_name),
            Some(active_provider_name),
            Some(active_model),
            Some(&turn_id),
            None,
            None,
            serde_json::json!({
                "iteration": iteration + 1,
                "messages_count": history.len(),
            }),
        );

        let llm_started_at = Instant::now();

        if let Some(svc) = crate::services::try_get_services() {
            let est_tokens: u64 = history
                .iter()
                .map(|m| svc.token_estimator.estimate(&m.content))
                .sum();
            tracing::debug!(estimated_tokens = est_tokens, "Pre-call token estimate");
        }

        if let Some(hooks) = hooks {
            hooks.fire_llm_input(history, model).await;
        }

        if let Some(BudgetCheck::Exceeded {
            current_usd,
            limit_usd,
            period,
        }) = check_tool_loop_budget(None)
        {
            return Err(anyhow::anyhow!(
                "Budget exceeded: ${:.4} of ${:.2} {:?} limit. Cannot make further API calls until the budget resets.",
                current_usd,
                limit_usd,
                period
            ));
        }

        if let Some(svc) = crate::services::try_get_services() {
            if !svc.rate_limiter.try_acquire("llm").await {
                if let Some(msg) = svc.rate_limiter.message("llm").await {
                    tracing::warn!("{}", msg.message);
                    if let Some(retry_ms) = svc
                        .rate_limiter
                        .status("llm")
                        .await
                        .and_then(|s| s.retry_after_ms)
                    {
                        tokio::time::sleep(std::time::Duration::from_millis(retry_ms.min(10_000)))
                            .await;
                    }
                }
            }
        }

        let request_tools = if use_native_tools {
            Some(tool_specs.as_slice())
        } else {
            None
        };
        let should_consume_provider_stream = on_delta.is_some()
            && provider.supports_streaming()
            && (request_tools.is_none() || provider.supports_streaming_tool_events());
        tracing::debug!(
            has_on_delta = on_delta.is_some(),
            supports_streaming = provider.supports_streaming(),
            should_consume_provider_stream,
            "Streaming decision for iteration {}",
            iteration + 1,
        );
        let mut streamed_live_deltas = false;

        let chat_result = if should_consume_provider_stream {
            match consume_provider_streaming_response(
                active_provider,
                &prepared_messages.messages,
                request_tools,
                active_model,
                temperature,
                cancellation_token.as_ref(),
                on_delta.as_ref(),
            )
            .await
            {
                Ok(streamed) => {
                    streamed_live_deltas = streamed.forwarded_live_deltas;

                    let reasoning_content = if !streamed.reasoning_content.is_empty() {
                        Some(streamed.reasoning_content)
                    } else if !streamed.tool_calls.is_empty() {
                        Some(
                            "(chain-of-thought unavailable — model emitted tool calls without a CoT stream)"
                                .to_string(),
                        )
                    } else {
                        None
                    };
                    Ok(crate::providers::ChatResponse {
                        text: Some(streamed.response_text),
                        tool_calls: streamed.tool_calls,
                        usage: None,
                        reasoning_content,
                    })
                }
                Err(stream_err) => {
                    tracing::warn!(
                        provider = active_provider_name,
                        model = active_model,
                        iteration = iteration + 1,
                        "provider streaming failed, falling back to non-streaming chat: {stream_err}"
                    );
                    runtime_trace::record_event(
                        "llm_stream_fallback",
                        Some(channel_name),
                        Some(active_provider_name),
                        Some(active_model),
                        Some(&turn_id),
                        Some(false),
                        Some("provider stream failed; fallback to non-streaming chat"),
                        serde_json::json!({
                            "iteration": iteration + 1,
                            "error": scrub_credentials(&stream_err.to_string()),
                        }),
                    );
                    if let Some(ref tx) = on_delta {
                        let _ = tx.send(DraftEvent::Clear).await;
                    }
                    call_provider_chat(
                        active_provider,
                        &prepared_messages.messages,
                        request_tools,
                        active_model,
                        temperature,
                        cancellation_token.as_ref(),
                    )
                    .await
                }
            }
        } else {

            let chat_future = active_provider.chat(
                ChatRequest {
                    messages: &prepared_messages.messages,
                    tools: request_tools,
                },
                active_model,
                temperature,
            );

            match pacing.step_timeout_secs {
                Some(step_secs) if step_secs > 0 => {
                    let step_timeout = Duration::from_secs(step_secs);
                    if let Some(token) = cancellation_token.as_ref() {
                        tokio::select! {
                            () = token.cancelled() => return Err(ToolLoopCancelled.into()),
                            result = tokio::time::timeout(step_timeout, chat_future) => {
                                match result {
                                    Ok(inner) => inner,
                                    Err(_) => anyhow::bail!(
                                        "LLM inference step timed out after {step_secs}s (step_timeout_secs)"
                                    ),
                                }
                            },
                        }
                    } else {
                        match tokio::time::timeout(step_timeout, chat_future).await {
                            Ok(inner) => inner,
                            Err(_) => anyhow::bail!(
                                "LLM inference step timed out after {step_secs}s (step_timeout_secs)"
                            ),
                        }
                    }
                }
                _ => {
                    if let Some(token) = cancellation_token.as_ref() {
                        tokio::select! {
                            () = token.cancelled() => return Err(ToolLoopCancelled.into()),
                            result = chat_future => result,
                        }
                    } else {
                        chat_future.await
                    }
                }
            }
        };

        let (
            response_text,
            parsed_text,
            tool_calls,
            assistant_history_content,
            native_tool_calls,
            _parse_issue_detected,
            response_streamed_live,
        ) = match chat_result {
            Ok(resp) => {
                let (resp_input_tokens, resp_output_tokens) = resp
                    .usage
                    .as_ref()
                    .map(|u| (u.input_tokens, u.output_tokens))
                    .unwrap_or((None, None));

                observer.record_event(&ObserverEvent::LlmResponse {
                    provider: provider_name.to_string(),
                    model: model.to_string(),
                    duration: llm_started_at.elapsed(),
                    success: true,
                    error_message: None,
                    input_tokens: resp_input_tokens,
                    output_tokens: resp_output_tokens,
                });

                let _ = resp
                    .usage
                    .as_ref()
                    .and_then(|usage| record_tool_loop_cost_usage(provider_name, model, usage));

                if let Some(usage) = resp.usage.as_ref() {
                    let input_tokens = usage.input_tokens.unwrap_or(0);
                    let output_tokens = usage.output_tokens.unwrap_or(0);
                    if input_tokens + output_tokens > 0 {
                        let prices = TOOL_LOOP_COST_TRACKING_CONTEXT
                            .try_with(Clone::clone)
                            .ok()
                            .flatten()
                            .map(|c| c.prices)
                            .unwrap_or_default();
                        let pricing = lookup_model_pricing(&prices, provider_name, model);
                        let cost_usd = CostTokenUsage::new(
                            model,
                            input_tokens,
                            output_tokens,
                            pricing.map_or(0.0, |e| e.input),
                            pricing.map_or(0.0, |e| e.output),
                        )
                        .cost_usd;
                        if let Some(bs) = crate::bootstrap::try_get_state() {
                            bs.write(|state| {
                                state.accumulate_usage(
                                    model,
                                    input_tokens,
                                    output_tokens,
                                    0,
                                    0,
                                    cost_usd,
                                );
                                state.total_api_duration_ms +=
                                    llm_started_at.elapsed().as_millis() as u64;
                            });
                        }
                    }
                }

                let response_text = resp.text_or_empty().to_string();

                let mut calls = parse_structured_tool_calls(&resp.tool_calls);
                let mut parsed_text = String::new();

                if calls.is_empty() {
                    let (fallback_text, fallback_calls) = parse_tool_calls(&response_text);
                    if !fallback_text.is_empty() {
                        parsed_text = fallback_text;
                    }
                    calls = fallback_calls;
                }

                let parse_issue = detect_tool_call_parse_issue(&response_text, &calls);
                if let Some(ref issue) = parse_issue {
                    runtime_trace::record_event(
                        "tool_call_parse_issue",
                        Some(channel_name),
                        Some(provider_name),
                        Some(model),
                        Some(&turn_id),
                        Some(false),
                        Some(issue.as_str()),
                        serde_json::json!({
                            "iteration": iteration + 1,
                            "response_excerpt": truncate_with_ellipsis(
                                &scrub_credentials(&response_text),
                                600
                            ),
                        }),
                    );
                }

                runtime_trace::record_event(
                    "llm_response",
                    Some(channel_name),
                    Some(provider_name),
                    Some(model),
                    Some(&turn_id),
                    Some(true),
                    None,
                    serde_json::json!({
                        "iteration": iteration + 1,
                        "duration_ms": llm_started_at.elapsed().as_millis(),
                        "input_tokens": resp_input_tokens,
                        "output_tokens": resp_output_tokens,
                        "raw_response": scrub_credentials(&response_text),
                        "native_tool_calls": resp.tool_calls.len(),
                        "parsed_tool_calls": calls.len(),
                    }),
                );

                let reasoning_content = resp.reasoning_content.clone();
                let assistant_history_content = if resp.tool_calls.is_empty() {
                    if use_native_tools {
                        build_native_assistant_history_from_parsed_calls(
                            &response_text,
                            &calls,
                            reasoning_content.as_deref(),
                        )
                        .unwrap_or_else(|| response_text.clone())
                    } else {
                        response_text.clone()
                    }
                } else {
                    build_native_assistant_history(
                        &response_text,
                        &resp.tool_calls,
                        reasoning_content.as_deref(),
                    )
                };

                let native_calls = resp.tool_calls;
                (
                    response_text,
                    parsed_text,
                    calls,
                    assistant_history_content,
                    native_calls,
                    parse_issue.is_some(),
                    streamed_live_deltas,
                )
            }
            Err(e) => {
                let safe_error = crate::providers::sanitize_api_error(&e.to_string());
                observer.record_event(&ObserverEvent::LlmResponse {
                    provider: provider_name.to_string(),
                    model: model.to_string(),
                    duration: llm_started_at.elapsed(),
                    success: false,
                    error_message: Some(safe_error.clone()),
                    input_tokens: None,
                    output_tokens: None,
                });
                runtime_trace::record_event(
                    "llm_response",
                    Some(channel_name),
                    Some(provider_name),
                    Some(model),
                    Some(&turn_id),
                    Some(false),
                    Some(&safe_error),
                    serde_json::json!({
                        "iteration": iteration + 1,
                        "duration_ms": llm_started_at.elapsed().as_millis(),
                    }),
                );
                return Err(e);
            }
        };

        let display_text = if parsed_text.is_empty() {
            response_text.clone()
        } else {
            parsed_text
        };

        if let Some(ref tx) = on_delta {
            let llm_secs = llm_started_at.elapsed().as_secs();
            if !tool_calls.is_empty() {
                let _ = tx
                    .send(DraftEvent::Progress(format!(
                        "\u{1f4ac} Got {} tool call(s) ({llm_secs}s)\n",
                        tool_calls.len()
                    )))
                    .await;
            }
        }

        if tool_calls.is_empty() {

            let in_plan_mode =
                crate::agent::plan_mode_enforcement::detect_plan_mode_active(
                    plan_mode_flag,
                );

            if matches!(
                crate::agent::plan_mode_enforcement::evaluate_plan_mode_exit(
                    in_plan_mode,
                    &plan_nudge_state,
                    awaiting_user_input,
                ),
                crate::agent::plan_mode_enforcement::PlanModeExitDecision::InjectNudge
            ) {
                tracing::info!(
                    target: "agent.plan_mode",
                    turn_id = %turn_id,
                    nudge_count = plan_nudge_state.nudge_count + 1,
                    max_nudges =
                        crate::agent::plan_mode_enforcement::MAX_PLAN_NUDGES,
                    "Plan mode: model exited without exit_plan_mode; injecting nudge"
                );

                if !response_text.trim().is_empty() {
                    history.push(ChatMessage::assistant(&response_text));
                }
                let msg = crate::agent::plan_mode_enforcement::nudge_message(
                    &plan_nudge_state,
                );
                history.push(ChatMessage::system(msg));
                plan_nudge_state.nudge_count += 1;
                continue;
            }

            let sc_cfg = resolve_self_consistency_config();
            let (response_text, display_text) = if sc_cfg.should_engage() {
                let (winner, _agreement, overridden, samples_used) =
                    run_self_consistency_resampling(
                        &sc_cfg,
                        active_provider,
                        &prepared_messages.messages,
                        request_tools,
                        active_model,
                        cancellation_token.as_ref(),
                        response_text.clone(),
                    )
                    .await;
                if overridden {
                    runtime_trace::record_event(
                        "self_consistency_override",
                        Some(channel_name),
                        Some(provider_name),
                        Some(model),
                        Some(&turn_id),
                        Some(true),
                        None,
                        serde_json::json!({
                            "samples_used": samples_used,
                            "configured_samples": sc_cfg.samples,
                        }),
                    );
                    (winner.clone(), winner)
                } else {
                    (response_text, display_text)
                }
            } else {
                (response_text, display_text)
            };
            runtime_trace::record_event(
                "turn_final_response",
                Some(channel_name),
                Some(provider_name),
                Some(model),
                Some(&turn_id),
                Some(true),
                None,
                serde_json::json!({
                    "iteration": iteration + 1,
                    "text": scrub_credentials(&display_text),
                }),
            );

            if let Some(ref tx) = on_delta {
                let should_emit_post_hoc_chunks =
                    !response_streamed_live || display_text != response_text;
                if !should_emit_post_hoc_chunks {
                    history.push(ChatMessage::assistant(response_text.clone()));
                    _turn_metrics.mark_ok();
                    return Ok(display_text);
                }

                let _ = tx.send(DraftEvent::Clear).await;

                let mut chunk = String::new();
                for word in display_text.split_inclusive(char::is_whitespace) {
                    if cancellation_token
                        .as_ref()
                        .is_some_and(CancellationToken::is_cancelled)
                    {
                        return Err(ToolLoopCancelled.into());
                    }
                    chunk.push_str(word);
                    if chunk.len() >= STREAM_CHUNK_MIN_CHARS
                        && tx
                            .send(DraftEvent::Content(std::mem::take(&mut chunk)))
                            .await
                            .is_err()
                    {
                        break;
                    }
                }
                if !chunk.is_empty() {
                    let _ = tx.send(DraftEvent::Content(chunk)).await;
                }
            }
            history.push(ChatMessage::assistant(response_text.clone()));
            _turn_metrics.mark_ok();
            return Ok(display_text);
        }

        if !display_text.is_empty() {
            if !native_tool_calls.is_empty() {
                if let Some(ref tx) = on_delta {
                    let mut narration = display_text.clone();
                    if !narration.ends_with('\n') {
                        narration.push('\n');
                    }
                    let _ = tx.send(DraftEvent::Content(narration)).await;
                }
            }
            if !silent {
                print!("{display_text}");
                let _ = std::io::stdout().flush();
            }
        }

        let mut tool_results = String::new();
        let mut individual_results: Vec<(Option<String>, String)> = Vec::new();
        let mut ordered_results: Vec<Option<(String, Option<String>, ToolExecutionOutcome)>> =
            (0..tool_calls.len()).map(|_| None).collect();
        let allow_parallel_execution = should_execute_tools_in_parallel(&tool_calls, approval);
        let mut executable_indices: Vec<usize> = Vec::new();
        let mut executable_calls: Vec<ParsedToolCall> = Vec::new();

        let mut deferred_system_after_tool_batch: Vec<String> = Vec::new();

        for (idx, call) in tool_calls.iter().enumerate() {

            let mut tool_name = call.name.clone();
            let mut tool_args = call.arguments.clone();

            if call.parse_error {
                tracing::warn!(
                    tool = %call.name,
                    "Tool '{}' received empty arguments because JSON parsing of its \
                    original arguments failed. The model should retry with valid JSON arguments.",
                    call.name
                );
            }
            if let Some(hooks) = hooks {
                match hooks
                    .run_before_tool_call(tool_name.clone(), tool_args.clone())
                    .await
                {
                    crate::hooks::HookResult::Cancel(reason) => {
                        tracing::info!(tool = %call.name, %reason, "tool call cancelled by hook");
                        let cancelled = format!("Cancelled by hook: {reason}");
                        runtime_trace::record_event(
                            "tool_call_result",
                            Some(channel_name),
                            Some(provider_name),
                            Some(model),
                            Some(&turn_id),
                            Some(false),
                            Some(&cancelled),
                            serde_json::json!({
                                "iteration": iteration + 1,
                                "tool": call.name,
                                "arguments": scrub_credentials(&tool_args.to_string()),
                            }),
                        );
                        if let Some(ref tx) = on_delta {
                            let _ = tx
                                .send(DraftEvent::Progress(format!(
                                    "\u{274c} {}: {}\n",
                                    call.name,
                                    truncate_with_ellipsis(&scrub_credentials(&cancelled), 200)
                                )))
                                .await;
                        }
                        ordered_results[idx] = Some((
                            call.name.clone(),
                            call.tool_call_id.clone(),
                            ToolExecutionOutcome {
                                output: cancelled,
                                success: false,
                                error_reason: Some(scrub_credentials(&reason)),
                                duration: Duration::ZERO,
                            },
                        ));
                        continue;
                    }
                    crate::hooks::HookResult::Continue((name, args)) => {
                        tool_name = name;
                        tool_args = args;
                    }
                }
            }

            maybe_inject_channel_delivery_defaults(
                &tool_name,
                &mut tool_args,
                channel_name,
                channel_reply_target,
            );

            let intercept = crate::services::try_get_services().and_then(|svc| {
                let mode = *svc.coding_mode.read();
                crate::agent::mode_effects::mode_blocks_tool(mode, &tool_name)
                    .map(|reason| (mode, reason))
            });
            if let Some((intercepted_mode, reason)) = intercept {
                crate::agent::mode_effects::record_mode_intercept(
                    crate::agent::mode_effects::ModeInterceptReason::ReadOnlyPolicy,
                    &crate::agent::mode_effects::ModeInterceptContext {
                        mode: intercepted_mode,
                        channel: Some(channel_name),
                        provider: Some(provider_name),
                        model: Some(model),
                        turn_id: Some(&turn_id),
                        tool: Some(&tool_name),
                        tool_call_id: call.tool_call_id.as_deref(),
                        iteration: Some(iteration + 1),
                        message: Some(&reason),
                    },
                );
                if let Some(ref tx) = on_delta {
                    let _ = tx
                        .send(DraftEvent::Progress(format!(
                            "\u{274c} {}: {}\n",
                            tool_name,
                            truncate_with_ellipsis(&reason, 200)
                        )))
                        .await;
                }
                ordered_results[idx] = Some((
                    tool_name.clone(),
                    call.tool_call_id.clone(),
                    ToolExecutionOutcome {
                        output: reason.clone(),
                        success: false,
                        error_reason: Some(reason),
                        duration: Duration::ZERO,
                    },
                ));
                continue;
            }

            let mode_auto_approved = crate::services::try_get_services()
                .map(|svc| crate::agent::mode_effects::mode_auto_approves(*svc.coding_mode.read()))
                .unwrap_or(false);

            if let Some(mgr) = approval {
                if !mode_auto_approved && mgr.needs_approval(&tool_name) {
                    let request = ApprovalRequest {
                        tool_name: tool_name.clone(),
                        arguments: tool_args.clone(),
                    };

                    let decision = if mgr.is_non_interactive() {
                        ApprovalResponse::No
                    } else {
                        mgr.prompt_cli(&request)
                    };

                    mgr.record_decision(&tool_name, &tool_args, decision, channel_name);

                    if decision == ApprovalResponse::No {
                        let denied = "Denied by user.".to_string();
                        runtime_trace::record_event(
                            "tool_call_result",
                            Some(channel_name),
                            Some(provider_name),
                            Some(model),
                            Some(&turn_id),
                            Some(false),
                            Some(&denied),
                            serde_json::json!({
                                "iteration": iteration + 1,
                                "tool": tool_name.clone(),
                                "arguments": scrub_credentials(&tool_args.to_string()),
                            }),
                        );
                        if let Some(ref tx) = on_delta {
                            let _ = tx
                                .send(DraftEvent::Progress(format!(
                                    "\u{274c} {}: {}\n",
                                    tool_name, denied
                                )))
                                .await;
                        }
                        ordered_results[idx] = Some((
                            tool_name.clone(),
                            call.tool_call_id.clone(),
                            ToolExecutionOutcome {
                                output: denied.clone(),
                                success: false,
                                error_reason: Some(denied),
                                duration: Duration::ZERO,
                            },
                        ));
                        continue;
                    }
                }
            }

            let signature = tool_call_signature(&tool_name, &tool_args);
            let dedup_exempt = dedup_exempt_tools.iter().any(|e| e == &tool_name);
            if !dedup_exempt && !seen_tool_signatures.insert(signature) {

                let deduplicated = format!(
                    "[Deduplicated] Tool '{tool_name}' with identical arguments was already \
                    executed in this turn and its result was returned above. \
                    No further action needed for this duplicate call."
                );
                runtime_trace::record_event(
                    "tool_call_result",
                    Some(channel_name),
                    Some(provider_name),
                    Some(model),
                    Some(&turn_id),
                    Some(true),
                    None::<&str>,
                    serde_json::json!({
                        "iteration": iteration + 1,
                        "tool": tool_name.clone(),
                        "arguments": scrub_credentials(&tool_args.to_string()),
                        "deduplicated": true,
                    }),
                );
                if let Some(ref tx) = on_delta {
                    let _ = tx
                        .send(DraftEvent::Progress(format!(
                            "\u{1f7e9} {}: deduplicated\n",
                            tool_name
                        )))
                        .await;
                }
                ordered_results[idx] = Some((
                    tool_name.clone(),
                    call.tool_call_id.clone(),
                    ToolExecutionOutcome {
                        output: deduplicated,
                        success: true,
                        error_reason: None,
                        duration: Duration::ZERO,
                    },
                ));
                continue;
            }

            runtime_trace::record_event(
                "tool_call_start",
                Some(channel_name),
                Some(provider_name),
                Some(model),
                Some(&turn_id),
                None,
                None,
                serde_json::json!({
                    "iteration": iteration + 1,
                    "tool": tool_name.clone(),
                    "arguments": scrub_credentials(&tool_args.to_string()),
                }),
            );

            if let Some(ref tx) = on_delta {
                let hint = truncate_tool_args_for_progress(&tool_name, &tool_args, 60);
                let progress = if hint.is_empty() {
                    format!("\u{23f3} {}\n", tool_name)
                } else {
                    format!("\u{23f3} {}: {hint}\n", tool_name)
                };
                tracing::debug!(tool = %tool_name, "Sending progress start to draft");
                let _ = tx.send(DraftEvent::Progress(progress)).await;

                let _ = tx
                    .send(DraftEvent::ToolCall {
                        name: tool_name.clone(),
                        args: tool_args.clone(),
                    })
                    .await;
            }

            executable_indices.push(idx);
            executable_calls.push(ParsedToolCall {
                name: tool_name,
                arguments: tool_args,
                tool_call_id: call.tool_call_id.clone(),
                parse_error: call.parse_error,
            });
        }

        let parent_draft_for_scope = on_delta.clone();
        let executed_outcomes: Vec<(usize, ParsedToolCall, ToolExecutionOutcome)> =
            if allow_parallel_execution && executable_calls.len() > 1 {
                let outcomes = PARENT_DRAFT_CHANNEL
                    .scope(
                        parent_draft_for_scope,
                        execute_tools_parallel(
                            &executable_calls,
                            tools_registry,
                            tool_registry,
                            activated_tools,
                            observer,
                            cancellation_token.as_ref(),
                            rbac_engine,
                            rbac_identity,
                        ),
                    )
                    .await?;
                executable_indices
                    .into_iter()
                    .zip(executable_calls.into_iter())
                    .zip(outcomes.into_iter())
                    .map(|((idx, call), outcome)| (idx, call, outcome))
                    .collect()
            } else {
                let outcomes = PARENT_DRAFT_CHANNEL
                    .scope(
                        parent_draft_for_scope,
                        execute_tools_sequential(
                            &executable_calls,
                            tools_registry,
                            tool_registry,
                            activated_tools,
                            observer,
                            cancellation_token.as_ref(),
                            rbac_engine,
                            rbac_identity,
                        ),
                    )
                    .await?;
                executable_indices
                    .into_iter()
                    .zip(executable_calls.into_iter())
                    .zip(outcomes.into_iter())
                    .map(|((idx, call), outcome)| (idx, call, outcome))
                    .collect()
            };

        for (idx, call, outcome) in executed_outcomes {
            runtime_trace::record_event(
                "tool_call_result",
                Some(channel_name),
                Some(provider_name),
                Some(model),
                Some(&turn_id),
                Some(outcome.success),
                outcome.error_reason.as_deref(),
                serde_json::json!({
                    "iteration": iteration + 1,
                    "tool": call.name.clone(),
                    "duration_ms": outcome.duration.as_millis(),
                    "output": scrub_credentials(&outcome.output),
                }),
            );

            if let Some(hooks) = hooks {
                let tool_result_obj = crate::tools::ToolResult {
                    success: outcome.success,
                    output: outcome.output.clone(),
                    error: None,
                };
                hooks
                    .fire_after_tool_call(&call.name, &tool_result_obj, outcome.duration)
                    .await;
            }

            if let Some(ref tx) = on_delta {
                let secs = outcome.duration.as_secs();
                let progress_msg = if outcome.success {
                    format!("\u{2705} {} ({secs}s)\n", call.name)
                } else if let Some(ref reason) = outcome.error_reason {
                    format!(
                        "\u{274c} {} ({secs}s): {}\n",
                        call.name,
                        truncate_with_ellipsis(reason, 200)
                    )
                } else {
                    format!("\u{274c} {} ({secs}s)\n", call.name)
                };
                tracing::debug!(tool = %call.name, secs, "Sending progress complete to draft");
                let _ = tx.send(DraftEvent::Progress(progress_msg)).await;

                let _ = tx
                    .send(DraftEvent::ToolResult {
                        name: call.name.clone(),
                        output: outcome.output.clone(),
                    })
                    .await;
            }

            if outcome.success {
                let is_file_mod = matches!(
                    call.name.as_str(),
                    "file_write" | "file_edit" | "multi_edit" | "notebook_edit"
                );
                if is_file_mod {
                    if let Some(svc) = crate::services::try_get_services() {
                        let mode = *svc.coding_mode.read();
                        if let Some(nudge) =
                            crate::agent::mode_effects::file_mod_auto_verify_nudge(mode)
                        {
                            deferred_system_after_tool_batch.push(nudge.to_string());
                        }
                    }
                }
            }

            if outcome.success && call.name == "exit_plan_mode" {
                plan_nudge_state.note_exit_plan_mode_success();
                plan_finalized_this_iter = true;
            }

            let mut outcome = outcome;
            if crate::agent::plan_mode_enforcement::is_ask_question_pause(
                &call.name,
                &outcome.output,
            ) {
                awaiting_user_input = true;
                outcome.output = crate::agent::plan_mode_enforcement::ASK_QUESTION_PAUSE_NOTICE
                    .to_string();
            }

            ordered_results[idx] = Some((call.name.clone(), call.tool_call_id.clone(), outcome));
        }

        use std::hash::{Hash, Hasher};
        let mut detection_fingerprint_hasher =
            std::collections::hash_map::DefaultHasher::new();
        let mut detection_has_payload = false;

        for (result_index, (tool_name, tool_call_id, outcome)) in ordered_results
            .into_iter()
            .enumerate()
            .filter_map(|(i, opt)| opt.map(|v| (i, v)))
        {
            if !loop_ignore_tools.contains(tool_name.as_str()) {
                let args = tool_calls
                    .get(result_index)
                    .map(|c| &c.arguments)
                    .unwrap_or(&serde_json::Value::Null);

                detection_has_payload = true;
                tool_name.hash(&mut detection_fingerprint_hasher);
                crate::agent::loop_detector::canonicalise_args_string(args)
                    .hash(&mut detection_fingerprint_hasher);
                outcome.output.hash(&mut detection_fingerprint_hasher);
                let det_result = loop_detector.record(&tool_name, args, &outcome.output);
                match det_result {
                    crate::agent::loop_detector::LoopDetectionResult::Ok => {}
                    crate::agent::loop_detector::LoopDetectionResult::Warning(ref msg) => {
                        tracing::warn!(tool = %tool_name, %msg, "loop detector warning");

                        deferred_system_after_tool_batch.push(format!("[Loop Detection] {msg}"));
                    }
                    crate::agent::loop_detector::LoopDetectionResult::Block(ref msg) => {
                        tracing::warn!(tool = %tool_name, %msg, "loop detector blocked tool call");

                        deferred_system_after_tool_batch.push(format!(
                            "[Loop Detection — BLOCKED] {msg}"
                        ));
                    }
                    crate::agent::loop_detector::LoopDetectionResult::Break(msg) => {
                        runtime_trace::record_event(
                            "loop_detector_circuit_breaker",
                            Some(channel_name),
                            Some(provider_name),
                            Some(model),
                            Some(&turn_id),
                            Some(false),
                            Some(&msg),
                            serde_json::json!({
                                "iteration": iteration + 1,
                                "tool": tool_name,
                            }),
                        );
                        anyhow::bail!("Agent loop aborted by loop detector: {msg}");
                    }
                }
            }

            crate::agent::runtime_hooks::publish_tool_event(
                &tool_name,
                outcome.success,
                outcome.duration.as_millis() as u64,
            );

            if let Some(svc) = crate::services::try_get_services() {
                let props = std::collections::HashMap::from([
                    (
                        "tool".to_string(),
                        serde_json::Value::String(tool_name.clone()),
                    ),
                    (
                        "duration_ms".to_string(),
                        serde_json::json!(outcome.duration.as_millis() as u64),
                    ),
                    (
                        "success".to_string(),
                        serde_json::Value::Bool(outcome.success),
                    ),
                ]);
                let analytics = svc.analytics.clone();
                crate::runtime::spawn_supervised("agent.loop.analytics", async move {
                    analytics.log_event("tool_call", props).await;
                });
                if let Ok(mut summary) = svc.tool_use_summary.lock() {
                    summary.record(crate::services::tool_use_summary::ToolInvocation {
                        tool_name: tool_name.clone(),
                        turn: iteration as u32,
                        duration_ms: outcome.duration.as_millis() as u64,
                        success: outcome.success,
                        input_tokens: 0,
                        output_tokens: 0,
                    });
                }
            }

            individual_results.push((tool_call_id, outcome.output.clone()));
            let _ = writeln!(
                tool_results,
                "<tool_result name=\"{}\">\n{}\n</tool_result>",
                tool_name, outcome.output
            );
        }

        if !plan_finalized_this_iter
            && crate::agent::plan_mode_enforcement::detect_plan_mode_active(
                plan_mode_flag,
            )
            && !awaiting_user_input
        {
            plan_nudge_state.nudge_count += 1;
            let msg = crate::agent::plan_mode_enforcement::nudge_message(
                &plan_nudge_state,
            );
            deferred_system_after_tool_batch.push(msg.to_string());
        }

        if plan_finalized_this_iter {
            tracing::info!(
                target: "agent.plan_mode",
                turn_id = %turn_id,
                "Halting turn: exit_plan_mode succeeded; waiting for user's Build → Switch click"
            );
            let halt_text = "_Plan finalised. Waiting for the user to click \
                **Build** in the plan card to switch to Agent mode and start \
                executing._"
                .to_string();
            _turn_metrics.mark_ok();
            if let Some(ref tx) = on_delta {
                let _ = tx.send(DraftEvent::Clear).await;
                let _ = tx
                    .send(DraftEvent::Content(halt_text.clone()))
                    .await;
            }
            return Ok(halt_text);
        }

        if awaiting_user_input {
            tracing::info!(
                target: "agent.plan_mode",
                turn_id = %turn_id,
                "Pausing turn: ask_question is awaiting user reply (plan nudge suppressed)"
            );
            let pause_text =
                "_Waiting for the user's reply to the clarifying question(s) above._"
                    .to_string();

            _turn_metrics.mark_ok();
            if let Some(ref tx) = on_delta {
                let _ = tx.send(DraftEvent::Clear).await;
                let _ = tx
                    .send(DraftEvent::Content(pause_text.clone()))
                    .await;
            }
            return Ok(pause_text);
        }

        let loop_detection_active = match pacing.loop_detection_min_elapsed_secs {
            Some(min_secs) => loop_started_at.elapsed() >= Duration::from_secs(min_secs),
            None => false,
        };

        if loop_detection_active && detection_has_payload {
            let current_hash = detection_fingerprint_hasher.finish();
            let threshold = identical_output_threshold;

            if last_tool_output_hash == Some(current_hash) {
                consecutive_identical_outputs += 1;
            } else {
                consecutive_identical_outputs = 0;
                last_tool_output_hash = Some(current_hash);
            }

            if consecutive_identical_outputs >= threshold {
                let abort_msg = format!(
                    "identical tool call (name + arguments + output) detected {} consecutive times",
                    consecutive_identical_outputs
                );
                runtime_trace::record_event(
                    "tool_loop_identical_output_abort",
                    Some(channel_name),
                    Some(provider_name),
                    Some(model),
                    Some(&turn_id),
                    Some(false),
                    Some(&abort_msg),
                    serde_json::json!({
                        "iteration": iteration + 1,
                        "consecutive_identical": consecutive_identical_outputs,
                        "threshold": threshold,
                    }),
                );
                anyhow::bail!("Agent loop aborted: {abort_msg}");
            }
        }

        history.push(ChatMessage::assistant(assistant_history_content));
        if native_tool_calls.is_empty() {
            let all_results_have_ids = use_native_tools
                && !individual_results.is_empty()
                && individual_results
                    .iter()
                    .all(|(tool_call_id, _)| tool_call_id.is_some());
            if all_results_have_ids {
                for (tool_call_id, result) in &individual_results {
                    let tool_msg = serde_json::json!({
                        "tool_call_id": tool_call_id,
                        "content": result,
                    });
                    history.push(ChatMessage::tool(tool_msg.to_string()));
                }
            } else {
                history.push(ChatMessage::user(format!("[Tool results]\n{tool_results}")));
            }
        } else {
            for (native_call, (_, result)) in
                native_tool_calls.iter().zip(individual_results.iter())
            {
                let tool_msg = serde_json::json!({
                    "tool_call_id": native_call.id,
                    "content": result,
                });
                history.push(ChatMessage::tool(tool_msg.to_string()));
            }
        }

        for body in deferred_system_after_tool_batch {
            history.push(ChatMessage::system(body));
        }

        if let Some(svc) = crate::services::try_get_services() {
            let mode = *svc.coding_mode.read();
            if let Some(msg) = crate::agent::mode_effects::post_tool_batch_message(mode) {
                history.push(ChatMessage::system(msg));
            }
        }

        let pair_break_mode = crate::services::try_get_services().and_then(|svc| {
            let mode = *svc.coding_mode.read();
            mode.breaks_turn_after_tool_batch().then_some(mode)
        });
        if let Some(intercepted_mode) = pair_break_mode {
            let pair_text = "_Pair Checkpoint: tool batch complete. Pausing for your \
                input — type to continue or redirect, or press the input box to send \
                the next instruction._"
                .to_string();
            crate::agent::mode_effects::record_mode_intercept(
                crate::agent::mode_effects::ModeInterceptReason::PairCheckpoint,
                &crate::agent::mode_effects::ModeInterceptContext {
                    mode: intercepted_mode,
                    channel: Some(channel_name),
                    provider: Some(provider_name),
                    model: Some(model),
                    turn_id: Some(&turn_id),
                    tool: None,
                    tool_call_id: None,
                    iteration: Some(iteration + 1),
                    message: Some("Pair Checkpoint pause"),
                },
            );
            _turn_metrics.mark_ok();
            if let Some(ref tx) = on_delta {
                let _ = tx.send(DraftEvent::Clear).await;
                let _ = tx
                    .send(DraftEvent::Content(pair_text.clone()))
                    .await;
            }
            history.push(ChatMessage::system(
                "[Pair Checkpoint] Turn paused after tool batch. The runtime returned \
                 control to the user. The next user message will resume execution.",
            ));
            return Ok(pair_text);
        }
    }

    runtime_trace::record_event(
        "tool_loop_exhausted",
        Some(channel_name),
        Some(provider_name),
        Some(model),
        Some(&turn_id),
        Some(false),
        Some("agent exceeded maximum tool iterations"),
        serde_json::json!({
            "max_iterations": max_iterations,
        }),
    );
    anyhow::bail!("Agent exceeded maximum tool iterations ({max_iterations})")
}

pub(crate) fn build_tool_instructions(
    tools_registry: &[Box<dyn Tool>],
    tool_descriptions: Option<&ToolDescriptions>,
) -> String {
    let mut instructions = String::new();
    instructions.push_str("\n## Tool Use Protocol\n\n");
    instructions.push_str("To use a tool, wrap a JSON object in <tool_call></tool_call> tags:\n\n");
    instructions.push_str("```\n<tool_call>\n{\"name\": \"tool_name\", \"arguments\": {\"param\": \"value\"}}\n</tool_call>\n```\n\n");
    instructions.push_str(
        "CRITICAL: Output actual <tool_call> tags—never describe steps or give examples.\n\n",
    );
    instructions.push_str("Example: User says \"what's the date?\". You MUST respond with:\n<tool_call>\n{\"name\":\"shell\",\"arguments\":{\"command\":\"date\"}}\n</tool_call>\n\n");
    instructions.push_str("You may use multiple tool calls in a single response. ");
    instructions.push_str("After tool execution, results appear in <tool_result> tags. ");
    instructions
        .push_str("Continue reasoning with the results until you can give a final answer.\n\n");
    instructions.push_str("### Available Tools\n\n");

    for tool in tools_registry {
        let desc = tool_descriptions
            .and_then(|td| td.get(tool.name()))
            .unwrap_or_else(|| tool.description());
        let _ = writeln!(
            instructions,
            "**{}**: {}\nParameters: `{}`\n",
            tool.name(),
            desc,
            tool.parameters_schema()
        );
    }

    instructions
}

#[allow(clippy::too_many_lines)]
pub async fn run(
    config: Config,
    message: Option<String>,
    provider_override: Option<String>,
    model_override: Option<String>,
    temperature: f64,
    peripheral_overrides: Vec<String>,
    interactive: bool,
    session_state_file: Option<PathBuf>,
    allowed_tools: Option<Vec<String>>,
) -> Result<String> {

    let observer: Arc<dyn Observer> = crate::agent::cli_runtime::build_observer(&config);
    let runtime: Arc<dyn runtime::RuntimeAdapter> =
        crate::agent::cli_runtime::build_runtime(&config)?;
    let security = crate::agent::cli_runtime::build_security(&config);

    let _ = crate::services::init_services(
        crate::services::container::ServiceContainerConfig::default(),
    );

    if let Some(svc) = crate::services::try_get_services() {
        svc.set_max_context_tokens(config.agent.max_context_tokens);
        let rl = svc.rate_limiter.clone();
        crate::runtime::spawn_supervised("agent.loop.rate_limiter", async move {
            rl.register("llm", std::time::Duration::from_secs(60), 60)
                .await;
        });
    }

    crate::event_bus::integration::init_global_bus();

    let mem: Arc<dyn Memory> = crate::agent::cli_runtime::build_memory(&config)?;
    tracing::info!(backend = mem.name(), "Memory initialized");

    if !peripheral_overrides.is_empty() {
        tracing::info!(
            peripherals = ?peripheral_overrides,
            "Peripheral overrides from CLI (config boards take precedence)"
        );
    }

    let (composio_key, composio_entity_id) = if config.composio.enabled {
        (
            config.composio.api_key.as_deref(),
            Some(config.composio.entity_id.as_str()),
        )
    } else {
        (None, None)
    };
    let (
        mut tools_registry,
        delegate_handle,
        _reaction_handle,
        _channel_map_handle,
        _ask_user_handle,
        _escalate_handle,
        plan_mode_flag,
    ) = tools::all_tools_with_runtime(
        Arc::new(config.clone()),
        &security,
        runtime,
        mem.clone(),
        composio_key,
        composio_entity_id,
        &config.browser,
        &config.http_request,
        &config.web_fetch,
        &config.workspace_dir,
        &config.agents,
        config.api_key.as_deref(),
        &config,
        None,
    );

    let peripheral_tools: Vec<Box<dyn Tool>> =
        crate::peripherals::create_peripheral_tools(&config.peripherals).await?;
    if !peripheral_tools.is_empty() {
        tracing::info!(count = peripheral_tools.len(), "Peripheral tools added");
        tools_registry.extend(peripheral_tools);
    }
    let board_info_tools = crate::peripherals::create_board_info_tools(&config.peripherals);
    if !board_info_tools.is_empty() {
        tracing::info!(count = board_info_tools.len(), "Board info tools added");
        tools_registry.extend(board_info_tools);
    }

    if let Some(ref allow_list) = allowed_tools {
        tools_registry.retain(|t| allow_list.iter().any(|name| name == t.name()));
        tracing::info!(
            allowed = allow_list.len(),
            retained = tools_registry.len(),
            "Applied capability-based tool access filter"
        );
    }

    let mut deferred_section = String::new();
    let mut activated_handle: Option<
        std::sync::Arc<parking_lot::Mutex<crate::tools::ActivatedToolSet>>,
    > = None;
    if config.mcp.enabled && !config.mcp.servers.is_empty() {
        tracing::info!(
            "Initializing MCP client — {} server(s) configured",
            config.mcp.servers.len()
        );
        match crate::tools::McpRegistry::connect_all(&config.mcp.servers).await {
            Ok(registry) => {
                let registry = std::sync::Arc::new(registry);
                if config.mcp.deferred_loading {

                    let deferred_set = crate::tools::DeferredMcpToolSet::from_registry(
                        std::sync::Arc::clone(&registry),
                    )
                    .await;
                    tracing::info!(
                        "MCP deferred: {} tool stub(s) from {} server(s)",
                        deferred_set.len(),
                        registry.server_count()
                    );
                    deferred_section =
                        crate::tools::mcp_deferred::build_deferred_tools_section(&deferred_set);
                    let activated = std::sync::Arc::new(parking_lot::Mutex::new(
                        crate::tools::ActivatedToolSet::new(),
                    ));
                    activated_handle = Some(std::sync::Arc::clone(&activated));
                    tools_registry.push(Box::new(crate::tools::ToolSearchTool::new(
                        deferred_set,
                        activated,
                    )));
                } else {

                    let names = registry.tool_names();
                    let mut registered = 0usize;
                    for name in names {
                        if let Some(def) = registry.get_tool_def(&name).await {
                            let wrapper: std::sync::Arc<dyn Tool> =
                                std::sync::Arc::new(crate::tools::McpToolWrapper::new(
                                    name,
                                    def,
                                    std::sync::Arc::clone(&registry),
                                ));
                            if let Some(ref handle) = delegate_handle {
                                handle.write().push(std::sync::Arc::clone(&wrapper));
                            }
                            tools_registry.push(Box::new(crate::tools::ArcToolRef(wrapper)));
                            registered += 1;
                        }
                    }
                    tracing::info!(
                        "MCP: {} tool(s) registered from {} server(s)",
                        registered,
                        registry.server_count()
                    );
                }

                if let Some(svc) = crate::services::try_get_services() {
                    let mgr = svc.mcp.clone();
                    let reg = std::sync::Arc::clone(&registry);
                    crate::runtime::spawn_supervised("agent.loop.mcp_sync", async move {
                        let all_names = reg.tool_names();
                        let mut by_server: std::collections::HashMap<
                            String,
                            Vec<crate::services::mcp_manager::McpToolDef>,
                        > = std::collections::HashMap::new();
                        for prefixed in &all_names {
                            if let Some(sep) = prefixed.find("__") {
                                let srv = &prefixed[..sep];
                                if let Some(def) = reg.get_tool_def(prefixed).await {
                                    by_server.entry(srv.to_string()).or_default().push(
                                        crate::services::mcp_manager::McpToolDef {
                                            name: prefixed.clone(),
                                            description: def.description.clone(),
                                            input_schema: def.input_schema.clone(),
                                            server_name: srv.to_string(),
                                        },
                                    );
                                }
                            }
                        }
                        for (srv, tools) in by_server {
                            mgr.add_server(
                                &srv,
                                crate::services::mcp_manager::McpTransport::Stdio {
                                    command: String::new(),
                                    args: Vec::new(),
                                    env: std::collections::HashMap::new(),
                                },
                            )
                            .await;
                            mgr.set_server_status(
                                &srv,
                                crate::services::mcp_manager::McpServerStatus::Connected,
                                None,
                            )
                            .await;
                            mgr.set_server_tools(&srv, tools).await;
                        }
                    });
                }
            }
            Err(e) => {
                tracing::error!("MCP registry failed to initialize: {e:#}");
            }
        }
    }

    let mut deferred_builtin_set = crate::tools::DeferredBuiltinToolSet::new();
    if config.agent.builtin_tool_deferred_loading {
        let core: HashSet<&str> = crate::tools::BUILTIN_CORE_TOOL_NAMES.iter().copied().collect();
        for tool_box in tools_registry.iter() {
            let name = tool_box.name();
            if core.contains(name) {
                continue;
            }
            if name == "tool_search" || name.contains("__") || name.starts_with("custom_") {
                continue;
            }
            deferred_builtin_set.add_spec(tool_box.spec());
        }
        if !deferred_builtin_set.is_empty() {
            tracing::info!(
                "Builtin deferred: {} tool stub(s)",
                deferred_builtin_set.len()
            );
            let builtin_section =
                crate::tools::build_deferred_builtin_section(&deferred_builtin_set);
            if !deferred_section.is_empty() {
                deferred_section.push('\n');
            }
            deferred_section.push_str(&builtin_section);
            if let Some(handle) = activated_handle.as_ref() {
                tools_registry.retain(|t| t.name() != "tool_search");
                tools_registry.push(Box::new(
                    crate::tools::ToolSearchTool::new(
                        crate::tools::DeferredMcpToolSet {
                            stubs: Vec::new(),
                            registry: std::sync::Arc::new(crate::tools::McpRegistry::empty()),
                        },
                        std::sync::Arc::clone(handle),
                    )
                    .with_builtin(deferred_builtin_set.clone()),
                ));
            } else {
                let activated = std::sync::Arc::new(parking_lot::Mutex::new(
                    crate::tools::ActivatedToolSet::new(),
                ));
                activated_handle = Some(std::sync::Arc::clone(&activated));
                tools_registry.push(Box::new(crate::tools::ToolSearchTool::new_builtin_only(
                    deferred_builtin_set.clone(),
                    activated,
                )));
            }
        }
    }
    if let Some(svc) = crate::services::try_get_services() {
        let mut guard = svc.deferred_builtin_names.write();
        guard.clear();
        for stub in &deferred_builtin_set.stubs {
            guard.insert(stub.name.clone());
        }
    }

    let mut provider_name = provider_override
        .as_deref()
        .or(config.default_provider.as_deref())
        .unwrap_or("openrouter")
        .to_string();

    let mut model_name = model_override
        .as_deref()
        .or(config.default_model.as_deref())
        .unwrap_or("anthropic/claude-sonnet-4")
        .to_string();

    let provider_runtime_options = providers::provider_runtime_options_from_config(&config);

    let mut provider: std::sync::Arc<dyn Provider> =
        std::sync::Arc::from(providers::create_routed_provider_with_options(
            &provider_name,
            config.api_key.as_deref(),
            config.api_url.as_deref(),
            &config.reliability,
            &config.model_routes,
            &model_name,
            &provider_runtime_options,
        )?);

    let _model_switch_callback = get_model_switch_state();

    observer.record_event(&ObserverEvent::AgentStart {
        provider: provider_name.to_string(),
        model: model_name.to_string(),
    });

    crate::agent::runtime_hooks::publish_lifecycle_event("started");

    let mut query_engine =
        crate::query::QueryEngine::new(config.agent.max_context_tokens as u32, 4096);
    for hook in crate::query::standard_stop_hooks(0.9) {
        query_engine.add_stop_hook(hook);
    }

    if crate::agent::multi_agent_runtime::global_runtime().is_none() {
        let _ = crate::agent::multi_agent_runtime::init_global_runtime();
    }

    let hardware_rag: Option<crate::rag::HardwareRag> = config
        .peripherals
        .datasheet_dir
        .as_ref()
        .filter(|d| !d.trim().is_empty())
        .map(|dir| crate::rag::HardwareRag::load(&config.workspace_dir, dir.trim()))
        .and_then(Result::ok)
        .filter(|r: &crate::rag::HardwareRag| !r.is_empty());
    if let Some(ref rag) = hardware_rag {
        tracing::info!(chunks = rag.len(), "Hardware RAG loaded");
    }

    let board_names: Vec<String> = config
        .peripherals
        .boards
        .iter()
        .map(|b| b.board.clone())
        .collect();

    let i18n_locale = config
        .locale
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(crate::i18n::detect_locale);
    let i18n_search_dirs = crate::i18n::default_search_dirs(&config.workspace_dir);
    let i18n_descs = crate::i18n::ToolDescriptions::load(&i18n_locale, &i18n_search_dirs);

    let skills = crate::skills::load_skills_with_config(&config.workspace_dir, &config);

    tools::register_skill_tools(&mut tools_registry, &skills, security.clone());

    let mut tool_descs: Vec<(&str, &str)> = vec![
        (
            "shell",
            "Execute terminal commands. Use when: running local checks, build/test commands, diagnostics. Don't use when: a safer dedicated tool exists, or command is destructive without approval.",
        ),
        (
            "file_read",
            "Read file contents. Use when: inspecting project files, configs, logs. Don't use when: a targeted search is enough.",
        ),
        (
            "file_write",
            "Write file contents. Use when: applying focused edits, scaffolding files, updating docs/code. Don't use when: side effects are unclear or file ownership is uncertain.",
        ),
        (
            "memory_store",
            "Save to memory. Use when: preserving durable preferences, decisions, key context. Don't use when: information is transient/noisy/sensitive without need.",
        ),
        (
            "memory_recall",
            "Search memory. Use when: retrieving prior decisions, user preferences, historical context. Don't use when: answer is already in current context.",
        ),
        (
            "memory_forget",
            "Delete a memory entry. Use when: memory is incorrect/stale or explicitly requested for removal. Don't use when: impact is uncertain.",
        ),
    ];
    if matches!(
        config.skills.prompt_injection_mode,
        crate::config::SkillsPromptInjectionMode::Compact
    ) {
        tool_descs.push((
            "read_skill",
            "Load the full source for an available skill by name. Use when: compact mode only shows a summary and you need the complete skill instructions.",
        ));
    }
    tool_descs.push((
        "cron_add",
        "Create a cron job. Supports schedule kinds: cron, at, every; and job types: shell or agent.",
    ));
    tool_descs.push((
        "cron_list",
        "List all cron jobs with schedule, status, and metadata.",
    ));
    tool_descs.push(("cron_remove", "Remove a cron job by job_id."));
    tool_descs.push((
        "cron_update",
        "Patch a cron job (schedule, enabled, command/prompt, model, delivery, session_target).",
    ));
    tool_descs.push((
        "cron_run",
        "Force-run a cron job immediately and record a run history entry.",
    ));
    tool_descs.push(("cron_runs", "Show recent run history for a cron job."));
    tool_descs.push((
        "screenshot",
        "Capture a screenshot of the current screen. Returns file path and base64-encoded PNG. Use when: visual verification, UI inspection, debugging displays.",
    ));
    tool_descs.push((
        "image_info",
        "Read image file metadata (format, dimensions, size) and optionally base64-encode it. Use when: inspecting images, preparing visual data for analysis.",
    ));
    if config.browser.enabled {
        tool_descs.push((
            "browser_open",
            "Open approved HTTPS URLs in system browser (allowlist-only, no scraping)",
        ));
    }
    if config.composio.enabled {
        tool_descs.push((
            "composio",
            "Execute actions on 1000+ apps via Composio (Gmail, Notion, GitHub, Slack, etc.). Use action='list' to discover, 'execute' to run (optionally with connected_account_id), 'connect' to OAuth.",
        ));
    }
    tool_descs.push((
        "schedule",
        "Manage scheduled tasks (create/list/get/cancel/pause/resume). Supports recurring cron and one-shot delays.",
    ));
    tool_descs.push((
        "model_routing_config",
        "Configure default model, scenario routing, and delegate agents. Use for natural-language requests like: 'set conversation to kimi and coding to gpt-5.3-codex'.",
    ));
    if !config.agents.is_empty() {
        tool_descs.push((
            "delegate",
            "Delegate a sub-task to a specialized agent. Use when: task needs different model/capability, or to parallelize work.",
        ));
    }
    if config.peripherals.enabled && !config.peripherals.boards.is_empty() {
        tool_descs.push((
            "gpio_read",
            "Read GPIO pin value (0 or 1) on connected hardware (STM32, Arduino). Use when: checking sensor/button state, LED status.",
        ));
        tool_descs.push((
            "gpio_write",
            "Set GPIO pin high (1) or low (0) on connected hardware. Use when: turning LED on/off, controlling actuators.",
        ));
        tool_descs.push((
            "arduino_upload",
            "Upload agent-generated Arduino sketch. Use when: user asks for 'make a heart', 'blink pattern', or custom LED behavior on Arduino. You write the full .ino code; SenWeaverCoding compiles and uploads it. Pin 13 = built-in LED on Uno.",
        ));
        tool_descs.push((
            "hardware_memory_map",
            "Return flash and RAM address ranges for connected hardware. Use when: user asks for 'upper and lower memory addresses', 'memory map', or 'readable addresses'.",
        ));
        tool_descs.push((
            "hardware_board_info",
            "Return full board info (chip, architecture, memory map) for connected hardware. Use when: user asks for 'board info', 'what board do I have', 'connected hardware', 'chip info', or 'what hardware'.",
        ));
        tool_descs.push((
            "hardware_memory_read",
            "Read actual memory/register values from Nucleo via USB. Use when: user asks to 'read register values', 'read memory', 'dump lower memory 0-126', 'give address and value'. Params: address (hex, default 0x20000000), length (bytes, default 128).",
        ));
        tool_descs.push((
            "hardware_capabilities",
            "Query connected hardware for reported GPIO pins and LED pin. Use when: user asks what pins are available.",
        ));
    }
    let bootstrap_max_chars = if config.agent.compact_context {
        Some(6000)
    } else {
        None
    };
    let native_tools = provider.supports_native_tools();
    let coding_mode_label_owned = crate::services::try_get_services()
        .map(|svc| svc.coding_mode.read().label().to_string());
    let mut system_prompt = crate::channels::build_system_prompt_with_mode_and_autonomy(
        &config.workspace_dir,
        &model_name,
        &tool_descs,
        &skills,
        Some(&config.identity),
        bootstrap_max_chars,
        Some(&config.autonomy),
        native_tools,
        config.skills.prompt_injection_mode,
        config.agent.compact_context,
        config.agent.max_system_prompt_chars,
        Some(&config.agent),
        coding_mode_label_owned.as_deref(),
    );

    if !native_tools {
        system_prompt.push_str(&build_tool_instructions(&tools_registry, Some(&i18n_descs)));
    }

    if !deferred_section.is_empty() {
        system_prompt.push('\n');
        system_prompt.push_str(&deferred_section);
    }

    if let Some(svc) = crate::services::try_get_services() {
        let mem_prompt = svc.session_memory.build_memory_prompt(500).await;
        if !mem_prompt.is_empty() {
            system_prompt.push('\n');
            system_prompt.push_str(&mem_prompt);
        }
    }

    if let Some(svc) = crate::services::try_get_services() {
        let mode = *svc.coding_mode.read();
        let mode_prompt = mode.system_prompt_injection();
        system_prompt.push_str(&mode_prompt);
    }

    let approval_manager = if interactive {
        Some(ApprovalManager::from_config(&config.autonomy))
    } else {
        None
    };
    let channel_name = if interactive { "cli" } else { "daemon" };
    let memory_session_id = session_state_file
        .as_deref()
        .and_then(memory_session_id_from_state_file);

    let start = Instant::now();

    let mut final_output = String::new();

    let base_system_prompt = system_prompt.clone();

    if let Some(msg) = message {

        let (thinking_directive, effective_msg) =
            match crate::agent::thinking::parse_thinking_directive(&msg) {
                Some((level, remaining)) => {
                    tracing::info!(thinking_level = ?level, "Thinking directive parsed from message");
                    (Some(level), remaining)
                }
                None => (None, msg.clone()),
            };
        let thinking_level = crate::agent::thinking::resolve_thinking_level(
            thinking_directive,
            None,
            &config.agent.thinking,
        );
        let thinking_params = crate::agent::thinking::apply_thinking_level(thinking_level);
        let effective_temperature = crate::agent::thinking::clamp_temperature(
            temperature + thinking_params.temperature_adjustment,
        );

        if let Some(ref prefix) = thinking_params.system_prompt_prefix {
            system_prompt = format!("{prefix}\n\n{system_prompt}");
        }

        if config.memory.auto_save
            && effective_msg.chars().count() >= AUTOSAVE_MIN_MESSAGE_CHARS
            && !memory::should_skip_autosave_content(&effective_msg)
        {
            let user_key = autosave_memory_key("user_msg");
            let _ = mem
                .store(
                    &user_key,
                    &effective_msg,
                    MemoryCategory::Conversation,
                    memory_session_id.as_deref(),
                )
                .await;
        }

        let mem_context = build_context(
            mem.as_ref(),
            &effective_msg,
            config.memory.min_relevance_score,
            memory_session_id.as_deref(),
        )
        .await;
        let hw_context = if !board_names.is_empty() {
            let rag_limit = if config.agent.compact_context { 2 } else { 5 };
            hardware_rag
                .as_ref()
                .map(|r| build_hardware_context(r, &effective_msg, &board_names, rag_limit))
                .unwrap_or_default()
        } else {
            String::new()
        };
        let context = format!("{mem_context}{hw_context}");
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z");
        let enriched = if context.is_empty() {
            format!("[{now}] {effective_msg}")
        } else {
            format!("{context}[{now}] {effective_msg}")
        };

        let mut history = vec![
            ChatMessage::system(&system_prompt),
            ChatMessage::user(&enriched),
        ];

        if config.agent.history_pruning.enabled {
            let _stats = crate::agent::history_pruner::prune_history(
                &mut history,
                &config.agent.history_pruning,
            );
        }

        let excluded_tools = compute_excluded_mcp_tools(
            &tools_registry,
            &config.agent.tool_filter_groups,
            &effective_msg,
        );

        let rbac_engine_cell = if config.rbac.enabled {
            Some(std::sync::Arc::new(crate::security::rbac::RbacEngine::new(
                config.rbac.clone(),
                &config.workspace_dir,
            )))
        } else {
            None
        };
        let rbac_cli_identity = crate::security::rbac::CallerIdentity::cli_operator();
        let rbac_engine_ref = rbac_engine_cell.as_ref();
        let rbac_identity_ref = rbac_engine_ref.map(|_| &rbac_cli_identity);

        let model_switch_callback = get_model_switch_state();
        let response = scope_model_switch(async {
            let mut current_provider = std::sync::Arc::clone(&provider);
            let mut current_provider_name = provider_name.to_string();
            let mut current_model_name = model_name.to_string();

            loop {
                let result = run_tool_call_loop(
                    current_provider.as_ref(),
                    &mut history,
                    &tools_registry,
                    observer.as_ref(),
                    &current_provider_name,
                    &current_model_name,
                    effective_temperature,
                    false,
                    approval_manager.as_ref(),
                    channel_name,
                    None,
                    &config.multimodal,
                    config.agent.max_tool_iterations,
                    None,
                    None,
                    None,
                    &excluded_tools,
                    &config.agent.tool_call_dedup_exempt,
                    activated_handle.as_ref(),
                    Some(model_switch_callback.clone()),
                    &config.pacing,
                    rbac_engine_ref,
                    rbac_identity_ref,
                    Some(&plan_mode_flag),
                    None,
                )
                .await;

                match result {
                    Ok(resp) => return Ok::<String, anyhow::Error>(resp),
                    Err(e) => {
                        if let Some((new_provider_name, new_model_name)) =
                            is_model_switch_requested(&e)
                        {
                            tracing::info!(
                                "Model switch: {} {} -> {} {}",
                                current_provider_name,
                                current_model_name,
                                new_provider_name,
                                new_model_name
                            );
                            match providers::create_routed_provider_with_options(
                                &new_provider_name,
                                config.api_key.as_deref(),
                                config.api_url.as_deref(),
                                &config.reliability,
                                &config.model_routes,
                                &new_model_name,
                                &provider_runtime_options,
                            ) {
                                Ok(new_provider) => {
                                    current_provider = std::sync::Arc::from(new_provider);
                                    current_provider_name = new_provider_name;
                                    current_model_name = new_model_name;
                                    clear_model_switch_request();
                                    continue;
                                }
                                Err(e) => return Err(e),
                            }
                        }
                        return Err(e);
                    }
                }
            }
        })
        .await?;

        #[cfg(feature = "skill-creation")]
        if config.skills.skill_creation.enabled {
            let tool_calls = crate::skills::creator::extract_tool_calls_from_history(&history);
            if tool_calls.len() >= 2 {
                let creator = crate::skills::creator::SkillCreator::new(
                    config.workspace_dir.clone(),
                    config.skills.skill_creation.clone(),
                );
                match creator
                    .create_from_execution(&effective_msg, &tool_calls, None)
                    .await
                {
                    Ok(Some(slug)) => {
                        tracing::info!(slug, "Auto-created skill from execution");
                    }
                    Ok(None) => {
                        tracing::debug!("Skill creation skipped (duplicate or disabled)");
                    }
                    Err(e) => tracing::warn!("Skill creation failed: {e}"),
                }
            }
        }

        final_output = response.clone();
        println!("{final_output}");
        observer.record_event(&ObserverEvent::TurnComplete);
        return Ok(final_output);
    }

    if message.is_none() {
        println!("🦀 SenWeaverCoding Interactive Mode");
        println!("Type /help for commands.\n");
        let _cli = crate::channels::CliChannel::new();
        let _command_registry = crate::services::container::register_all_commands();

        let mut history = if let Some(path) = session_state_file.as_deref() {
            load_interactive_session_history(path, &system_prompt)?
        } else {
            vec![ChatMessage::system(&system_prompt)]
        };

        let mut interactive_turn_count: u32 = 0;

        loop {

            let prompt_prefix = {
                let model_hint = model_name.split('/').last().unwrap_or(&model_name);
                let cost_hint = if let Some(bs) = crate::bootstrap::try_get_state() {
                    let mut cost = 0.0f64;
                    bs.read(|state| cost = state.total_cost_usd);
                    if cost > 0.0 {
                        format!(" ${cost:.3}")
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                let token_hint = {
                    let used = estimate_history_tokens(&history);
                    let max_ctx = config.agent.max_context_tokens;
                    let remaining = max_ctx.saturating_sub(used);
                    let remaining_k = remaining / 1000;
                    format!(" \x1b[2m{remaining_k}k\x1b[0m")
                };
                let mode_badge = if let Some(svc) = crate::services::try_get_services() {
                    let mode = *svc.coding_mode.read();
                    if mode != crate::agent::coding_mode::CodingMode::Vibe {
                        format!(" \x1b[1;33m[{}]\x1b[0m", mode.display_name())
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                let vim_badge = if crate::commands::vim::is_vim_enabled() {
                    " \x1b[1;35m[VIM]\x1b[0m"
                } else {
                    ""
                };
                format!(
                    "\x1b[1;36m{model_hint}{cost_hint}\x1b[0m{token_hint}{mode_badge}{vim_badge} \x1b[1;32m>\x1b[0m "
                )
            };
            print!("{prompt_prefix}");
            let _ = std::io::stdout().flush();

            let mut full_input = String::new();
            loop {
                let mut raw = Vec::new();
                match std::io::BufRead::read_until(&mut std::io::stdin().lock(), b'\n', &mut raw) {
                    Ok(0) => return Ok(final_output),
                    Ok(_) => {
                        let line = String::from_utf8_lossy(&raw);
                        if full_input.is_empty() && line.trim_end().ends_with('\\') {
                            full_input.push_str(line.trim_end().trim_end_matches('\\'));
                            full_input.push('\n');
                            continue;
                        }
                        full_input.push_str(&line);
                        break;
                    }
                    Err(e) => {
                        eprintln!("Read error: {e}");
                        return Err(anyhow::anyhow!("{e}"));
                    }
                }
            }

            let effective_input = full_input.trim().to_string();
            if effective_input.is_empty() {
                continue;
            }

            if effective_input == "/quit" || effective_input == "/exit" {
                break;
            }

            interactive_turn_count += 1;
            if interactive_turn_count > 1 {
                system_prompt = base_system_prompt.clone();
            }

            let thinking_level =
                crate::agent::thinking::resolve_thinking_level(None, None, &config.agent.thinking);
            let thinking_params = crate::agent::thinking::apply_thinking_level(thinking_level);
            let effective_temperature = crate::agent::thinking::clamp_temperature(
                temperature + thinking_params.temperature_adjustment,
            );
            if let Some(ref prefix) = thinking_params.system_prompt_prefix {
                system_prompt = format!("{prefix}\n\n{system_prompt}");
            }

            let mem_context = build_context(
                mem.as_ref(),
                &effective_input,
                config.memory.min_relevance_score,
                memory_session_id.as_deref(),
            )
            .await;
            let hw_context = if !board_names.is_empty() {
                let rag_limit = if config.agent.compact_context { 2 } else { 5 };
                hardware_rag
                    .as_ref()
                    .map(|r| build_hardware_context(r, &effective_input, &board_names, rag_limit))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            let extra_dir_context = String::new();
            let context = format!("{mem_context}{hw_context}{extra_dir_context}");
            let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z");
            let enriched = if context.is_empty() {
                format!("[{now}] {effective_input}")
            } else {
                format!("{context}[{now}] {effective_input}")
            };

            history.push(ChatMessage::user(&enriched));

            let excluded_tools = compute_excluded_mcp_tools(
                &tools_registry,
                &config.agent.tool_filter_groups,
                &effective_input,
            );

            let rbac_engine_cell = if config.rbac.enabled {
                Some(std::sync::Arc::new(crate::security::rbac::RbacEngine::new(
                    config.rbac.clone(),
                    &config.workspace_dir,
                )))
            } else {
                None
            };
            let rbac_cli_identity = crate::security::rbac::CallerIdentity::cli_operator();
            let rbac_engine_ref = rbac_engine_cell.as_ref();
            let rbac_identity_ref = rbac_engine_ref.map(|_| &rbac_cli_identity);

            let (delta_tx, mut delta_rx) = tokio::sync::mpsc::channel::<DraftEvent>(64);
            let content_was_streamed =
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let content_was_streamed_clone = content_was_streamed.clone();
            let is_tty = std::io::IsTerminal::is_terminal(&std::io::stderr());

            let consumer_handle =
                crate::runtime::spawn_supervised("agent.loop.cli_consumer", async move {
                    use std::io::Write;
                    const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
                    let mut spinner_active = is_tty;
                    let content_was_streamed = content_was_streamed_clone;
                    if spinner_active {
                        let _ = write!(std::io::stderr(), "\x1b[2m{} Thinking…\x1b[0m", SPINNER[0]);
                        let _ = std::io::stderr().flush();
                    }
                    loop {
                        match tokio::time::timeout(
                            std::time::Duration::from_millis(80),
                            delta_rx.recv(),
                        )
                        .await
                        {
                            Ok(Some(event)) => {
                                if spinner_active {
                                    let _ = write!(std::io::stderr(), "\r\x1b[K");
                                    let _ = std::io::stderr().flush();
                                    spinner_active = false;
                                }
                                match event {
                                    DraftEvent::Clear => {
                                        let _ = writeln!(std::io::stderr());
                                    }
                                    DraftEvent::Progress(text) => {
                                        let _ = write!(
                                            std::io::stderr(),
                                            "\r\x1b[K\x1b[2m{text}\x1b[0m"
                                        );
                                        let _ = std::io::stderr().flush();
                                    }
                                    DraftEvent::Content(text) => {
                                        content_was_streamed
                                            .store(true, std::sync::atomic::Ordering::Relaxed);
                                        print!("{text}");
                                        let _ = std::io::stdout().flush();
                                    }

                                    _ => {}
                                }
                            }
                            Ok(None) | Err(_) => {
                                if spinner_active {
                                    let _ =
                                        write!(std::io::stderr(), "\r\x1b[2mThinking… done\x1b[0m");
                                    let _ = writeln!(std::io::stderr());
                                    let _ = std::io::stderr().flush();
                                }
                                break;
                            }
                        }
                    }
                });

            let model_switch_callback = get_model_switch_state();
            let response = loop {
                match run_tool_call_loop(
                    provider.as_ref(),
                    &mut history,
                    &tools_registry,
                    observer.as_ref(),
                    &provider_name,
                    &model_name,
                    effective_temperature,
                    true,
                    None,
                    channel_name,
                    None,
                    &config.multimodal,
                    config.agent.max_tool_iterations,
                    None,
                    Some(delta_tx.clone()),
                    None,
                    &excluded_tools,
                    &config.agent.tool_call_dedup_exempt,
                    activated_handle.as_ref(),
                    Some(model_switch_callback.clone()),
                    &config.pacing,
                    rbac_engine_ref,
                    rbac_identity_ref,
                    None,
                    None,
                )
                .await
                {
                    Ok(resp) => break resp,
                    Err(e) => {
                        if let Some((new_provider, new_model)) = is_model_switch_requested(&e) {
                            tracing::info!(
                                "Model switch: {} {} -> {} {}",
                                provider_name,
                                model_name,
                                new_provider,
                                new_model
                            );
                            provider = std::sync::Arc::from(
                                providers::create_routed_provider_with_options(
                                    &new_provider,
                                    config.api_key.as_deref(),
                                    config.api_url.as_deref(),
                                    &config.reliability,
                                    &config.model_routes,
                                    &new_model,
                                    &provider_runtime_options,
                                )?,
                            );
                            provider_name = new_provider;
                            model_name = new_model;
                            clear_model_switch_request();
                            continue;
                        }
                        eprintln!("Error: {e}");
                        break String::new();
                    }
                }
            };

            consumer_handle.abort();

            if !response.is_empty() {
                if !content_was_streamed.load(std::sync::atomic::Ordering::Relaxed) {
                    println!("{response}");
                }
                history.push(ChatMessage::assistant(&response));
            }

            if let Some(path) = session_state_file.as_deref() {
                let _ = save_interactive_session_history(path, &history);
            }
        }
    }

    let duration = start.elapsed();
    observer.record_event(&crate::observability::traits::ObserverEvent::AgentEnd {
        provider: provider_name.to_string(),
        model: model_name.to_string(),
        duration,
        tokens_used: None,
        cost_usd: None,
    });

    crate::agent::runtime_hooks::publish_lifecycle_event("stopped");

    Ok(final_output)
}

pub async fn process_message(
    config: Config,
    message: &str,
    session_id: Option<&str>,
) -> Result<String> {
    let observer: Arc<dyn Observer> =
        Arc::from(observability::create_observer(&config.observability));
    let runtime: Arc<dyn runtime::RuntimeAdapter> =
        Arc::from(runtime::create_runtime(&config.runtime)?);
    let security = Arc::new(SecurityPolicy::from_config(
        &config.autonomy,
        &config.workspace_dir,
    ));
    let approval_manager = ApprovalManager::for_non_interactive(&config.autonomy);
    let mem: Arc<dyn Memory> = Arc::from(memory::create_memory_with_storage_and_routes(
        &config.memory,
        &config.embedding_routes,
        Some(&config.storage.provider.config),
        &config.workspace_dir,
        config.api_key.as_deref(),
    )?);

    let (composio_key, composio_entity_id) = if config.composio.enabled {
        (
            config.composio.api_key.as_deref(),
            Some(config.composio.entity_id.as_str()),
        )
    } else {
        (None, None)
    };
    let (
        mut tools_registry,
        delegate_handle_pm,
        _reaction_handle_pm,
        _channel_map_handle_pm,
        _ask_user_handle_pm,
        _escalate_handle_pm,
        _plan_mode_flag_pm,
    ) = tools::all_tools_with_runtime(
        Arc::new(config.clone()),
        &security,
        runtime,
        mem.clone(),
        composio_key,
        composio_entity_id,
        &config.browser,
        &config.http_request,
        &config.web_fetch,
        &config.workspace_dir,
        &config.agents,
        config.api_key.as_deref(),
        &config,
        None,
    );
    let peripheral_tools: Vec<Box<dyn Tool>> =
        crate::peripherals::create_peripheral_tools(&config.peripherals).await?;
    tools_registry.extend(peripheral_tools);

    let mut deferred_section = String::new();
    let mut activated_handle_pm: Option<
        std::sync::Arc<parking_lot::Mutex<crate::tools::ActivatedToolSet>>,
    > = None;
    if config.mcp.enabled && !config.mcp.servers.is_empty() {
        tracing::info!(
            "Initializing MCP client — {} server(s) configured",
            config.mcp.servers.len()
        );
        match crate::tools::McpRegistry::connect_all(&config.mcp.servers).await {
            Ok(registry) => {
                let registry = std::sync::Arc::new(registry);
                if config.mcp.deferred_loading {
                    let deferred_set = crate::tools::DeferredMcpToolSet::from_registry(
                        std::sync::Arc::clone(&registry),
                    )
                    .await;
                    tracing::info!(
                        "MCP deferred: {} tool stub(s) from {} server(s)",
                        deferred_set.len(),
                        registry.server_count()
                    );
                    deferred_section =
                        crate::tools::mcp_deferred::build_deferred_tools_section(&deferred_set);
                    let activated = std::sync::Arc::new(parking_lot::Mutex::new(
                        crate::tools::ActivatedToolSet::new(),
                    ));
                    activated_handle_pm = Some(std::sync::Arc::clone(&activated));
                    tools_registry.push(Box::new(crate::tools::ToolSearchTool::new(
                        deferred_set,
                        activated,
                    )));
                } else {
                    let names = registry.tool_names();
                    let mut registered = 0usize;
                    for name in names {
                        if let Some(def) = registry.get_tool_def(&name).await {
                            let wrapper: std::sync::Arc<dyn Tool> =
                                std::sync::Arc::new(crate::tools::McpToolWrapper::new(
                                    name,
                                    def,
                                    std::sync::Arc::clone(&registry),
                                ));
                            if let Some(ref handle) = delegate_handle_pm {
                                handle.write().push(std::sync::Arc::clone(&wrapper));
                            }
                            tools_registry.push(Box::new(crate::tools::ArcToolRef(wrapper)));
                            registered += 1;
                        }
                    }
                    tracing::info!(
                        "MCP: {} tool(s) registered from {} server(s)",
                        registered,
                        registry.server_count()
                    );
                }
            }
            Err(e) => {
                tracing::error!("MCP registry failed to initialize: {e:#}");
            }
        }
    }

    let mut deferred_builtin_set_pm = crate::tools::DeferredBuiltinToolSet::new();
    if config.agent.builtin_tool_deferred_loading {
        let core: HashSet<&str> = crate::tools::BUILTIN_CORE_TOOL_NAMES.iter().copied().collect();
        for tool_box in tools_registry.iter() {
            let name = tool_box.name();
            if core.contains(name) {
                continue;
            }
            if name == "tool_search" || name.contains("__") || name.starts_with("custom_") {
                continue;
            }
            deferred_builtin_set_pm.add_spec(tool_box.spec());
        }
        if !deferred_builtin_set_pm.is_empty() {
            tracing::info!(
                "Builtin deferred (process_message): {} tool stub(s)",
                deferred_builtin_set_pm.len()
            );
            let builtin_section =
                crate::tools::build_deferred_builtin_section(&deferred_builtin_set_pm);
            if !deferred_section.is_empty() {
                deferred_section.push('\n');
            }
            deferred_section.push_str(&builtin_section);
            if let Some(handle) = activated_handle_pm.as_ref() {
                tools_registry.retain(|t| t.name() != "tool_search");
                tools_registry.push(Box::new(
                    crate::tools::ToolSearchTool::new(
                        crate::tools::DeferredMcpToolSet {
                            stubs: Vec::new(),
                            registry: std::sync::Arc::new(crate::tools::McpRegistry::empty()),
                        },
                        std::sync::Arc::clone(handle),
                    )
                    .with_builtin(deferred_builtin_set_pm.clone()),
                ));
            } else {
                let activated = std::sync::Arc::new(parking_lot::Mutex::new(
                    crate::tools::ActivatedToolSet::new(),
                ));
                activated_handle_pm = Some(std::sync::Arc::clone(&activated));
                tools_registry.push(Box::new(crate::tools::ToolSearchTool::new_builtin_only(
                    deferred_builtin_set_pm.clone(),
                    activated,
                )));
            }
        }
    }
    if let Some(svc) = crate::services::try_get_services() {
        let mut guard = svc.deferred_builtin_names.write();
        guard.clear();
        for stub in &deferred_builtin_set_pm.stubs {
            guard.insert(stub.name.clone());
        }
    }

    let provider_name = config.default_provider.as_deref().unwrap_or("openrouter");
    let model_name = config
        .default_model
        .clone()
        .unwrap_or_else(|| "anthropic/claude-sonnet-4-20250514".into());
    let provider_runtime_options = providers::provider_runtime_options_from_config(&config);
    let provider: std::sync::Arc<dyn Provider> =
        std::sync::Arc::from(providers::create_routed_provider_with_options(
            provider_name,
            config.api_key.as_deref(),
            config.api_url.as_deref(),
            &config.reliability,
            &config.model_routes,
            &model_name,
            &provider_runtime_options,
        )?);

    let hardware_rag: Option<crate::rag::HardwareRag> = config
        .peripherals
        .datasheet_dir
        .as_ref()
        .filter(|d| !d.trim().is_empty())
        .map(|dir| crate::rag::HardwareRag::load(&config.workspace_dir, dir.trim()))
        .and_then(Result::ok)
        .filter(|r: &crate::rag::HardwareRag| !r.is_empty());
    let board_names: Vec<String> = config
        .peripherals
        .boards
        .iter()
        .map(|b| b.board.clone())
        .collect();

    let i18n_locale = config
        .locale
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(crate::i18n::detect_locale);
    let i18n_search_dirs = crate::i18n::default_search_dirs(&config.workspace_dir);
    let i18n_descs = crate::i18n::ToolDescriptions::load(&i18n_locale, &i18n_search_dirs);

    let skills = crate::skills::load_skills_with_config(&config.workspace_dir, &config);

    tools::register_skill_tools(&mut tools_registry, &skills, security.clone());

    let mut tool_descs: Vec<(&str, &str)> = vec![
        ("shell", "Execute terminal commands."),
        ("file_read", "Read file contents."),
        ("file_write", "Write file contents."),
        ("memory_store", "Save to memory."),
        ("memory_recall", "Search memory."),
        ("memory_forget", "Delete a memory entry."),
        (
            "model_routing_config",
            "Configure default model, scenario routing, and delegate agents.",
        ),
        ("screenshot", "Capture a screenshot."),
        ("image_info", "Read image metadata."),
    ];
    if matches!(
        config.skills.prompt_injection_mode,
        crate::config::SkillsPromptInjectionMode::Compact
    ) {
        tool_descs.push((
            "read_skill",
            "Load the full source for an available skill by name.",
        ));
    }
    if config.browser.enabled {
        tool_descs.push(("browser_open", "Open approved URLs in browser."));
    }
    if config.composio.enabled {
        tool_descs.push(("composio", "Execute actions on 1000+ apps via Composio."));
    }
    if config.peripherals.enabled && !config.peripherals.boards.is_empty() {
        tool_descs.push(("gpio_read", "Read GPIO pin value on connected hardware."));
        tool_descs.push((
            "gpio_write",
            "Set GPIO pin high or low on connected hardware.",
        ));
        tool_descs.push((
            "arduino_upload",
            "Upload Arduino sketch. Use for 'make a heart', custom patterns. You write full .ino code; SenWeaverCoding uploads it.",
        ));
        tool_descs.push((
            "hardware_memory_map",
            "Return flash and RAM address ranges. Use when user asks for memory addresses or memory map.",
        ));
        tool_descs.push((
            "hardware_board_info",
            "Return full board info (chip, architecture, memory map). Use when user asks for board info, what board, connected hardware, or chip info.",
        ));
        tool_descs.push((
            "hardware_memory_read",
            "Read actual memory/register values from Nucleo. Use when user asks to read registers, read memory, dump lower memory 0-126, or give address and value.",
        ));
        tool_descs.push((
            "hardware_capabilities",
            "Query connected hardware for reported GPIO pins and LED pin. Use when user asks what pins are available.",
        ));
    }

    if config.autonomy.level != AutonomyLevel::Full {
        let excluded = &config.autonomy.non_cli_excluded_tools;
        if !excluded.is_empty() {
            tool_descs.retain(|(name, _)| !excluded.iter().any(|ex| ex == name));
        }
    }

    let bootstrap_max_chars = if config.agent.compact_context {
        Some(6000)
    } else {
        None
    };
    let native_tools = provider.supports_native_tools();
    let coding_mode_label_owned = crate::services::try_get_services()
        .map(|svc| svc.coding_mode.read().label().to_string());
    let mut system_prompt = crate::channels::build_system_prompt_with_mode_and_autonomy(
        &config.workspace_dir,
        &model_name,
        &tool_descs,
        &skills,
        Some(&config.identity),
        bootstrap_max_chars,
        Some(&config.autonomy),
        native_tools,
        config.skills.prompt_injection_mode,
        config.agent.compact_context,
        config.agent.max_system_prompt_chars,
        Some(&config.agent),
        coding_mode_label_owned.as_deref(),
    );
    if !native_tools {
        system_prompt.push_str(&build_tool_instructions(&tools_registry, Some(&i18n_descs)));
    }
    if !deferred_section.is_empty() {
        system_prompt.push('\n');
        system_prompt.push_str(&deferred_section);
    }

    let (thinking_directive, effective_message) =
        match crate::agent::thinking::parse_thinking_directive(message) {
            Some((level, remaining)) => {
                tracing::info!(thinking_level = ?level, "Thinking directive parsed from message");
                (Some(level), remaining)
            }
            None => (None, message.to_string()),
        };
    let thinking_level = crate::agent::thinking::resolve_thinking_level(
        thinking_directive,
        None,
        &config.agent.thinking,
    );
    let thinking_params = crate::agent::thinking::apply_thinking_level(thinking_level);
    let effective_temperature = crate::agent::thinking::clamp_temperature(
        config.default_temperature + thinking_params.temperature_adjustment,
    );

    if let Some(ref prefix) = thinking_params.system_prompt_prefix {
        system_prompt = format!("{prefix}\n\n{system_prompt}");
    }

    let effective_msg_ref = effective_message.as_str();
    let mem_context = build_context(
        mem.as_ref(),
        effective_msg_ref,
        config.memory.min_relevance_score,
        session_id,
    )
    .await;
    let rag_limit = if config.agent.compact_context { 2 } else { 5 };
    let hw_context = hardware_rag
        .as_ref()
        .map(|r| build_hardware_context(r, effective_msg_ref, &board_names, rag_limit))
        .unwrap_or_default();
    let context = format!("{mem_context}{hw_context}");
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z");
    let enriched = if context.is_empty() {
        format!("[{now}] {effective_message}")
    } else {
        format!("{context}[{now}] {effective_message}")
    };

    let mut history = vec![
        ChatMessage::system(&system_prompt),
        ChatMessage::user(&enriched),
    ];
    let mut excluded_tools = compute_excluded_mcp_tools(
        &tools_registry,
        &config.agent.tool_filter_groups,
        effective_msg_ref,
    );
    if config.autonomy.level != AutonomyLevel::Full {
        excluded_tools.extend(config.autonomy.non_cli_excluded_tools.iter().cloned());
    }

    agent_turn(
        provider.as_ref(),
        &mut history,
        &tools_registry,
        observer.as_ref(),
        provider_name,
        &model_name,
        effective_temperature,
        true,
        "daemon",
        None,
        &config.multimodal,
        config.agent.max_tool_iterations,
        Some(&approval_manager),
        &excluded_tools,
        &config.agent.tool_call_dedup_exempt,
        activated_handle_pm.as_ref(),
        None,
    )
    .await
}
