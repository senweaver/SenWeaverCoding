// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod control;
pub mod core;
pub mod detector;
pub mod policy;
pub mod services;
pub mod traits;
pub mod unified;


use crate::approval::{ApprovalManager, ApprovalRequest, ApprovalResponse};
use crate::config::Config;
use crate::cost::types::{BudgetCheck, TokenUsage as CostTokenUsage};
use crate::i18n::ToolDescriptions;
use crate::memory::{self, Memory, MemoryCategory, decay};
use crate::multimodal;
use crate::observability::{self, Observer, ObserverEvent, runtime_trace};
use crate::providers::traits::StreamEvent;
use crate::providers::{self, ChatMessage, ChatRequest, Provider, ToolCall};
use crate::runtime;
use crate::security::{AutonomyLevel, SecurityPolicy};
use crate::tools::{self, Tool, ToolRegistry};
use crate::util::truncate_with_ellipsis;
use anyhow::Result;
use futures_util::{FutureExt, StreamExt};
use std::collections::HashSet;
use std::fmt::Write;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

tokio::task_local! {
    pub static TOOL_DENY_PREFIXES: Vec<String>;
}

pub use crate::agent::reward::cost_tracking::{
    TOOL_LOOP_COST_TRACKING_CONTEXT, ToolLoopCostTrackingContext, scope_tool_loop_cost_tracking,
};
pub(crate) use crate::agent::reward::cost_tracking::{
    check_tool_loop_budget, lookup_model_pricing, record_tool_loop_cost_usage,
};

const STREAM_CHUNK_MIN_CHARS: usize = 80;

const DEDUP_RESULT_MARKER: &str = "[Deduplicated] Tool '";

const STREAM_TOOL_MARKER_WINDOW_CHARS: usize = 512;

const MAX_STREAM_RESPONSE_BYTES: usize = 24 * 1024 * 1024;

const DEFAULT_MAX_TOOL_ITERATIONS: usize = 2000;

const AUTOSAVE_MIN_MESSAGE_CHARS: usize = 20;

fn patch_history_runtime_model(history: &mut [ChatMessage], old_model: &str, new_model: &str) {
    if old_model == new_model || old_model.is_empty() || new_model.is_empty() {
        return;
    }
    let Some(sys) = history.iter_mut().find(|m| m.role == "system") else {
        return;
    };
    let old_marker = format!("| Model: {old_model}");
    let new_marker = format!("| Model: {new_model}");
    if sys.content.contains(&old_marker) {
        sys.content = sys.content.replacen(&old_marker, &new_marker, 1);
    }
}

pub fn resolve_model_override_target(
    requested: &str,
    config: &Config,
) -> Option<(Option<String>, String)> {
    let trimmed = requested.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.eq_ignore_ascii_case("fast") {
        if let Some(route) = config
            .model_routes
            .iter()
            .find(|r| r.hint.eq_ignore_ascii_case("fast"))
        {
            return Some((Some(route.provider.clone()), route.model.clone()));
        }
        if let Some(fast) = config
            .agent_runtime
            .fast_apply_model
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
        {
            return Some((None, fast.to_string()));
        }
        return None;
    }
    Some((None, trimmed.to_string()))
}

pub use crate::agent::model_switch::{
    ModelSwitchCallback, clear_model_switch_request, get_model_switch_state, scope_model_switch,
};
pub(crate) use crate::agent::model_switch::{ModelSwitchRequested, is_model_switch_requested};

use crate::agent::tool_handler::call_parser::{
    ParseGate, ParsedToolCall, detect_tool_call_parse_issue, parse_structured_tool_calls,
    parse_tool_calls_gated,
};
use crate::agent::tool_handler::filter::{compute_excluded_mcp_tools, is_plan_mode_allowed};

pub(crate) use crate::agent::profile::pii_sanitize::{
    apply_outgoing_pii_sanitization, scrub_credentials, scrub_tool_output,
};

pub use crate::agent::history::compaction::estimate_history_tokens;
pub(crate) use crate::agent::history::compaction::{
    load_interactive_session_history_async, save_interactive_session_history_async,
};

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
        tool_call_id: Option<String>,
    },

    ToolResult {
        name: String,
        output: String,
        success: bool,
        tool_call_id: Option<String>,
    },

    PlanProgressCommitted {
        plan_path: String,
        title: String,
        todos_json: String,
    },

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

    PermissionRequest {
        request_id: String,
        tool_name: String,
        input: serde_json::Value,
        description: Option<String>,
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

    PiiSanitized {
        report: crate::services::governance::pii_sanitizer::SanitizationReport,
    },

    ProviderRetry {
        attempt: u32,
        max_attempts: u32,
        wait_ms: u64,
        class: String,
        provider: String,
        model: String,
        message: String,
    },

    WorkerSpawned {
        parent_tool_use_id: String,
        worker_id: String,
        title: String,
        model: String,
    },

    WorkerStatus {
        worker_id: String,
        status: String,
        detail: Option<String>,
    },

    WorkerProgress {
        worker_id: String,
        action: String,
        detail: String,
    },

    WorkerCompleted {
        worker_id: String,
        success: bool,
        summary: String,
    },

    WorkerStopped {
        worker_id: String,
        reason: String,
    },

    ParentResumed {
        reason: String,
    },
}

tokio::task_local! {
    pub(crate) static TOOL_CHOICE_OVERRIDE: Option<String>;
}

tokio::task_local! {

    pub(crate) static PARENT_DRAFT_CHANNEL:
        Option<tokio::sync::mpsc::Sender<DraftEvent>>;
}

tokio::task_local! {
    pub(crate) static CURRENT_TOOL_CALL_ID: Option<String>;
}

tokio::task_local! {
    pub(crate) static CURRENT_TOOL_RUNTIME_APPROVED: bool;
}

pub fn take_parent_draft_channel() -> Option<tokio::sync::mpsc::Sender<DraftEvent>> {
    PARENT_DRAFT_CHANNEL.try_with(|c| c.clone()).ok().flatten()
}

pub fn current_tool_call_id() -> Option<String> {
    CURRENT_TOOL_CALL_ID.try_with(|c| c.clone()).ok().flatten()
}

pub fn current_tool_runtime_approved() -> bool {
    CURRENT_TOOL_RUNTIME_APPROVED.try_with(|v| *v).unwrap_or(false)
}

pub(crate) fn resolve_compaction_context_window(model: &str) -> usize {
    let model_window = crate::constants::api_limits::context_window_for_model(model) as usize;
    let budget_window = crate::agent::token::optimizer::global_optimizer()
        .map(|opt| opt.budget().context_window());
    match budget_window {
        Some(w) if w != 128_000 && w < model_window => w.max(32_000),
        _ => model_window.max(32_000),
    }
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

pub(crate) fn autosave_content_key(prefix: &str, content: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{prefix}_{:016x}", hasher.finish())
}

pub(crate) fn build_cli_turn_companion(
    raw_input: &str,
    expanded_input: &str,
    context: &str,
) -> String {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z");
    let mut companion = format!("[MESSAGE DATE & TIME: {now}]");
    let trimmed_context = context.trim();
    if !trimmed_context.is_empty() {
        companion.push_str("\n\n");
        companion.push_str(trimmed_context);
    }
    if expanded_input != raw_input {
        let appended_only = expanded_input
            .strip_prefix(raw_input)
            .map(str::trim)
            .filter(|extra| !extra.is_empty());
        if let Some(extra) = appended_only {
            companion.push_str(
                "\n\n[ATTACHED CONTEXT - resolved from the references in the user message \
                 above]\n",
            );
            companion.push_str(extra);
        } else if let Some(idx) = expanded_input.find("<context ") {
            let attachments = expanded_input[idx..].trim();
            if !attachments.is_empty() {
                companion.push_str(
                    "\n\n[ATTACHED CONTEXT - resolved from the @references in the user \
                     message above]\n",
                );
                companion.push_str(attachments);
            }
        } else {
            companion.push_str(
                "\n\n[EXPANDED REQUEST - the user message above with its references \
                 resolved]\n",
            );
            companion.push_str(expanded_input);
        }
    }
    companion
}

fn memory_session_id_from_state_file(path: &Path) -> Option<String> {
    let raw = path.to_string_lossy().trim().to_string();
    if raw.is_empty() {
        return None;
    }

    Some(format!("cli:{raw}"))
}

async fn build_context(
    mem: &dyn Memory,
    user_msg: &str,
    min_relevance_score: f64,
    session_id: Option<&str>,
) -> String {
    let mut context = String::new();

    let recall_query = {
        let kw = extract_code_search_query(user_msg);
        if kw.trim().is_empty() {
            user_msg.to_string()
        } else {
            kw
        }
    };

    let recall_result = match tokio::time::timeout(
        std::time::Duration::from_secs(20),
        mem.recall(&recall_query, 5, session_id, None, None),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            tracing::warn!(
                "memory recall timed out after 20s; continuing without memory context"
            );
            Err(anyhow::anyhow!("memory recall timed out"))
        }
    };
    if let Ok(mut entries) = recall_result {

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
                const MAX_MEMORY_ENTRY_CHARS: usize = 700;
                let content = truncate_with_ellipsis(&entry.content, MAX_MEMORY_ENTRY_CHARS);
                let _ = writeln!(context, "- {}: {}", entry.key, content);
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
    use crate::agent::loop_::services as loop_services;

    let cwd = crate::session::current_session_context()
        .map(|c| std::path::PathBuf::from(c.workspace_dir))
        .or_else(|| std::env::current_dir().ok())?;
    let registry_focus = crate::context::builder::FocusPathRegistry::current();
    let query = user_msg.trim();
    if registry_focus.is_empty() && query.is_empty() {
        return Some(String::new());
    }

    let mut builder = crate::context::builder::ContextBuilder::new(cwd.clone());
    if !registry_focus.is_empty() {
        builder = builder.with_focus_files(registry_focus.clone());
    }
    if let Some(lsp) = loop_services::lsp_context_source() {
        builder = builder.with_lsp(lsp);
    }
    let graph_cwd = cwd.clone();
    let graph_state =
        tokio::task::spawn_blocking(move || loop_services::symbol_graph_source_state(&graph_cwd))
            .await
            .ok();
    match graph_state {
        Some(loop_services::SymbolGraphSourceState::Ready(graph)) => {
            builder = builder.with_symbol_graph(graph);
            crate::code_intel::symbol_graph::incremental::ensure_workspace_watcher(&cwd);
        }
        Some(loop_services::SymbolGraphSourceState::Building) => {
            builder = builder.with_symbol_graph_building(true);
        }
        _ => {}
    }
    if !query.is_empty() {
        let rag_query = extract_code_search_query(query);
        if !rag_query.is_empty() {
            let rag_cwd = cwd.clone();
            if let Some(rag) =
                tokio::task::spawn_blocking(move || loop_services::rag_source(&rag_cwd))
                    .await
                    .ok()
                    .flatten()
            {
                builder = builder.with_rag(rag, rag_query);
            }
        }
    }
    const INJECTION_BUILD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);
    match tokio::time::timeout(INJECTION_BUILD_TIMEOUT, builder.build()).await {
        Ok(Ok(qc)) => {
            let qc: crate::context::builder::QueryContext = qc;
            Some(qc.render_injection_block())
        }
        Ok(Err(_)) => None,
        Err(_) => {
            tracing::warn!(
                target: "agent.context",
                "query-context assembly timed out after 8s; continuing without injection block"
            );
            None
        }
    }
}

fn extract_code_search_query(user_msg: &str) -> String {
    const STOP: &[&str] = &[
        "the", "and", "for", "with", "this", "that", "from", "into", "please", "fix",
    ];
    const STOP_CJK: &[&str] = &[
        "帮我", "一下", "如何", "怎么", "什么", "修改", "实现", "添加", "删除", "文件",
        "代码", "这个", "那个", "可以", "需要", "问题", "为什么", "然后", "现在", "一个",
        "不要", "使用", "请问", "麻烦", "所有", "进行", "或者", "以及", "但是", "如果",
        "优化", "功能", "检查", "支持", "错误", "报错", "运行", "执行", "调用",
    ];
    fn is_cjk(c: char) -> bool {
        matches!(c,
            '\u{4E00}'..='\u{9FFF}'
                | '\u{3400}'..='\u{4DBF}'
                | '\u{F900}'..='\u{FAFF}'
                | '\u{3040}'..='\u{30FF}'
                | '\u{AC00}'..='\u{D7AF}'
        )
    }
    let mut terms: Vec<String> = Vec::new();
    let push_term = |t: &str, terms: &mut Vec<String>| {
        if !terms.iter().any(|e| e.eq_ignore_ascii_case(t)) {
            terms.push(t.to_string());
        }
    };
    for raw in user_msg.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-')) {
        if terms.len() >= 12 {
            break;
        }
        let t = raw.trim();
        if t.len() < 2 {
            continue;
        }
        if t.chars().any(is_cjk) {
            let cjk_run: String = t.chars().filter(|c| is_cjk(*c)).collect();
            let ascii_run: String = t.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
            if ascii_run.len() >= 3 && !STOP.contains(&ascii_run.to_ascii_lowercase().as_str()) {
                push_term(&ascii_run, &mut terms);
            }
            let chars: Vec<char> = cjk_run.chars().collect();
            let mut i = 0usize;
            while i < chars.len() && terms.len() < 12 {
                let take = (chars.len() - i).min(2);
                if take < 2 {
                    break;
                }
                let gram: String = chars[i..i + 2].iter().collect();
                if !STOP_CJK.contains(&gram.as_str()) {
                    push_term(&gram, &mut terms);
                }
                i += 2;
            }
            continue;
        }
        let lower = t.to_ascii_lowercase();
        if STOP.contains(&lower.as_str()) {
            continue;
        }
        if t.chars().any(|c| c.is_ascii_uppercase())
            || t.contains('_')
            || t.contains('-')
            || t.len() >= 4
        {
            push_term(t, &mut terms);
        }
    }
    terms.join(" ")
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

fn canonical_tool_alias(name: &str) -> Option<&'static str> {
    let mapped = match name.to_ascii_lowercase().as_str() {
        "grep" | "ripgrep" | "rg" | "code_search" | "codesearch" | "search_files"
        | "searchfiles" | "search_code" => "content_search",
        "read" | "readfile" | "read_file" | "cat" | "view" | "viewfile" => "file_read",
        "write" | "writefile" | "create_file" | "createfile" => "file_write",
        "edit" | "str_replace" | "str_replace_editor" | "apply_patch" | "applypatch"
        | "edit_file" | "editfile" => "file_edit",
        "bash" | "sh" | "exec" | "command" | "cmd" | "terminal" | "run_command"
        | "runcommand" | "shell_command" => "shell",
        "web_search" | "websearch" | "web-search" | "search_web" | "websearch_tool" => {
            "web_search_tool"
        }
        "ls" | "list_files" | "listfiles" | "list_dir" | "listdir" | "dir" | "file_list"
        | "filelist" => "dir_list",
        "askquestion" => "ask_question",
        "askuser" => "ask_user",
        "memory_search" | "memorysearch" | "memrecall" | "memory_query" => "memory_recall",
        "lsp_symbols" | "lspsymbols" | "symbols" | "lsp_hover" | "lsphover"
        | "lsp_definition" => "lsp",
        _ => return None,
    };
    Some(mapped)
}

fn find_tool<'a>(
    tools: &'a [Box<dyn Tool>],
    name: &str,
    tool_registry: Option<&ToolRegistry>,
) -> Option<crate::tools::handle::ToolHandle<'a>> {
    if let Some(handle) = lookup_tool_exact(tools, name, tool_registry) {
        return Some(handle);
    }
    if let Some(canonical) = canonical_tool_alias(name) {
        if canonical != name {
            if let Some(handle) = lookup_tool_exact(tools, canonical, tool_registry) {
                tracing::debug!(
                    target: "agent.tool",
                    requested = %name,
                    resolved = %canonical,
                    "resolved tool name via alias normalization"
                );
                return Some(handle);
            }
        }
    }
    None
}

fn lookup_tool_exact<'a>(
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

pub(crate) fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

pub(crate) async fn execute_tool_panic_safe(
    tool: &dyn Tool,
    tool_name: &str,
    args: serde_json::Value,
) -> anyhow::Result<tools::ToolResult> {
    let fut = std::panic::AssertUnwindSafe(tool.execute(args)).catch_unwind();
    match fut.await {
        Ok(inner) => inner,
        Err(panic) => {
            let detail = panic_payload_message(panic.as_ref());
            tracing::error!(
                target: "agent.tool",
                tool = %tool_name,
                panic = %detail,
                "tool execution panicked; recovered as a tool error so the caller keeps running"
            );
            Err(anyhow::anyhow!(
                "Tool '{tool_name}' crashed internally ({detail}). The underlying file/state was \
                 left unchanged."
            ))
        }
    }
}

fn append_turn_records_to_history(
    history: &mut Vec<ChatMessage>,
    assistant_history_content: &str,
    native_tool_calls: &[ToolCall],
    individual_results: &[(Option<String>, String)],
    use_native_tools: bool,
    tool_results: &str,
) {
    history.push(ChatMessage::assistant(assistant_history_content));
    if native_tool_calls.is_empty() {
        let all_results_have_ids = use_native_tools
            && !individual_results.is_empty()
            && individual_results
                .iter()
                .all(|(tool_call_id, _)| tool_call_id.is_some());
        if all_results_have_ids {
            for (tool_call_id, result) in individual_results {
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
        for (native_call, (_, result)) in native_tool_calls.iter().zip(individual_results.iter()) {
            let tool_msg = serde_json::json!({
                "tool_call_id": native_call.id,
                "content": result,
            });
            history.push(ChatMessage::tool(tool_msg.to_string()));
        }
    }
}

async fn auto_finalize_incomplete_plan_steps(
    tools_registry: &[Box<dyn Tool>],
    tool_registry: Option<&ToolRegistry>,
    on_delta: Option<&tokio::sync::mpsc::Sender<DraftEvent>>,
    history: &mut Vec<ChatMessage>,
    intent_window: &str,
) -> usize {
    let Some(handle) = find_tool(tools_registry, "update_plan", tool_registry) else {
        return 0;
    };
    let tool = handle.as_tool();

    let get_args = serde_json::json!({ "action": "get" });
    let snapshot = match execute_tool_panic_safe(tool, "update_plan", get_args).await {
        Ok(r) if r.success => r,
        _ => return 0,
    };

    let mut pending_steps: Vec<(String, String)> = Vec::new();
    let mut in_progress_steps: Vec<(String, String)> = Vec::new();
    for raw_line in snapshot.output.lines() {
        let trimmed = raw_line.trim_start();
        let (rest, is_in_progress) = if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
            (rest, false)
        } else if let Some(rest) = trimmed.strip_prefix("- [~] ") {
            (rest, true)
        } else {
            continue;
        };
        let title = rest
            .split(" -- ")
            .next()
            .map(str::trim)
            .unwrap_or("")
            .to_string();
        if title.is_empty() {
            continue;
        }
        let notes_part = rest.split(" -- ").nth(1).unwrap_or("").trim().to_string();
        if is_in_progress {
            in_progress_steps.push((title, notes_part));
        } else {
            pending_steps.push((title, notes_part));
        }
    }

    let total_remaining = pending_steps.len() + in_progress_steps.len();
    if total_remaining == 0 {
        return 0;
    }

    let intent = crate::agent::plan_mode::execution_enforcement::classify_auto_finalize_intent(
        intent_window,
    );
    let assume_completed = matches!(
        intent,
        crate::agent::plan_mode::execution_enforcement::AutoFinalizeIntent::AssumeCompleted
    );

    tracing::warn!(
        target: "agent.plan_execution",
        pending = pending_steps.len(),
        in_progress = in_progress_steps.len(),
        intent = if assume_completed { "assume_completed" } else { "assume_skipped" },
        "Auto-finalizing incomplete plan steps after nudges exhausted"
    );

    let mut applied = 0usize;
    let in_progress_count = in_progress_steps.len();
    let mut all_steps: Vec<(String, bool)> = Vec::with_capacity(total_remaining);
    for (title, _) in in_progress_steps {
        all_steps.push((title, true));
    }
    for (title, _) in pending_steps {
        all_steps.push((title, false));
    }

    for (title, was_in_progress) in all_steps {
        let (status, note) = decide_auto_finalize_status(
            assume_completed,
            was_in_progress,
            intent_window,
        );
        let args = serde_json::json!({
            "action": "update",
            "step_id": title.clone(),
            "title": title.clone(),
            "status": status,
            "notes": note,
        });
        let tool_call_id = format!("auto_finalize_{}", Uuid::new_v4());

        if let Some(tx) = on_delta {
            let _ = tx
                .send(DraftEvent::ToolCall {
                    name: "update_plan".to_string(),
                    args: args.clone(),
                    tool_call_id: Some(tool_call_id.clone()),
                })
                .await;
        }

        let (output, success) =
            match execute_tool_panic_safe(tool, "update_plan", args.clone()).await {
                Ok(r) => (r.output, r.success),
                Err(e) => (format!("Auto-finalize update failed: {e}"), false),
            };

        if let Some(tx) = on_delta {
            let _ = tx
                .send(DraftEvent::ToolResult {
                    name: "update_plan".to_string(),
                    output: output.clone(),
                    success,
                    tool_call_id: Some(tool_call_id.clone()),
                })
                .await;
        }

        let assistant_payload = serde_json::json!({
            "tool_calls": [{
                "id": tool_call_id,
                "type": "function",
                "function": {
                    "name": "update_plan",
                    "arguments": args.to_string(),
                }
            }]
        });
        history.push(ChatMessage::assistant(assistant_payload.to_string()));
        let tool_msg = serde_json::json!({
            "tool_call_id": tool_call_id,
            "content": output,
        });
        history.push(ChatMessage::tool(tool_msg.to_string()));
        if success {
            applied = applied.saturating_add(1);
        }
    }

    if let Some(tx) = on_delta {
        let notice = if assume_completed {
            format!(
                "\n\u{2139}\u{fe0f} Plan auto-finalized: {} step(s) inferred as \
                 `completed` (and {} previously `in_progress`) based on your final \
                 summary. If any of these were actually unfinished, edit the .plan.md \
                 directly or open a new turn to fix the tracker.\n",
                applied.saturating_sub(in_progress_count),
                in_progress_count
            )
        } else {
            format!(
                "\n\u{26a0}\u{fe0f} Plan auto-finalized: {} step(s) marked as `skipped` \
                 because the agent exited without an explicit completion claim. \
                 If the work was actually done, re-run the plan or edit the \
                 .plan.md to fix the tracker.\n",
                applied
            )
        };
        let _ = tx.send(DraftEvent::Progress(notice)).await;
    }

    applied
}

async fn emit_plan_progress_completion_card(
    tools_registry: &[Box<dyn Tool>],
    tool_registry: Option<&ToolRegistry>,
    on_delta: Option<&tokio::sync::mpsc::Sender<DraftEvent>>,
    state: &crate::agent::plan_mode::execution_enforcement::PlanExecutionNudgeState,
) {
    if !state.active {
        return;
    }
    let Some(tx) = on_delta else {
        return;
    };
    let Some(handle) = find_tool(tools_registry, "update_plan", tool_registry) else {
        return;
    };
    let tool = handle.as_tool();
    let get_args = serde_json::json!({ "action": "get" });
    let snapshot = match execute_tool_panic_safe(tool, "update_plan", get_args).await {
        Ok(r) if r.success => r,
        _ => return,
    };

    let mut todos: Vec<serde_json::Value> = Vec::new();
    let mut has_open_step = false;
    for raw_line in snapshot.output.lines() {
        let trimmed = raw_line.trim_start();
        let (rest, status) = if let Some(rest) = trimmed.strip_prefix("- [x] ") {
            (rest, "completed")
        } else if let Some(rest) = trimmed.strip_prefix("- [-] ") {
            (rest, "cancelled")
        } else if let Some(rest) = trimmed.strip_prefix("- [~] ") {
            has_open_step = true;
            (rest, "in_progress")
        } else if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
            has_open_step = true;
            (rest, "pending")
        } else {
            continue;
        };
        let content = rest.split(" -- ").next().map(str::trim).unwrap_or("").to_string();
        if content.is_empty() {
            continue;
        }
        let notes = rest
            .split(" -- ")
            .nth(1)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let id = format!("s{}", todos.len() + 1);
        let mut obj = serde_json::Map::new();
        obj.insert("id".to_string(), serde_json::Value::String(id));
        obj.insert("content".to_string(), serde_json::Value::String(content));
        obj.insert("status".to_string(), serde_json::Value::String(status.to_string()));
        if let Some(n) = notes {
            obj.insert("notes".to_string(), serde_json::Value::String(n));
        }
        todos.push(serde_json::Value::Object(obj));
    }

    if todos.is_empty() {
        return;
    }

    if has_open_step {
        tracing::info!(
            target: "agent.plan_execution",
            total = todos.len(),
            "Plan execution ended with open steps; skipping completion card \
             (the plan card keeps its Resume/Continue affordance instead)"
        );
        return;
    }

    let plan_path = state.plan_path.clone().unwrap_or_default();
    let norm_path = plan_path.replace('\\', "/");
    let is_curator_handoff =
        norm_path.contains("/curators/") || norm_path.ends_with("impl_blueprint.md");
    let title = if is_curator_handoff {
        norm_path
            .trim_end_matches("/impl_blueprint.md")
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty() && *s != "impl_blueprint.md")
            .map(ToString::to_string)
            .unwrap_or_else(|| "Curator".to_string())
    } else {
        plan_path
            .rsplit(['/', '\\'])
            .next()
            .map(|f| f.trim_end_matches(".plan.md").to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Plan".to_string())
    };
    let todos_json = serde_json::Value::Array(todos).to_string();

    let _ = tx
        .send(DraftEvent::PlanProgressCommitted {
            plan_path,
            title,
            todos_json,
        })
        .await;
}

fn current_turn_preserved_indices(
    h: &[crate::providers::traits::ChatMessage],
) -> Vec<usize> {
    let current = h
        .iter()
        .rposition(|m| m.role == "user" && m.has_current_request_marker())
        .or_else(|| {
            h.iter().rposition(|m| {
                m.role == "user" && !m.content.trim_start().starts_with("[Tool results]")
            })
        });
    let mut idxs: Vec<usize> = Vec::new();
    if let Some(cur) = current {
        idxs.push(cur);
        if let Some(note_pos) = h[..cur].iter().rposition(|m| {
            m.role == "assistant"
                && crate::agent::dangling_tool_repair::is_turn_close_note(&m.content)
        }) && let Some(prior_user) = h[..note_pos].iter().rposition(|m| m.role == "user")
        {
            const MAX_PINNED_TURN_MSGS: usize = 40;
            idxs.push(prior_user);
            let tail_start = note_pos.saturating_sub(MAX_PINNED_TURN_MSGS).max(prior_user);
            for idx in tail_start..note_pos {
                idxs.push(idx);
            }
        }
    }
    idxs.sort_unstable();
    idxs.dedup();
    idxs
}

fn build_intent_text_window(
    recent_assistant_text: &str,
    history: &[ChatMessage],
) -> String {
    const MAX_ASSISTANT_TAIL_MESSAGES: usize = 3;
    let mut buf = String::new();
    let mut collected = 0;
    for msg in history.iter().rev() {
        if collected >= MAX_ASSISTANT_TAIL_MESSAGES {
            break;
        }
        if msg.role != "assistant" {
            continue;
        }
        if msg.content.trim().is_empty() {
            continue;
        }
        if msg.content.contains("\"tool_calls\"") {
            continue;
        }
        if !buf.is_empty() {
            buf.push_str("\n---\n");
        }
        buf.push_str(&msg.content);
        collected += 1;
    }
    if !recent_assistant_text.trim().is_empty() {
        if !buf.is_empty() {
            buf.push_str("\n---\n");
        }
        buf.push_str(recent_assistant_text);
    }
    buf
}

fn decide_auto_finalize_status(
    assume_completed: bool,
    was_in_progress: bool,
    aggregate_text: &str,
) -> (&'static str, String) {
    if assume_completed {
        let quote = first_completion_quote(aggregate_text).unwrap_or_default();
        let note = if quote.is_empty() {
            "Auto-completed: agent's final summary declared the work done but did \
             not flip this step's tracker status. The plan tracker was updated to \
             reflect that completion claim; open the plan and verify."
                .to_string()
        } else {
            format!(
                "Auto-completed: agent's final summary declared completion \
                 ({:?}) but did not flip this step. Updated to match the claim; \
                 verify against the .plan.md if anything looks off.",
                quote
            )
        };
        ("completed", note)
    } else if was_in_progress {
        (
            "skipped",
            "Auto-skipped: this step was still in_progress when the turn ended and \
             the agent did not produce a completion claim. Marked skipped so the \
             tracker no longer blocks the UI; re-run the plan if the step still \
             needs real work."
                .to_string(),
        )
    } else {
        (
            "skipped",
            "Auto-skipped: this step was never started and the agent exited \
             without claiming completion. Marked skipped to release the tracker; \
             open the plan and resume if the step still applies."
                .to_string(),
        )
    }
}

fn first_completion_quote(text: &str) -> Option<String> {
    const LIMIT: usize = 80;
    let lower = text.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "all done",
        "all fixes complete",
        "all the fixes",
        "successfully completed",
        "verification passed",
        "全部完成",
        "全部修复",
        "已全部完成",
        "已完成",
        "修复完成",
        "验证通过",
        "所有修复",
    ];
    for needle in NEEDLES {
        let lower_needle = needle.to_ascii_lowercase();
        if let Some(idx) = lower.find(&lower_needle) {
            let start = idx.saturating_sub(20);
            let mut window_end = idx + lower_needle.len() + 30;
            if window_end > text.len() {
                window_end = text.len();
            }
            while !text.is_char_boundary(window_end) && window_end < text.len() {
                window_end += 1;
            }
            let mut window_start = start;
            while !text.is_char_boundary(window_start) && window_start < text.len() {
                window_start += 1;
            }
            let snippet: String = text[window_start..window_end]
                .chars()
                .take(LIMIT)
                .collect();
            return Some(snippet.trim().to_string());
        }
    }
    None
}

pub fn tool_call_signature(name: &str, arguments: &serde_json::Value) -> (String, String) {
    let args_json = crate::agent::loop_::detector::canonicalise_args_string(arguments);
    (name.trim().to_ascii_lowercase(), args_json)
}

fn build_native_assistant_history(
    text: &str,
    tool_calls: &[ToolCall],
    reasoning_content: Option<&str>,
    thinking_signature: Option<&str>,
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
        if let Some(map) = obj.as_object_mut() {
            map.insert(
                "reasoning_content".to_string(),
                serde_json::Value::String(rc.to_string()),
            );
        }
    }
    if let Some(sig) = thinking_signature.filter(|s| !s.is_empty()) {
        if let Some(map) = obj.as_object_mut() {
            map.insert(
                "thinking_signature".to_string(),
                serde_json::Value::String(sig.to_string()),
            );
        }
    }

    obj.to_string()
}

fn build_native_assistant_history_from_parsed_calls(
    text: &str,
    tool_calls: &[ParsedToolCall],
    reasoning_content: Option<&str>,
    thinking_signature: Option<&str>,
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
        if let Some(map) = obj.as_object_mut() {
            map.insert(
                "reasoning_content".to_string(),
                serde_json::Value::String(rc.to_string()),
            );
        }
    }
    if let Some(sig) = thinking_signature.filter(|s| !s.is_empty()) {
        if let Some(map) = obj.as_object_mut() {
            map.insert(
                "thinking_signature".to_string(),
                serde_json::Value::String(sig.to_string()),
            );
        }
    }

    Some(obj.to_string())
}

pub(crate) fn tool_loop_cancelled() -> anyhow::Error {
    anyhow::Error::new(crate::error::AgentError::TurnCancelled)
}

#[derive(Debug, Default)]
struct StreamedChatOutcome {
    response_text: String,
    tool_calls: Vec<ToolCall>,

    reasoning_content: String,

    thinking_signature: String,

    thinking_signature_blocks: u32,

    usage: Option<crate::providers::traits::TokenUsage>,
    forwarded_live_deltas: bool,

    stop_reason: Option<crate::providers::traits::StopReason>,

    pre_executed: Vec<PreExecutedToolRecord>,
}

#[derive(Clone, Debug)]
struct PreExecutedToolRecord {
    name: String,
    args: String,
    output: Option<String>,
}

fn retry_friendly_message(notice: &crate::providers::traits::RetryNotice) -> String {
    use crate::providers::traits::RetryClass;
    let provider = if notice.provider.is_empty() {
        "upstream".to_string()
    } else {
        notice.provider.clone()
    };
    match notice.failure_class {
        RetryClass::EngineOverloaded => format!(
            "{provider} is temporarily overloaded (engine overloaded); retrying automatically…"
        ),
        RetryClass::AccountRateLimited => format!(
            "{provider} account quota is rate limited; waiting for recovery before retrying…"
        ),
        RetryClass::Transient => format!(
            "{provider} hit a transient network error; retrying automatically…"
        ),
    }
}

const LLM_RESILIENCE_MAX_RETRIES: u32 = 600;
const LLM_RESILIENCE_BACKOFF_BASE_MS: u64 = 1_000;
const LLM_RESILIENCE_BACKOFF_CAP_MS: u64 = 15_000;
const LLM_RESILIENCE_MAX_TOTAL: std::time::Duration = std::time::Duration::from_secs(600);

fn llm_resilience_backoff_ms(attempt: u32) -> u64 {
    let shift = attempt.saturating_sub(1).min(20);
    let raw = LLM_RESILIENCE_BACKOFF_BASE_MS.saturating_mul(1u64 << shift);
    let capped = raw.min(LLM_RESILIENCE_BACKOFF_CAP_MS);
    let jitter_seed = attempt.wrapping_mul(2_654_435_761);
    let jitter_ratio = 1.0 + ((jitter_seed as f64 / u32::MAX as f64) - 0.5) * 0.4;
    ((capped as f64 * jitter_ratio).max(0.0) as u64).min(LLM_RESILIENCE_BACKOFF_CAP_MS)
}

fn llm_error_is_terminal(err: &anyhow::Error) -> bool {
    if crate::providers::reliable::is_non_retryable(err) {
        return true;
    }
    if crate::providers::reliable::is_context_window_exceeded(err) {
        return true;
    }
    let lower = err.to_string().to_lowercase();
    const TERMINAL_HINTS: &[&str] = &[
        "non-retryable",
        "non_retryable",
        "verify provider credentials",
        "request exceeds model context window",
        "exceeds the context window",
        "no provider supports streaming",
        "no_model_configured",
        "no model configured",
    ];
    TERMINAL_HINTS.iter().any(|h| lower.contains(h))
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
            () = token.cancelled() => Err(tool_loop_cancelled()),
            result = chat_future => result,
        }
    } else {
        chat_future.await
    }
}

#[derive(Default)]
struct StreamProgressProbe {
    made_progress: bool,
    partial_usage: Option<crate::providers::TokenUsage>,
}

#[allow(clippy::too_many_arguments)]
async fn consume_provider_streaming_response(
    provider: &dyn Provider,
    messages: &[ChatMessage],
    request_tools: Option<&[crate::tools::ToolSpec]>,
    model: &str,
    temperature: f64,
    cancellation_token: Option<&CancellationToken>,
    on_delta: Option<&tokio::sync::mpsc::Sender<DraftEvent>>,
    idle_timeout: Option<Duration>,
    progress: &mut StreamProgressProbe,
) -> Result<StreamedChatOutcome> {
    let cancel_for_provider = cancellation_token.cloned();
    let mut provider_stream =
        crate::providers::reliable::scope_stream_cancel_token_sync(cancel_for_provider, || {
            provider.stream_chat(
                ChatRequest {
                    messages,
                    tools: request_tools,
                },
                model,
                temperature,
                crate::providers::traits::StreamOptions::new(true),
            )
        });
    let mut outcome = StreamedChatOutcome::default();
    let mut delta_sender = on_delta;
    let mut suppress_forwarding = false;
    let mut tool_suppress_kind: Option<crate::agent::streaming_markers::ToolMarkerKind> = None;
    let mut marker_window = String::new();
    let mut think_splitter = crate::agent::think_extractor::ThinkTagSplitter::new();

    let stream_idle_err = |idle: Duration| {
        anyhow::Error::new(crate::error::AgentError::StreamInterrupted(format!(
            "stream idle timeout: no data received from provider for {}s",
            idle.as_secs()
        )))
    };

    loop {
        let next_chunk = match (idle_timeout, cancellation_token) {
            (Some(idle), Some(token)) => {
                tokio::select! {
                    () = token.cancelled() => return Err(tool_loop_cancelled()),
                    res = tokio::time::timeout(idle, provider_stream.next()) => match res {
                        Ok(chunk) => chunk,
                        Err(_) => return Err(stream_idle_err(idle)),
                    },
                }
            }
            (Some(idle), None) => match tokio::time::timeout(idle, provider_stream.next()).await {
                Ok(chunk) => chunk,
                Err(_) => return Err(stream_idle_err(idle)),
            },
            (None, Some(token)) => {
                tokio::select! {
                    () = token.cancelled() => return Err(tool_loop_cancelled()),
                    chunk = provider_stream.next() => chunk,
                }
            }
            (None, None) => provider_stream.next().await,
        };

        let Some(event_result) = next_chunk else {
            break;
        };

        let event = event_result.map_err(|err| {
            anyhow::Error::new(crate::error::AgentError::StreamInterrupted(err.to_string()))
        })?;
        match event {
            StreamEvent::Final => break,
            StreamEvent::ToolCall(tool_call) => {
                progress.made_progress = true;
                outcome.tool_calls.push(tool_call);
                suppress_forwarding = true;
                if outcome.forwarded_live_deltas {
                    if let Some(tx) = delta_sender {
                        let _ = tx.send(DraftEvent::Clear).await;
                    }
                    outcome.forwarded_live_deltas = false;
                }
            }
            StreamEvent::PreExecutedToolCall { name, args } => {
                let parsed_args = serde_json::from_str::<serde_json::Value>(&args)
                    .unwrap_or_else(|_| serde_json::Value::String(args.clone()));
                if let Some(tx) = delta_sender {
                    if tx
                        .send(DraftEvent::ToolCall {
                            name: name.clone(),
                            args: parsed_args,
                            tool_call_id: None,
                        })
                        .await
                        .is_err()
                    {
                        delta_sender = None;
                    }
                }
                outcome.pre_executed.push(PreExecutedToolRecord {
                    name,
                    args,
                    output: None,
                });
            }
            StreamEvent::PreExecutedToolResult { name, output } => {
                if let Some(tx) = delta_sender {
                    if tx
                        .send(DraftEvent::ToolResult {
                            name: name.clone(),
                            output: output.clone(),
                            success: true,
                            tool_call_id: None,
                        })
                        .await
                        .is_err()
                    {
                        delta_sender = None;
                    }
                }
                if let Some(rec) = outcome
                    .pre_executed
                    .iter_mut()
                    .rev()
                    .find(|r| r.name == name && r.output.is_none())
                {
                    rec.output = Some(output);
                } else {
                    outcome.pre_executed.push(PreExecutedToolRecord {
                        name,
                        args: String::new(),
                        output: Some(output),
                    });
                }
            }
            StreamEvent::Usage(usage) => {
                progress.made_progress = true;
                progress.partial_usage = Some(usage.clone());
                outcome.usage = Some(usage);
            }
            StreamEvent::ReasoningSignature(sig) => {
                outcome.thinking_signature = sig;
                outcome.thinking_signature_blocks =
                    outcome.thinking_signature_blocks.saturating_add(1);
            }
            StreamEvent::StopReason(reason) => {
                outcome.stop_reason = Some(reason);
            }
            StreamEvent::Retry(notice) => {
                let class_str = notice.failure_class.as_str();
                let friendly = retry_friendly_message(&notice);
                let session_label = crate::session::current_session_context()
                    .map(|ctx| ctx.session_id)
                    .unwrap_or_default();
                tracing::warn!(
                    target: "providers.reliable.retry",
                    session_id = %session_label,
                    provider = %notice.provider,
                    model = %notice.model,
                    attempt = notice.attempt,
                    max_attempts = notice.max_attempts,
                    wait_ms = notice.wait_ms,
                    class = class_str,
                    last_error = %notice.last_error_summary,
                    "Upstream returned retryable failure; awaiting backoff before re-attempting"
                );
                if let Some(tx) = delta_sender {
                    if outcome.forwarded_live_deltas {
                        let _ = tx.send(DraftEvent::Clear).await;
                        outcome.forwarded_live_deltas = false;
                    }
                    if tx
                        .send(DraftEvent::ProviderRetry {
                            attempt: notice.attempt,
                            max_attempts: notice.max_attempts,
                            wait_ms: notice.wait_ms,
                            class: class_str.to_string(),
                            provider: notice.provider.clone(),
                            model: notice.model.clone(),
                            message: friendly,
                        })
                        .await
                        .is_err()
                    {
                        delta_sender = None;
                    }
                }
                outcome.response_text.clear();
                outcome.reasoning_content.clear();
                outcome.thinking_signature.clear();
                outcome.thinking_signature_blocks = 0;
                outcome.tool_calls.clear();
                outcome.pre_executed.clear();
                outcome.stop_reason = None;
                marker_window.clear();
                suppress_forwarding = false;
                tool_suppress_kind = None;
                think_splitter = crate::agent::think_extractor::ThinkTagSplitter::new();
                progress.made_progress = false;
                progress.partial_usage = None;
            }
            StreamEvent::TextDelta(chunk) => {

                if outcome.response_text.len() + outcome.reasoning_content.len()
                    > MAX_STREAM_RESPONSE_BYTES
                {
                    tracing::warn!(
                        bytes = outcome.response_text.len() + outcome.reasoning_content.len(),
                        max = MAX_STREAM_RESPONSE_BYTES,
                        "LLM stream exceeded max response size; aborting turn to avoid presenting truncated output as complete"
                    );
                    return Err(anyhow::Error::new(
                        crate::error::AgentError::StreamInterrupted(format!(
                            "LLM stream exceeded max response size ({MAX_STREAM_RESPONSE_BYTES} bytes); aborting to avoid silently truncated output"
                        )),
                    ));
                }

                if let Some(rc) = &chunk.reasoning {
                    if !rc.is_empty() {
                        progress.made_progress = true;
                        outcome.reasoning_content.push_str(rc);
                        if let Some(tx) = delta_sender {
                            if tx
                                .send(DraftEvent::Thinking(rc.clone()))
                                .await
                                .is_err()
                            {
                                delta_sender = None;
                            }
                        }
                    }
                }

                if chunk.delta.is_empty() {
                    continue;
                }
                progress.made_progress = true;

                let (visible_delta, thinking_delta) = think_splitter.split(&chunk.delta);

                if !thinking_delta.is_empty() {
                    outcome.reasoning_content.push_str(&thinking_delta);
                    if let Some(tx) = delta_sender {
                        if tx
                            .send(DraftEvent::Thinking(thinking_delta.clone()))
                            .await
                            .is_err()
                        {
                            delta_sender = None;
                        }
                    }
                }

                if visible_delta.is_empty() {
                    continue;
                }

                outcome.response_text.push_str(&visible_delta);
                marker_window.push_str(&visible_delta);

                if marker_window.len() > STREAM_TOOL_MARKER_WINDOW_CHARS {
                    let keep_from = marker_window.len() - STREAM_TOOL_MARKER_WINDOW_CHARS;
                    let boundary = marker_window
                        .char_indices()
                        .find(|(idx, _)| *idx >= keep_from)
                        .map_or(0, |(idx, _)| idx);
                    marker_window.drain(..boundary);
                }

                if !suppress_forwarding {
                    if let Some(kind) =
                        crate::agent::streaming_markers::classify_tool_marker(&marker_window)
                    {
                        suppress_forwarding = true;
                        tool_suppress_kind = Some(kind);
                        if outcome.forwarded_live_deltas {
                            if let Some(tx) = delta_sender {
                                let _ = tx.send(DraftEvent::Clear).await;
                            }
                            outcome.forwarded_live_deltas = false;
                        }
                    }
                } else if matches!(
                    tool_suppress_kind,
                    Some(crate::agent::streaming_markers::ToolMarkerKind::Xml)
                ) && crate::agent::streaming_markers::find_tool_close_marker(&marker_window)
                    .is_some()
                {
                    suppress_forwarding = false;
                    tool_suppress_kind = None;
                    marker_window.clear();
                }

                if suppress_forwarding {
                    continue;
                }

                if let Some(tx) = delta_sender {
                    if !outcome.forwarded_live_deltas {
                        let _ = tx.send(DraftEvent::Clear).await;
                        outcome.forwarded_live_deltas = true;
                    }
                    if tx.send(DraftEvent::Content(visible_delta)).await.is_err() {
                        delta_sender = None;
                    }
                }
            }
        }
    }

    let (residual_visible, residual_thinking) = think_splitter.flush();
    if !residual_thinking.is_empty() {
        outcome.reasoning_content.push_str(&residual_thinking);
        if let Some(tx) = delta_sender {
            let _ = tx
                .send(DraftEvent::Thinking(residual_thinking))
                .await;
        }
    }
    if !residual_visible.is_empty() {
        outcome.response_text.push_str(&residual_visible);
        if let Some(tx) = delta_sender {
            if !outcome.forwarded_live_deltas {
                let _ = tx.send(DraftEvent::Clear).await;
                outcome.forwarded_live_deltas = true;
            }
            let _ = tx.send(DraftEvent::Content(residual_visible)).await;
        }
    }

    Ok(outcome)
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
        Some(slot @ serde_json::Value::Null) => {
            *slot = default_delivery();
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

async fn request_session_tool_approval(
    mgr: &ApprovalManager,
    request: &ApprovalRequest,
    on_delta: Option<&tokio::sync::mpsc::Sender<DraftEvent>>,
    cancellation_token: Option<&CancellationToken>,
    description: &str,
) -> Option<crate::approval::SessionApprovalVerdict> {
    if !mgr.has_session_sink() {
        return None;
    }
    let tx = on_delta?;
    let mut rx = crate::gateway::ws::gateway_approval_bus().subscribe();
    let request_id = mgr.request_via_session(request)?;
    if let Some(h) = crate::hooks::global_hooks() {
        let message = format!("{}: {}", request.tool_name, description);
        crate::runtime::spawn_supervised("hooks.notification", async move {
            h.fire_notification("permission_request", &message).await;
        });
    }
    let _ = tx
        .send(DraftEvent::PermissionRequest {
            request_id: request_id.clone(),
            tool_name: request.tool_name.clone(),
            input: request.arguments.clone(),
            description: Some(description.to_string()),
        })
        .await;
    let verdict =
        crate::approval::wait_for_session_decision(&request_id, &mut rx, cancellation_token)
            .await;
    let _ = crate::approval::drop_pending_gateway_approval(&request_id);
    Some(verdict)
}

fn approval_timeout_denial(tool_name: &str) -> String {
    format!(
        "Approval request for tool '{}' timed out after {}s with no user response; the call was \
         denied. Ask the user to respond to the approval prompt, or have them adjust the \
         [autonomy] auto_approve configuration.",
        tool_name,
        crate::approval::SESSION_APPROVAL_TIMEOUT_MS / 1000
    )
}

async fn run_auto_verify_gate(
    verify_retries: u32,
    cancellation_token: Option<&CancellationToken>,
) -> Option<(String, bool)> {
    let svc = crate::services::try_get_services()?;
    let pacing = &svc.config().pacing;
    if !pacing.auto_verify_after_edit {
        return None;
    }
    let root = crate::session::current_session_context()
        .map(|c| std::path::PathBuf::from(c.workspace_dir))
        .filter(|p| p.is_dir())?;

    let pipeline = crate::agent::verification::pipeline::VerificationPipeline::default_for_workspace(
        &root,
        Some(std::sync::Arc::new(svc.lsp.clone())),
    );

    let timeout = Duration::from_secs(pacing.auto_verify_timeout_secs.max(1));
    let run_fut = pipeline.run_on_workspace(&root);
    let report = {
        let with_timeout = tokio::time::timeout(timeout, run_fut);
        let outcome = match cancellation_token {
            Some(token) => tokio::select! {
                biased;
                () = token.cancelled() => return None,
                r = with_timeout => r,
            },
            None => with_timeout.await,
        };
        match outcome {
            Ok(Ok(report)) => report,
            Ok(Err(e)) => {
                tracing::debug!(error = %e, "auto-verify pipeline error; skipping gate");
                return None;
            }
            Err(_) => {
                tracing::debug!("auto-verify pipeline timed out; skipping gate");
                return None;
            }
        }
    };

    if report.passed {
        return None;
    }

    let retry_budget_left = verify_retries < pacing.auto_verify_max_retries;
    let mut body = String::new();
    let summary = report.joined_summary();
    if !summary.is_empty() {
        body.push_str(&summary);
        body.push('\n');
    }
    for issue in report
        .all_issues()
        .into_iter()
        .filter(|i| matches!(i.severity, crate::agent::verification::IssueSeverity::Error))
        .take(20)
    {
        body.push_str(&format!(
            "  - {}:{} {}\n",
            issue.line,
            issue.column,
            truncate_with_ellipsis(&issue.message, 300)
        ));
    }

    let feedback = if retry_budget_left {
        format!(
            "[Auto-verify] The workspace does NOT build/type-check cleanly after your edits. \
             These are real errors from the project's own tools ({}). Fix them before you \
             finish — do not claim the task is done while these fail:\n{}",
            report.failed_stages.join(", "),
            body.trim_end()
        )
    } else {
        format!(
            "[Auto-verify] The workspace still fails verification ({}) after {} fix \
             attempt(s). Remaining problems (stop and surface these to the user if you cannot \
             resolve them):\n{}",
            report.failed_stages.join(", "),
            verify_retries,
            body.trim_end()
        )
    };
    Some((feedback, retry_budget_left))
}

async fn execute_one_tool(
    call_name: &str,
    mut call_arguments: serde_json::Value,
    tools_registry: &[Box<dyn Tool>],
    tool_registry: Option<&ToolRegistry>,
    activated_tools: Option<&std::sync::Arc<parking_lot::Mutex<crate::tools::ActivatedToolSet>>>,
    observer: &dyn Observer,
    cancellation_token: Option<&CancellationToken>,
    rbac_engine: Option<&std::sync::Arc<crate::security::rbac::RbacEngine>>,
    rbac_identity: Option<&crate::security::rbac::CallerIdentity>,
    approval: Option<&ApprovalManager>,
    guardrails_pre_cleared: bool,
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

    if crate::security::estop::is_kill_all() || crate::security::estop::is_tool_frozen(call_name) {
        let reason = if crate::security::estop::is_kill_all() {
            "Emergency stop engaged (kill_all): all tool execution is halted".to_string()
        } else {
            format!("Tool '{call_name}' is frozen by an active emergency stop")
        };
        let duration = start.elapsed();
        observer.record_event(&ObserverEvent::ToolCall {
            tool: call_name.to_string(),
            duration,
            success: false,
        });
        return Ok(ToolExecutionOutcome {
            output: reason.clone(),
            success: false,
            error_reason: Some(reason),
            duration,
        });
    }

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

    if let Some(feedback) = crate::agent::tool_handler::arg_validate::validate_args_against_schema(
        call_name,
        &tool.parameters_schema(),
        &call_arguments,
    ) {
        let duration = start.elapsed();
        observer.record_event(&ObserverEvent::ToolCall {
            tool: call_name.to_string(),
            duration,
            success: false,
        });
        if let Some(svc) = crate::services::try_get_services() {
            crate::observability::agent_metrics::inc_tool_call(
                &svc.agent_metrics,
                call_name,
                "schema_rejected",
            );
        }
        return Ok(ToolExecutionOutcome {
            output: feedback.clone(),
            success: false,
            error_reason: Some(feedback),
            duration,
        });
    }

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

    if !guardrails_pre_cleared {
        let coding_label =
            Some(crate::agent::coding_mode::active_coding_mode().label().to_string());
        let coding_label_lc = coding_label.as_deref().map(str::to_ascii_lowercase);
        let perm_mode_lc = crate::gateway::ws::desktop::active_permission_mode();
        let tool_lc = call_name.to_ascii_lowercase();
        let guardrail_ctx = crate::guardrails::GuardrailContext {
            coding_mode: coding_label_lc.as_deref(),
            permission_mode: Some(&perm_mode_lc),
            tool_name: Some(&tool_lc),
        };
        match crate::guardrails::evaluate_tool_guardrails(call_name, Some(&guardrail_ctx)) {
            crate::guardrails::GuardrailDecision::Allow => {}
            crate::guardrails::GuardrailDecision::Deny(reason) => {
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
            crate::guardrails::GuardrailDecision::RequireApproval(reason) => {
                let mode_auto_approved = crate::agent::mode::effects::mode_auto_approves(
                    crate::agent::coding_mode::active_coding_mode(),
                ) && approval.is_none_or(|m| m.mode_auto_approve_allows(call_name));
                let mut timeout_denial: Option<String> = None;
                let approved = if mode_auto_approved {
                    true
                } else if let Some(mgr) = approval {
                    let request = ApprovalRequest {
                        tool_name: call_name.to_string(),
                        arguments: call_arguments.clone(),
                    };
                    let parent_draft = take_parent_draft_channel();
                    match request_session_tool_approval(
                        mgr,
                        &request,
                        parent_draft.as_ref(),
                        cancellation_token,
                        &format!("Guardrail approval required: {reason}"),
                    )
                    .await
                    {
                        Some(crate::approval::SessionApprovalVerdict::Decision(decision)) => {
                            mgr.record_decision(
                                call_name,
                                &call_arguments,
                                decision,
                                "guardrail",
                            );
                            decision != ApprovalResponse::No
                        }
                        Some(crate::approval::SessionApprovalVerdict::Cancelled) => {
                            mgr.record_decision(
                                call_name,
                                &call_arguments,
                                ApprovalResponse::No,
                                "guardrail",
                            );
                            return Err(tool_loop_cancelled());
                        }
                        Some(crate::approval::SessionApprovalVerdict::TimedOut) => {
                            mgr.record_decision(
                                call_name,
                                &call_arguments,
                                ApprovalResponse::No,
                                "guardrail",
                            );
                            timeout_denial = Some(approval_timeout_denial(call_name));
                            false
                        }
                        None => {
                            if mgr.is_non_interactive() {
                                false
                            } else {
                                let decision = mgr.prompt_cli_async(&request).await;
                                mgr.record_decision(
                                    call_name,
                                    &call_arguments,
                                    decision,
                                    "guardrail",
                                );
                                decision != ApprovalResponse::No
                            }
                        }
                    }
                } else {
                    false
                };
                if !approved {
                    let duration = start.elapsed();
                    observer.record_event(&ObserverEvent::ToolCall {
                        tool: call_name.to_string(),
                        duration,
                        success: false,
                    });
                    let denial = timeout_denial.unwrap_or_else(|| {
                        format!(
                            "Blocked by guardrails: approval required but not granted ({reason})"
                        )
                    });
                    return Ok(ToolExecutionOutcome {
                        output: denial.clone(),
                        success: false,
                        error_reason: Some(denial),
                        duration,
                    });
                }
            }
        }
    }

    {
        let web_search_enabled = crate::services::try_get_services()
            .map(|svc| svc.config().web_search.enabled)
            .unwrap_or(true);
        match crate::agent::web_search_url_guard::evaluate_browser_or_web_fetch_call(
            call_name,
            &call_arguments,
            web_search_enabled,
        ) {
            crate::agent::web_search_url_guard::GuardDecision::Allow => {}
            crate::agent::web_search_url_guard::GuardDecision::AllowWithFallbackTrace => {
                tracing::info!(
                    tool = %call_name,
                    "Permitting search-engine URL as fallback; web_search recently failed"
                );
            }
            crate::agent::web_search_url_guard::GuardDecision::Refuse(refusal) => {
                tracing::warn!(
                    tool = %call_name,
                    "Blocked search-engine URL misuse; web_search has not been tried yet"
                );
                let duration = start.elapsed();
                observer.record_event(&ObserverEvent::ToolCall {
                    tool: call_name.to_string(),
                    duration,
                    success: false,
                });
                return Ok(ToolExecutionOutcome {
                    output: refusal.clone(),
                    success: false,
                    error_reason: Some(refusal),
                    duration,
                });
            }
        }
    }

    if let Some(obj) = call_arguments.as_object_mut() {
        if call_name == "shell" || obj.contains_key("approved") {
            obj.insert("approved".to_string(), serde_json::Value::Bool(true));
        }
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

    let browser_trace_args = if call_name == "browser" {
        Some(call_arguments.clone())
    } else {
        None
    };

    let tool_timeout = crate::services::try_get_services()
        .and_then(|svc| svc.config().pacing.tool_timeout_secs)
        .filter(|s| *s > 0)
        .map(Duration::from_secs);

    let tool_future =
        std::panic::AssertUnwindSafe(tool.execute(call_arguments)).catch_unwind();

    let run_with_optional_timeout = async {
        match tool_timeout {
            Some(limit) => match tokio::time::timeout(limit, tool_future).await {
                Ok(result) => Some(result),
                Err(_) => None,
            },
            None => Some(tool_future.await),
        }
    };

    let caught = if let Some(token) = cancellation_token {
        tokio::select! {
            () = token.cancelled() => return Err(tool_loop_cancelled()),
            result = run_with_optional_timeout => result,
        }
    } else {
        run_with_optional_timeout.await
    };

    let Some(caught) = caught else {
        let duration = start.elapsed();
        let limit_secs = tool_timeout.map(|d| d.as_secs()).unwrap_or(0);
        let reason = format!(
            "Tool '{call_name}' timed out after {limit_secs}s (pacing.tool_timeout_secs). \
             The operation was aborted. Retry with a smaller/faster scope, or split it into steps."
        );
        tracing::warn!(
            target: "agent.tool",
            tool = %call_name,
            timeout_secs = limit_secs,
            elapsed_ms = duration.as_millis() as u64,
            "tool execution timed out; the in-flight tool future has been dropped to best-effort cancel tool-side work (kill_on_drop children are reaped on drop). Tools that spawn children without kill_on_drop may leave an orphan until OS reclaim; recovering as a non-silent tool error so the turn keeps running"
        );
        observer.record_event(&ObserverEvent::ToolCall {
            tool: call_name.to_string(),
            duration,
            success: false,
        });
        if let Some(svc) = crate::services::try_get_services() {
            crate::observability::agent_metrics::inc_tool_call(
                &svc.agent_metrics,
                call_name,
                "timeout",
            );
        }
        return Ok(ToolExecutionOutcome {
            output: reason.clone(),
            success: false,
            error_reason: Some(reason),
            duration,
        });
    };
    let tool_result = match caught {
        Ok(inner) => inner,
        Err(panic) => {
            let detail = panic_payload_message(panic.as_ref());
            tracing::error!(
                target: "agent.tool",
                tool = %call_name,
                panic = %detail,
                "tool execution panicked; recovering as a tool error so the turn keeps running"
            );
            Err(anyhow::anyhow!(
                "Tool '{call_name}' crashed internally ({detail}). The underlying file/state was \
                 left unchanged. Re-read the relevant file and try a smaller or different edit."
            ))
        }
    };

    if let Some(svc) = crate::services::try_get_services() {
        let status = if tool_result.is_ok() {
            "success"
        } else {
            "error"
        };
        crate::observability::agent_metrics::inc_tool_call(&svc.agent_metrics, call_name, status);
    }

    if crate::agent::web_search_url_guard::is_web_search_tool_name(call_name) {
        let succeeded = matches!(&tool_result, Ok(r) if r.success);
        if succeeded {
            crate::agent::web_search_url_guard::record_web_search_success();
        } else {
            crate::agent::web_search_url_guard::record_web_search_failure();
        }
    }

    if let Some(args) = browser_trace_args.as_ref() {
        let success = matches!(&tool_result, Ok(r) if r.success);
        let action_label = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("<unknown>")
            .to_string();
        crate::tools::debug_test_report::record_browser_action(&action_label, args, success);
    }

    match tool_result {
        Ok(r) => {
            let duration = start.elapsed();
            let normalized =
                crate::agent::tool_handler::outcome::normalize_tool_result(call_name, r);
            observer.record_event(&ObserverEvent::ToolCall {
                tool: call_name.to_string(),
                duration,
                success: normalized.success,
            });
            if normalized.success {
                const COMPRESS_SPAWN_THRESHOLD_BYTES: usize = 8 * 1024;
                let scrubbed: std::sync::Arc<str> =
                    scrub_tool_output(call_name, &normalized.output).into();
                let compressed = if scrubbed.len() < COMPRESS_SPAWN_THRESHOLD_BYTES {
                    crate::agent::token::optimizer::compress_output(call_name, &scrubbed)
                } else {
                    let call_name_owned = call_name.to_string();
                    let scrubbed_for_task = std::sync::Arc::clone(&scrubbed);
                    match tokio::task::spawn_blocking(move || {
                        crate::agent::token::optimizer::compress_output(
                            &call_name_owned,
                            &scrubbed_for_task,
                        )
                    })
                    .await
                    {
                        Ok(output) => output,
                        Err(err) => {
                            tracing::warn!(
                                tool = call_name,
                                error = %err,
                                "tool output compression task failed; using uncompressed output"
                            );
                            scrubbed.to_string()
                        }
                    }
                };

                if let Some(fp) = cache_fp.as_ref() {
                    if !compressed.trim().is_empty() {
                        crate::agent::turn_engine::cache_bind::write_tool_cache(
                            call_name,
                            fp,
                            compressed.clone(),
                            tool.cache_ttl_secs(),
                        );
                    }
                }
                Ok(ToolExecutionOutcome {
                    output: compressed,
                    success: true,
                    error_reason: None,
                    duration,
                })
            } else {
                let scrubbed = scrub_credentials(&normalized.output);
                let reason = normalized
                    .error_reason
                    .map(|r| scrub_credentials(&r))
                    .unwrap_or_else(|| scrubbed.clone());
                Ok(ToolExecutionOutcome {
                    output: scrubbed,
                    success: false,
                    error_reason: Some(reason),
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
        let rt = &svc.config().agent_runtime;
        if !rt.parallel_tools_enabled {
            return 1;
        }
        return (rt.parallel_tool_max_concurrency as usize).max(1);
    }
    8
}

fn resolve_self_consistency_config()
-> crate::config::domain::agent::runtime::SelfConsistencyConfig {
    if let Some(svc) = crate::services::try_get_services() {
        return svc.config().agent_runtime.self_consistency.clone();
    }
    crate::config::domain::agent::runtime::SelfConsistencyConfig::default()
}

async fn run_self_consistency_resampling(
    cfg: &crate::config::domain::agent::runtime::SelfConsistencyConfig,
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

    let result = crate::agent::self_assess::consistency::aggregate(
        &crate::agent::self_assess::consistency::Aggregator::EmbeddingCluster {
            similarity_threshold: 0.8,
        },
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

#[allow(clippy::too_many_arguments)]
async fn execute_tools_parallel(
    tool_calls: &[ParsedToolCall],
    guardrails_pre_cleared: &[bool],
    runtime_approved: &[bool],
    tools_registry: &[Box<dyn Tool>],
    tool_registry: Option<&ToolRegistry>,
    activated_tools: Option<&std::sync::Arc<parking_lot::Mutex<crate::tools::ActivatedToolSet>>>,
    observer: &dyn Observer,
    cancellation_token: Option<&CancellationToken>,
    rbac_engine: Option<&std::sync::Arc<crate::security::rbac::RbacEngine>>,
    rbac_identity: Option<&crate::security::rbac::CallerIdentity>,
    approval: Option<&ApprovalManager>,
) -> Result<Vec<ToolExecutionOutcome>> {

    let configured_cap = resolve_parallel_tool_cap();
    let max_concurrency = configured_cap.min(tool_calls.len().max(1));

    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(max_concurrency));

    let futures: Vec<_> = tool_calls
        .iter()
        .enumerate()
        .map(|(call_idx, call)| {
            let sem = semaphore.clone();
            let tool_call_id = call.tool_call_id.clone();
            let pre_cleared = guardrails_pre_cleared.get(call_idx).copied().unwrap_or(false);
            let call_approved = runtime_approved.get(call_idx).copied().unwrap_or(false);
            async move {
                let _permit = match sem.acquire_owned().await {
                    Ok(p) => p,
                    Err(e) => {
                        return Err(anyhow::anyhow!("semaphore closed: {e}"));
                    }
                };
                CURRENT_TOOL_RUNTIME_APPROVED
                    .scope(call_approved, CURRENT_TOOL_CALL_ID.scope(tool_call_id, async {
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
                            approval,
                            pre_cleared,
                        )
                        .await
                    }))
                    .await
            }
        })
        .collect();

    let results = futures_util::future::join_all(futures).await;

    let cancelled = cancellation_token.is_some_and(|t| t.is_cancelled());
    let mut outcomes = Vec::with_capacity(results.len());
    for (call, res) in tool_calls.iter().zip(results) {
        match res {
            Ok(outcome) => outcomes.push(outcome),
            Err(e) => {
                if cancelled {
                    outcomes.push(ToolExecutionOutcome {
                        output: format!(
                            "tool '{}' was interrupted by turn cancellation",
                            call.name
                        ),
                        success: false,
                        error_reason: Some("turn cancelled".to_string()),
                        duration: Duration::ZERO,
                    });
                    continue;
                }
                tracing::warn!(
                    target: "agent.tool",
                    tool = %call.name,
                    "tool failed unexpectedly; preserving batch and recording failure: {e}"
                );
                outcomes.push(ToolExecutionOutcome {
                    output: format!("tool '{}' failed: {e}", call.name),
                    success: false,
                    error_reason: Some(e.to_string()),
                    duration: Duration::ZERO,
                });
            }
        }
    }
    Ok(outcomes)
}

#[allow(clippy::too_many_arguments)]
async fn execute_tools_sequential(
    tool_calls: &[ParsedToolCall],
    guardrails_pre_cleared: &[bool],
    runtime_approved: &[bool],
    tools_registry: &[Box<dyn Tool>],
    tool_registry: Option<&ToolRegistry>,
    activated_tools: Option<&std::sync::Arc<parking_lot::Mutex<crate::tools::ActivatedToolSet>>>,
    observer: &dyn Observer,
    cancellation_token: Option<&CancellationToken>,
    rbac_engine: Option<&std::sync::Arc<crate::security::rbac::RbacEngine>>,
    rbac_identity: Option<&crate::security::rbac::CallerIdentity>,
    approval: Option<&ApprovalManager>,
) -> Result<Vec<ToolExecutionOutcome>> {
    let mut outcomes = Vec::with_capacity(tool_calls.len());

    for (call_idx, call) in tool_calls.iter().enumerate() {
        if cancellation_token.is_some_and(|t| t.is_cancelled()) {
            outcomes.push(ToolExecutionOutcome {
                output: format!(
                    "tool '{}' was not executed: the turn was cancelled before it ran",
                    call.name
                ),
                success: false,
                error_reason: Some("turn cancelled".to_string()),
                duration: Duration::ZERO,
            });
            continue;
        }
        let pre_cleared = guardrails_pre_cleared.get(call_idx).copied().unwrap_or(false);
        let call_approved = runtime_approved.get(call_idx).copied().unwrap_or(false);
        let res = CURRENT_TOOL_RUNTIME_APPROVED
            .scope(call_approved, CURRENT_TOOL_CALL_ID.scope(call.tool_call_id.clone(), async {
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
                    approval,
                    pre_cleared,
                )
                .await
            }))
            .await;
        match res {
            Ok(outcome) => outcomes.push(outcome),
            Err(e) => {
                if cancellation_token.is_some_and(|t| t.is_cancelled()) {
                    outcomes.push(ToolExecutionOutcome {
                        output: format!(
                            "tool '{}' was interrupted by turn cancellation",
                            call.name
                        ),
                        success: false,
                        error_reason: Some("turn cancelled".to_string()),
                        duration: Duration::ZERO,
                    });
                    continue;
                }
                tracing::warn!(
                    target: "agent.tool",
                    tool = %call.name,
                    "tool failed unexpectedly; preserving batch and recording failure: {e}"
                );
                outcomes.push(ToolExecutionOutcome {
                    output: format!("tool '{}' failed: {e}", call.name),
                    success: false,
                    error_reason: Some(e.to_string()),
                    duration: Duration::ZERO,
                });
            }
        }
    }

    Ok(outcomes)
}

async fn fire_post_turn_hooks(
    channel_name: &str,
    hooks: Option<&crate::hooks::HookRunner>,
    response_cache_hook: Option<&std::sync::Arc<dyn crate::agent::loop_::traits::ResponseCacheHook>>,
    experience_recorder_hook: Option<
        &std::sync::Arc<dyn crate::agent::loop_::traits::ExperienceRecorderHook>,
    >,
    memory_session_hook: Option<&std::sync::Arc<dyn crate::agent::loop_::traits::MemorySessionHook>>,
    cache_key: Option<&String>,
    user_message: &str,
    model: &str,
    final_text: &str,
    output_tokens: u32,
    tools_used: &[String],
    tool_results: &[(String, bool)],
    record_learning: bool,
) {
    if let Some(h) = hooks {
        h.fire_turn_end(channel_name, final_text, tools_used).await;
    }
    if record_learning {
        if let (Some(hook), Some(key)) = (response_cache_hook, cache_key) {
            hook.write_back(key, model, final_text, output_tokens).await;
        }
    }
    if let Some(hook) = experience_recorder_hook {
        let summary = crate::agent::loop_::traits::TurnExperienceSummary {
            user_query: user_message.to_string(),
            assistant_response: final_text.to_string(),
            tools_used: tools_used.to_vec(),
            tool_results: tool_results.to_vec(),
        };
        hook.record(&summary).await;
    }
    if let Some(hook) = memory_session_hook {
        hook.on_turn_end(final_text, tools_used).await;
    }
    if record_learning {
        record_post_turn_learning(user_message, model, final_text, tool_results);
        spawn_turn_heuristics_recording(channel_name, user_message, final_text, tool_results);
    }
    spawn_post_turn_session_memory(user_message, final_text);
    if !tool_results.is_empty() {
        let records: Vec<crate::services::agent_summary::ToolUsageRecord> = tool_results
            .iter()
            .map(|(name, success)| crate::services::agent_summary::ToolUsageRecord {
                tool_name: name.clone(),
                description: String::new(),
                duration_ms: 0,
                success: *success,
                is_write_operation: !crate::security::permissions::is_read_only_tool(name),
            })
            .collect();
        let session_id = crate::session::current_session_context()
            .map(|c| c.session_id)
            .unwrap_or_default();
        let summary = crate::services::agent_summary::AgentSummaryService::summarize(
            &session_id,
            &records,
            &[],
            crate::services::agent_summary::SummaryGranularity::Standard,
        );
        tracing::info!(
            target: "agent.turn_summary",
            session_id = %summary.session_id,
            tasks_completed = summary.tasks_completed,
            tasks_pending = summary.tasks_pending,
            "{}",
            summary.summary_text
        );
    }
}

fn spawn_turn_heuristics_recording(
    channel_name: &str,
    user_message: &str,
    final_text: &str,
    tool_results: &[(String, bool)],
) {
    if channel_name == "delegate" {
        return;
    }
    if channel_name == "gui" && crate::gateway::lifecycle::is_running() {
        return;
    }
    let user_message = user_message.to_string();
    let final_text = final_text.to_string();
    let tool_results: Vec<(String, bool)> = tool_results.to_vec();
    crate::runtime::spawn_supervised("agent.loop.turn_heuristics", async move {
        let Some(svc) = crate::services::try_get_services() else {
            return;
        };
        let hooks = crate::agent::profile::runtime_hooks::LearningHooks::from_config(
            &svc.config(),
        );
        let tool_result_refs: Vec<(&str, bool)> = tool_results
            .iter()
            .map(|(name, success)| (name.as_str(), *success))
            .collect();
        hooks.record_turn_heuristics(&user_message, &final_text, &tool_result_refs);
    });
}

pub(crate) fn record_post_turn_learning(
    user_message: &str,
    model: &str,
    final_text: &str,
    tool_results: &[(String, bool)],
) {
    let Some(engine) = crate::agent::reward::reinforcement::global_reinforcement_engine() else {
        return;
    };
    let tool_success_rate = if tool_results.is_empty() {
        1.0
    } else {
        tool_results.iter().filter(|(_, ok)| *ok).count() as f64 / tool_results.len() as f64
    };
    let response_reward = if final_text.trim().is_empty() { -0.5 } else { 0.5 };
    let reward = ((tool_success_rate * 2.0 - 1.0) * 0.5 + response_reward).clamp(-1.0, 1.0);
    let temperature_used = crate::services::try_get_services()
        .map(|svc| svc.config().default_temperature)
        .unwrap_or(0.7);
    let query_category = format!(
        "{:?}",
        crate::agent::eval::estimate_complexity(user_message)
    );
    let record = crate::agent::reward::reinforcement::TurnRecord {
        turn_index: engine.total_turns(),
        timestamp: chrono::Utc::now(),
        reward,
        model_used: model.to_string(),
        temperature_used,
        query_category: query_category.clone(),
        tools_used: tool_results.iter().map(|(name, _)| name.clone()).collect(),
        response_length: final_text.len(),
    };
    let _ = engine.record_turn(record);

    let optimizer_enabled = crate::services::try_get_services()
        .map(|svc| svc.config().prompt_optimizer.enabled)
        .unwrap_or(false);
    if optimizer_enabled {
        let failure_reason = tool_results
            .iter()
            .find(|(_, ok)| !ok)
            .map(|(name, _)| format!("tool '{name}' failed"));
        let success_pattern = if reward > 0.5 && !tool_results.is_empty() {
            let names: Vec<&str> = tool_results
                .iter()
                .filter(|(_, ok)| *ok)
                .map(|(name, _)| name.as_str())
                .take(3)
                .collect();
            if names.is_empty() {
                None
            } else {
                Some(format!("tool sequence: {}", names.join(" -> ")))
            }
        } else {
            None
        };
        crate::agent::prompt::optimizer::global_optimizer().record_turn(
            &query_category,
            reward,
            failure_reason.as_deref(),
            success_pattern.as_deref(),
        );
    }
}

fn spawn_post_turn_session_memory(user_message: &str, final_text: &str) {
    let Some(svc) = crate::services::try_get_services() else {
        return;
    };
    let cfg = svc.config();
    let auto_save = cfg.memory.auto_save;
    let extraction_cfg = svc.extraction_config.clone();
    if !auto_save && !extraction_cfg.enabled {
        return;
    }
    if final_text.trim().is_empty() {
        return;
    }
    let session_memory = svc.session_memory.clone();
    let user_msg = user_message.to_string();
    let assistant_text = final_text.to_string();
    let session_context = crate::session::current_session_context();
    crate::runtime::spawn_supervised("agent.loop.session_memory", async move {
        let work = async move {
            if auto_save {
                let summary = format!(
                    "U: {} | A: {}",
                    truncate_with_ellipsis(&user_msg, 400),
                    truncate_with_ellipsis(&assistant_text, 800)
                );
                session_memory
                    .store(
                        "last_turn",
                        &summary,
                        crate::services::memory::session::SessionMemoryCategory::TaskContext,
                    )
                    .await;
            }
            let extracted = crate::services::memory::extract::extract_from_turn(
                &user_msg,
                &assistant_text,
                &extraction_cfg,
            );
            for memory in extracted {
                let category = match memory.category {
                    crate::services::memory::extract::MemoryCategory::Preference => {
                        crate::services::memory::session::SessionMemoryCategory::UserPreference
                    }
                    crate::services::memory::extract::MemoryCategory::Decision => {
                        crate::services::memory::session::SessionMemoryCategory::Decision
                    }
                    crate::services::memory::extract::MemoryCategory::Fact
                    | crate::services::memory::extract::MemoryCategory::Convention
                    | crate::services::memory::extract::MemoryCategory::ProjectStructure => {
                        crate::services::memory::session::SessionMemoryCategory::ProjectContext
                    }
                    crate::services::memory::extract::MemoryCategory::Workflow => {
                        crate::services::memory::session::SessionMemoryCategory::TaskContext
                    }
                };
                let key = {
                    use std::hash::{Hash, Hasher};
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    memory.content.hash(&mut hasher);
                    format!("extracted_{:016x}", hasher.finish())
                };
                session_memory.store(&key, &memory.content, category).await;
            }
        };
        match session_context {
            Some(ctx) => crate::session::scope_session_context(ctx, work).await,
            None => work.await,
        }
    });
}

pub(crate) async fn run_unified_loop_impl(
    policy: crate::agent::loop_::policy::PolicyBundle<'_>,
    history: &mut Vec<ChatMessage>,
) -> Result<String> {
    let _sleep_guard = crate::services::prevent_sleep::SleepInhibitor::acquire("agent turn");
    let _activity_guard = crate::agent::activity::begin_turn();
    let crate::agent::loop_::policy::PolicyBundle {
        origin: _,
        provider,
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
        cancellation_token,
        on_delta,
        event_sink,
        hooks,
        excluded_tools,
        dedup_exempt_tools,
        activated_tools,
        model_switch_callback,
        pacing,
        rbac_engine,
        rbac_identity,
        plan_mode_flag,
        plan_execution_path,
        tool_registry,
        response_cache_hook,
        memory_session_hook,
        turn_preamble_hook,
        gui_model_switch_hook,
        iteration_context_budget_hook,
        experience_recorder_hook,
        plan_mode_nudge_hook,
        tool_descriptions,
    } = policy;
    let approval: Option<&ApprovalManager> = approval.or_else(|| {
        if channel_name == "gui" {
            crate::approval::session_surface_approval_manager()
        } else {
            None
        }
    });

    let user_msg_for_hooks: String = history
        .iter()
        .rfind(|m| m.role == "user")
        .map(|m| m.content.clone())
        .unwrap_or_default();
    let turn_event_tx_for_hooks: Option<tokio::sync::mpsc::Sender<crate::agent::agent::TurnEvent>> =
        event_sink.turn_sender();

    if let Some(hook) = &turn_preamble_hook {
        if let Some(ref tx) = turn_event_tx_for_hooks {
            if let Err(err) = hook.apply(&user_msg_for_hooks, tx).await {
                tracing::debug!(
                    target: "agent.hooks.turn_preamble",
                    error = %err,
                    "turn preamble hook returned error"
                );
            }
        }
    }
    if let Some(hook) = &memory_session_hook {
        hook.on_turn_start(&user_msg_for_hooks).await;
    }

    let response_cache_key: Option<String> = response_cache_hook
        .as_ref()
        .and_then(|h| h.build_key(history.as_slice(), model));
    if let (Some(hook), Some(key)) = (&response_cache_hook, &response_cache_key) {
        if let Some(cached) = hook.try_hit(key, &user_msg_for_hooks).await {
            history.push(ChatMessage::assistant(cached.clone()));
            fire_post_turn_hooks(
                channel_name,
                hooks,
                response_cache_hook.as_ref(),
                experience_recorder_hook.as_ref(),
                memory_session_hook.as_ref(),
                response_cache_key.as_ref(),
                &user_msg_for_hooks,
                model,
                &cached,
                0,
                &[],
                &[],
                true,
            )
            .await;
            return Ok(cached);
        }
    }

    let mode_max = crate::agent::coding_mode::active_coding_mode().max_iterations_override();
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
    let mut loop_state = crate::agent::loop_::control::LoopControlState::new(
        crate::agent::loop_::detector::LoopDetectorConfig {
            enabled: pacing.loop_detection_enabled,
            window_size: pacing.loop_detection_window_size,
            max_repeats: pacing.loop_detection_max_repeats,
        },
        pacing.loop_detection_identical_output_threshold,
    );

    let mut cached_tool_specs: Option<std::sync::Arc<Vec<crate::tools::ToolSpec>>> = None;
    let mut cached_mode_key: (u64, bool) = (0, false);
    let mut prepared_history_cache: Option<(u8, Vec<u64>, multimodal::PreparedMessages)> = None;

    let mut _turn_metrics = crate::agent::executor_core::TurnMetricsGuard::start();

    let (token_soft_cap, token_hard_cap) = crate::services::try_get_services()
        .map(|svc| {
            let rt = &svc.config().agent_runtime;
            (
                rt.per_turn_token_soft_cap as u64,
                rt.per_turn_token_hard_cap as u64,
            )
        })
        .unwrap_or_else(|| {
            let rt = crate::config::domain::AgentRuntimeExtras::default();
            (
                rt.per_turn_token_soft_cap as u64,
                rt.per_turn_token_hard_cap as u64,
            )
        });
    let mut _pacing_gov = crate::agent::executor_core::PacingGovernor::new(
        crate::agent::executor_core::PacingBudget {
            no_progress_limit: pacing.no_progress_iteration_limit.max(1),
            absolute_iteration_limit: max_iterations,
            total_timeout: pacing
                .total_turn_timeout_secs
                .filter(|s| *s > 0)
                .map(std::time::Duration::from_secs),
            token_soft_cap,
            token_hard_cap,
        },
    );

    let mut plan_nudge_state =
        crate::agent::plan_mode::enforcement::PlanModeNudgeState::new();
    let mut plan_exec_nudge_state = plan_execution_path
        .map(|path| {
            crate::agent::plan_mode::execution_enforcement::PlanExecutionNudgeState::armed(
                path.to_string(),
            )
        })
        .unwrap_or_default();
    #[cfg(feature = "tool-curator")]
    let mut curator_nudge_state =
        crate::agent::curator_mode_enforcement::CuratorModeNudgeState::new();

    let mut awaiting_user_input = false;

    if let Some(svc) = crate::services::try_get_services() {
        let mode = crate::agent::coding_mode::active_coding_mode();
        crate::agent::mode::effects::remove_stale_mode_reminders(history, mode);
        if let Some(reminder) = crate::agent::mode::effects::pre_turn_reminder(mode) {
            crate::agent::mode::effects::replace_or_push_system_reminder(
                history,
                reminder.to_string(),
            );
        }
        if plan_execution_path.is_some() {
            crate::agent::mode::effects::replace_or_push_system_reminder(
                history,
                crate::agent::mode::effects::plan_execution_reminder().to_string(),
            );
        }
        if let Some(pinned) = crate::agent::mode::effects::pinned_test_target_reminder(mode) {
            crate::agent::mode::effects::replace_or_push_system_reminder(history, pinned);
        }
        if let Some(proto) = crate::agent::mode::effects::prototype_ref_reminder(mode) {
            crate::agent::mode::effects::replace_or_push_system_reminder(history, proto);
        }
        let cfg = svc.config();
        if plan_execution_path.is_none() {
            if let Some(web_reminder) = crate::agent::mode::effects::web_research_disabled_reminder(
                mode,
                cfg.web_search.enabled,
                cfg.web_fetch.enabled,
            ) {
                crate::agent::mode::effects::replace_or_push_system_reminder(
                    history,
                    web_reminder.to_string(),
                );
            }
            if let Some(web_active) = crate::agent::mode::effects::web_research_active_reminder(
                mode,
                cfg.web_search.enabled,
                cfg.web_fetch.enabled,
            ) {
                crate::agent::mode::effects::replace_or_push_system_reminder(
                    history,
                    web_active.to_string(),
                );
            }
        }
    }

    let mut loop_recovery_used = 0usize;
    const MAX_LOOP_RECOVERY: usize = 2;

    let mut parse_issue_nudges_used = 0usize;
    const MAX_PARSE_ISSUE_NUDGES: usize = 2;

    let mut truncation_nudges_used = 0usize;
    const MAX_TRUNCATION_NUDGES: usize = 2;

    let mut empty_response_nudges_used = 0usize;
    const MAX_EMPTY_RESPONSE_NUDGES: usize = 2;
    let mut truncation_prefix = String::new();

    let mut turn_modified_files = false;
    let mut evaluator_retries = 0u32;
    let mut verify_retries = 0u32;

    let mut compression_retry_floor: Option<usize> = None;

    let mut pacing_break_reason: Option<crate::agent::executor_core::PacingExceeded> = None;

    let mut turn_tool_results: Vec<(String, bool)> = Vec::new();

    history.retain(|m| {
        !(m.role == "system"
            && crate::agent::executor_core::is_pacing_guard_message(&m.content))
    });

    for iteration in 0..usize::MAX {
        if let Err(budget_exceeded) = _pacing_gov.tick() {
            tracing::warn!(
                target: "agent.pacing",
                turn_id = %turn_id,
                reason = %budget_exceeded,
                "agent turn exceeded a pacing budget; ending turn gracefully"
            );
            pacing_break_reason = Some(budget_exceeded);
            break;
        }
        for warning in _pacing_gov.drain_warnings() {
            tracing::warn!(
                target: "agent.pacing",
                turn_id = %turn_id,
                iteration = iteration + 1,
                %warning,
                "pacing guard nudging the model to recover"
            );
            history.push(ChatMessage::system(warning));
        }

        tracing::debug!(
            target: "agent.iteration",
            turn_id = %turn_id,
            iter = iteration,
            "iteration start"
        );
        let mut seen_tool_signatures: HashSet<(String, String)> = HashSet::new();

        let mut plan_finalized_this_iter: bool = false;
        #[cfg(feature = "tool-curator")]
        let mut curator_finalized_this_iter: bool = false;

        if cancellation_token
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            return Err(tool_loop_cancelled());
        }

        if on_delta.as_ref().is_some_and(|tx| tx.is_closed()) {
            let user_cancelled = cancellation_token
                .as_ref()
                .is_some_and(CancellationToken::is_cancelled);
            if user_cancelled {
                tracing::info!(
                    target: "agent.loop",
                    turn_id = %turn_id,
                    "event receiver dropped after user cancellation; ending turn"
                );
                return Err(tool_loop_cancelled());
            }
            if cancellation_token.is_some() {
                tracing::info!(
                    target: "agent.loop",
                    turn_id = %turn_id,
                    "event consumer disconnected (transport/UI); keeping turn alive under cancellation guard so reasoning is preserved for reconnect/resume"
                );
            } else {
                tracing::info!(
                    target: "agent.loop",
                    turn_id = %turn_id,
                    "event receiver dropped without user cancellation and no cancellation guard; ending turn to avoid orphaned background execution"
                );
                return Err(anyhow::Error::new(
                    crate::error::AgentError::StreamInterrupted(
                        "event consumer disconnected before the turn completed".to_string(),
                    ),
                ));
            }
        }

        if let Some(hook) = &iteration_context_budget_hook {
            if let Some(ref tx) = turn_event_tx_for_hooks {
                hook.prepare(iteration, tx).await;
            }
        }
        if let Some(hook) = &gui_model_switch_hook {
            if let Some(ref tx) = turn_event_tx_for_hooks {
                if let Some(new_model) = hook.poll(tx).await {
                    tracing::debug!(
                        target: "agent.hooks.gui_model_switch",
                        new_model = %new_model,
                        "gui model switch hook signalled new model"
                    );
                }
            }
        }
        if let Some(hook) = &plan_mode_nudge_hook {
            if let Some(ref tx) = turn_event_tx_for_hooks {
                if hook.try_inject(iteration, history, tx).await {
                    continue;
                }
            }
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

        if crate::services::try_get_services().is_some() {
            let mode = crate::agent::coding_mode::active_coding_mode();
            let max_ctx = resolve_compaction_context_window(model);
            match crate::agent::mode::effects::build_context_budget_message(mode, history, max_ctx)
            {
                Some(budget_msg) => {
                    crate::agent::mode::effects::replace_or_push_system_reminder(
                        history,
                        budget_msg,
                    );
                }
                None => {
                    crate::agent::mode::effects::remove_system_reminder(
                        history,
                        crate::agent::mode::effects::CONTEXT_BUDGET_MARKER,
                    );
                }
            }
        }

        let coding_mode_allowlist: Option<HashSet<&str>> =
            crate::agent::coding_mode::active_coding_mode().allowed_tools();
        let plan_mode_active = plan_mode_flag.is_some_and(crate::tools::PlanModeFlag::is_active);

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
                    .map(|svc| svc.deferred_builtin_names_snapshot())
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
            let mut specs: Vec<_> = tools_registry
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
                .map(|tool| tool.spec_with_descriptions(tool_descriptions))
                .collect();
            specs.sort_by(|a, b| a.name.cmp(&b.name));
            cached_tool_specs = Some(std::sync::Arc::new(specs));
        }

        let tool_specs_arc = cached_tool_specs
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!(
                "tool spec cache should have been populated above (internal invariant violation)"
            ))?
            .clone();
        let mut activated_specs: Vec<crate::tools::ToolSpec> = Vec::new();
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
                        activated_specs.push(spec);
                    }
                }
            }
        }
        let tool_specs_extended: Option<Vec<crate::tools::ToolSpec>> =
            if activated_specs.is_empty() {
                None
            } else {
                activated_specs.sort_by(|a, b| a.name.cmp(&b.name));
                let mut extended = (*tool_specs_arc).clone();
                extended.extend(activated_specs);
                Some(extended)
            };
        let tool_specs: &[crate::tools::ToolSpec] = tool_specs_extended
            .as_deref()
            .unwrap_or_else(|| tool_specs_arc.as_slice());
        let use_native_tools = provider.supports_native_tools() && !tool_specs.is_empty();

        let image_marker_count = multimodal::count_image_markers(history);

        let needs_image_degrade = image_marker_count > 0 && {
            let effective_supports_vision = crate::services::try_get_services()
                .and_then(|svc| svc.config().model_vision_capability(provider_name, model))
                .unwrap_or_else(|| provider.supports_vision());
            !effective_supports_vision
        };

        let vision_provider_box: Option<Box<dyn Provider>> = if needs_image_degrade {
            let configured_vp = multimodal_config.vision_provider.clone();
            let vision_fallback_model = multimodal_config
                .vision_model
                .as_deref()
                .unwrap_or(model)
                .to_string();
            let usable_vp: Option<Box<dyn Provider>> = match configured_vp {
                Some(ref vp) => match providers::create_provider_async(vp.clone(), None).await {
                    Ok(instance) => {
                        let fallback_supports_vision = crate::services::try_get_services()
                            .and_then(|svc| {
                                svc.config()
                                    .model_vision_capability(vp, &vision_fallback_model)
                            })
                            .unwrap_or_else(|| instance.supports_vision());
                        if fallback_supports_vision {
                            Some(instance)
                        } else {
                            tracing::warn!(
                                target: "agent.loop.vision",
                                turn_id = %turn_id,
                                vision_provider = %vp,
                                "configured vision_provider does not support vision input; degrading images to text instead of failing the turn"
                            );
                            None
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "agent.loop.vision",
                            turn_id = %turn_id,
                            vision_provider = %vp,
                            error = %e,
                            "failed to create configured vision_provider; degrading images to text instead of failing the turn"
                        );
                        None
                    }
                },
                None => None,
            };

            if usable_vp.is_some() {
                usable_vp
            } else {
                if configured_vp.is_none() {
                    tracing::warn!(
                        target: "agent.loop.vision",
                        turn_id = %turn_id,
                        provider = provider_name,
                        model = model,
                        image_markers = image_marker_count,
                        "provider lacks vision support and no vision_provider configured; degrading images to text placeholders instead of failing the turn"
                    );
                }
                for msg in history.iter_mut() {
                    if msg.role != "user" {
                        continue;
                    }
                    let (cleaned, refs) = multimodal::parse_image_markers(&msg.content);
                    if refs.is_empty() {
                        continue;
                    }
                    let note = format!(
                        "[{} image(s) omitted: model '{model}' has no usable vision support]",
                        refs.len()
                    );
                    msg.content = if cleaned.is_empty() {
                        note
                    } else {
                        format!("{cleaned}\n\n{note}")
                    };
                }
                None
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

        if let Some(svc) = crate::services::try_get_services() {
            let cfg_snapshot = svc.config();
            let compression_cfg = cfg_snapshot.agent.context_compression.clone();
            let context_window = resolve_compaction_context_window(model);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let compression_threshold =
                (context_window as f64 * compression_cfg.threshold_ratio) as usize;
            let estimated_tokens =
                crate::agent::token::budget::estimate_history_tokens_calibrated(history, model);
            let over_threshold = estimated_tokens > compression_threshold;
            let retry_blocked =
                compression_retry_floor.is_some_and(|floor| estimated_tokens <= floor);
            if compression_cfg.enabled && over_threshold && !retry_blocked {
                if let Some(h) = hooks {
                    h.fire_pre_compact("proactive", estimated_tokens).await;
                }
                let compressor = crate::agent::context::compressor::ContextCompressor::new(
                    compression_cfg,
                    context_window,
                );
                let preserved_fn: Box<crate::agent::context::compressor::PreservedIndexFn> =
                    Box::new(current_turn_preserved_indices);
                if let Some(ref tx) = on_delta {
                    let _ = tx
                        .send(DraftEvent::Progress(
                            "Compressing conversation context to fit the model window…".to_string(),
                        ))
                        .await;
                }
                let progress_cb: Option<
                    Box<crate::agent::context::compressor::CompressionProgressFn>,
                > = on_delta.as_ref().map(|tx| {
                    let tx = tx.clone();
                    Box::new(
                        move |p: crate::agent::context::compressor::CompressionProgress| {
                            let _ = tx.try_send(DraftEvent::Progress(format!(
                                "Compressing conversation context (pass {}/{}, ~{} → target {} tokens)…",
                                p.pass, p.max_passes, p.tokens_current, p.tokens_target,
                            )));
                        },
                    )
                        as Box<crate::agent::context::compressor::CompressionProgressFn>
                });
                let compress_outcome = {
                    let compress_fut = compressor.compress_if_needed_with_progress(
                        history,
                        provider,
                        model,
                        Some(&*preserved_fn),
                        progress_cb.as_deref(),
                    );
                    if let Some(token) = cancellation_token.as_ref() {
                        tokio::select! {
                            () = token.cancelled() => None,
                            r = compress_fut => Some(r),
                        }
                    } else {
                        Some(compress_fut.await)
                    }
                };
                match compress_outcome {
                    None => {
                        tracing::info!(
                            target: "agent.context.compress",
                            turn_id = %turn_id,
                            "context compression cancelled by user; aborting turn"
                        );
                        return Err(tool_loop_cancelled());
                    }
                    Some(Ok(result)) if result.compressed => {
                        if result.tokens_after > compression_threshold {
                            compression_retry_floor =
                                Some(result.tokens_after + compression_threshold / 5);
                        } else {
                            compression_retry_floor = None;
                        }
                        if let Some(ref tx) = on_delta {
                            let _ = tx
                                .send(DraftEvent::ContextCompressed {
                                    tokens_before: result.tokens_before,
                                    tokens_after: result.tokens_after,
                                })
                                .await;
                        }
                        tracing::info!(
                            target: "agent.context.compress",
                            tokens_before = result.tokens_before,
                            tokens_after = result.tokens_after,
                            passes = result.passes_used,
                            "history compressed before LLM call"
                        );
                    }
                    Some(Ok(result)) => {
                        compression_retry_floor =
                            Some(result.tokens_after + compression_threshold / 5);
                    }
                    Some(Err(err)) => {
                        compression_retry_floor =
                            Some(estimated_tokens + compression_threshold / 5);
                        tracing::warn!(
                            target: "agent.context.compress",
                            error = %err,
                            "context compression failed; proceeding with un-compressed history"
                        );
                    }
                }
            }
        }

        let active_mode = crate::agent::coding_mode::active_coding_mode();
        let mode_key = active_mode as u8;
        let per_message_fingerprints: Vec<u64> = {
            use std::hash::{Hash, Hasher};
            history
                .iter()
                .map(|msg| {
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    msg.role.hash(&mut h);
                    msg.content.hash(&mut h);
                    if !msg.metadata.is_empty() {
                        let mut keys: Vec<&String> = msg.metadata.keys().collect();
                        keys.sort_unstable();
                        for key in keys {
                            key.hash(&mut h);
                            if let Some(value) = msg.metadata.get(key) {
                                crate::providers::traits::hash_json_value(value, &mut h, 0);
                            }
                        }
                    }
                    h.finish()
                })
                .collect()
        };
        let mut prepared_messages = 'prepared: {
            if let Some((cached_mode, cached_fps, cached_prep)) = prepared_history_cache.take()
            {
                if cached_mode == mode_key
                    && cached_fps.len() <= per_message_fingerprints.len()
                    && per_message_fingerprints[..cached_fps.len()] == cached_fps[..]
                {
                    if cached_fps.len() == per_message_fingerprints.len() {
                        break 'prepared cached_prep;
                    }
                    if !cached_prep.contains_images {
                        let suffix = multimodal::prepare_messages_for_provider(
                            &history[cached_fps.len()..],
                            multimodal_config,
                        )
                        .await?;
                        let mut suffix_messages = suffix.messages;
                        let sanitization_report = apply_outgoing_pii_sanitization(
                            Some(active_mode),
                            &mut suffix_messages,
                        );
                        if !sanitization_report.is_empty() {
                            tracing::debug!(
                                target: "agent.pii",
                                redactions = sanitization_report.total(),
                                "applied outbound PII sanitization in Debug mode"
                            );
                            if let Some(ref tx) = on_delta {
                                let _ = tx
                                    .send(DraftEvent::PiiSanitized {
                                        report: sanitization_report.clone(),
                                    })
                                    .await;
                            }
                        }
                        let mut combined = cached_prep;
                        combined.messages.extend(suffix_messages);
                        combined.contains_images |= suffix.contains_images;
                        break 'prepared combined;
                    }
                }
            }
            let mut prepared =
                multimodal::prepare_messages_for_provider(history, multimodal_config).await?;
            let sanitization_report = apply_outgoing_pii_sanitization(
                Some(active_mode),
                &mut prepared.messages,
            );
            if !sanitization_report.is_empty() {
                tracing::debug!(
                    target: "agent.pii",
                    redactions = sanitization_report.total(),
                    "applied outbound PII sanitization in Debug mode"
                );
                if let Some(ref tx) = on_delta {
                    let _ = tx
                        .send(DraftEvent::PiiSanitized {
                            report: sanitization_report.clone(),
                        })
                        .await;
                }
            }
            prepared
        };
        prepared_history_cache = Some((
            mode_key,
            per_message_fingerprints,
            prepared_messages.clone(),
        ));

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

        if tracing::enabled!(tracing::Level::DEBUG) {
            if let Some(svc) = crate::services::try_get_services() {
                let est_tokens: u64 = history
                    .iter()
                    .map(|m| svc.token_estimator.estimate(&m.content))
                    .sum();
                tracing::debug!(estimated_tokens = est_tokens, "Pre-call token estimate");
            }
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
            let budget_text = format!(
                "\u{1f4b0} Cost budget limit reached (${:.4} / ${:.2} {:?}). Stopping safely here; the conversation can continue once the budget resets.",
                current_usd, limit_usd, period
            );
            _turn_metrics.mark_ok();
            history.push(ChatMessage::assistant(budget_text.clone()));
            if let Some(ref tx) = on_delta {
                let _ = tx.send(DraftEvent::Content(budget_text.clone())).await;
            }
            return Ok(budget_text);
        }

        if let Some(svc) = crate::services::try_get_services() {
            let spent_cents = crate::bootstrap::try_get_state()
                .map(|bs| {
                    let mut cost = 0.0f64;
                    bs.read(|s| cost = s.total_cost_usd);
                    (cost * 100.0).max(0.0) as u64
                })
                .unwrap_or(0);
            if spent_cents > 0 && !svc.check_spending_policy(spent_cents) {
                let policy_text = format!(
                    "\u{1f4b0} Spending has hit the governance SpendingCap policy limit (currently ~{:.2} USD). Stopping safely here; adjust the policy or wait for the budget cycle to reset.",
                    spent_cents as f64 / 100.0
                );
                _turn_metrics.mark_ok();
                history.push(ChatMessage::assistant(policy_text.clone()));
                if let Some(ref tx) = on_delta {
                    let _ = tx.send(DraftEvent::Content(policy_text.clone())).await;
                }
                return Ok(policy_text);
            }
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
                        let sleep =
                            tokio::time::sleep(std::time::Duration::from_millis(retry_ms.min(10_000)));
                        match cancellation_token.as_ref() {
                            Some(token) => {
                                tokio::select! {
                                    biased;
                                    _ = token.cancelled() => {
                                        return Err(tool_loop_cancelled());
                                    }
                                    _ = sleep => {}
                                }
                            }
                            None => sleep.await,
                        }
                    }
                }
            }
        }

        let request_tools = if use_native_tools {
            Some(tool_specs)
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

        let mut llm_resilience_attempt: u32 = 0;
        let llm_resilience_started_at = std::time::Instant::now();
        let mut emergency_compress_attempts: u32 = 0;
        let mut emergency_context_window: Option<usize> = None;
        const MAX_EMERGENCY_COMPRESS_ATTEMPTS: u32 = 4;
        let chat_result = 'llm_attempt: loop {
        let mut stream_probe = StreamProgressProbe::default();
        let attempt_result = if should_consume_provider_stream {
            let stream_idle_timeout = pacing
                .stream_idle_timeout_secs
                .filter(|s| *s > 0)
                .map(Duration::from_secs);
            let stream_fut = consume_provider_streaming_response(
                active_provider,
                &prepared_messages.messages,
                request_tools,
                active_model,
                temperature,
                cancellation_token.as_ref(),
                on_delta.as_ref(),
                stream_idle_timeout,
                &mut stream_probe,
            );
            let consume_result = match pacing.step_timeout_secs {
                Some(step_secs) if step_secs > 0 => {
                    match tokio::time::timeout(Duration::from_secs(step_secs), stream_fut).await {
                        Ok(inner) => inner,
                        Err(_) => Err(anyhow::anyhow!(
                            "LLM streaming step timed out after {step_secs}s (step_timeout_secs)"
                        )),
                    }
                }
                _ => stream_fut.await,
            };
            match consume_result
            {
                Ok(streamed) => {
                    streamed_live_deltas = streamed.forwarded_live_deltas;

                    for rec in &streamed.pre_executed {
                        let call_line = if rec.args.is_empty() {
                            format!("[proxy executed tool: {}]", rec.name)
                        } else {
                            format!("[proxy executed tool: {} with arguments {}]", rec.name, rec.args)
                        };
                        history.push(ChatMessage::assistant(call_line));
                        if let Some(output) = &rec.output {
                            history.push(ChatMessage::tool(format!(
                                "[tool {} result]\n{}",
                                rec.name, output
                            )));
                        }
                    }

                    let reasoning_content = if !streamed.reasoning_content.is_empty() {
                        Some(streamed.reasoning_content)
                    } else if !streamed.tool_calls.is_empty() {
                        Some(
                            "(chain-of-thought unavailable  -  model emitted tool calls without a CoT stream)"
                                .to_string(),
                        )
                    } else {
                        None
                    };
                    let thinking_signature = if streamed.thinking_signature.is_empty()
                        || streamed.thinking_signature_blocks > 1
                    {
                        None
                    } else {
                        Some(streamed.thinking_signature)
                    };
                    Ok(crate::providers::ChatResponse {
                        text: Some(streamed.response_text),
                        tool_calls: streamed.tool_calls,
                        usage: streamed.usage,
                        reasoning_content,
                        thinking_signature,
                        stop_reason: streamed.stop_reason,
                    })
                }
                Err(stream_err) => {
                    if cancellation_token
                        .as_ref()
                        .is_some_and(|t| t.is_cancelled())
                    {
                        return Err(tool_loop_cancelled());
                    }
                    if let Some(partial) = stream_probe.partial_usage.take() {
                        let _ = record_tool_loop_cost_usage(
                            active_provider_name,
                            active_model,
                            &partial,
                        );
                    }
                    if stream_err.to_string().contains("exceeded max response size") {
                        return Err(stream_err);
                    }
                    if stream_err.to_string().contains("stream idle timeout")
                        && llm_resilience_attempt < LLM_RESILIENCE_MAX_RETRIES
                    {
                        tracing::warn!(
                            provider = active_provider_name,
                            model = active_model,
                            iteration = iteration + 1,
                            "provider stream idle timeout; retrying streaming call instead of degrading to non-streaming: {stream_err}"
                        );
                        Err(stream_err)
                    } else if stream_probe.made_progress
                        && llm_resilience_attempt < LLM_RESILIENCE_MAX_RETRIES
                    {
                        tracing::warn!(
                            provider = active_provider_name,
                            model = active_model,
                            iteration = iteration + 1,
                            "provider stream broke after partial output; retrying the streaming call: {stream_err}"
                        );
                        if let Some(ref tx) = on_delta {
                            let _ = tx.send(DraftEvent::Clear).await;
                        }
                        Err(stream_err)
                    } else {
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
                            () = token.cancelled() => return Err(tool_loop_cancelled()),
                            result = tokio::time::timeout(step_timeout, chat_future) => {
                                match result {
                                    Ok(inner) => inner,
                                    Err(_) => Err(anyhow::anyhow!(
                                        "LLM inference step timed out after {step_secs}s (step_timeout_secs)"
                                    )),
                                }
                            },
                        }
                    } else {
                        match tokio::time::timeout(step_timeout, chat_future).await {
                            Ok(inner) => inner,
                            Err(_) => Err(anyhow::anyhow!(
                                "LLM inference step timed out after {step_secs}s (step_timeout_secs)"
                            )),
                        }
                    }
                }
                _ => {
                    if let Some(token) = cancellation_token.as_ref() {
                        tokio::select! {
                            () = token.cancelled() => return Err(tool_loop_cancelled()),
                            result = chat_future => result,
                        }
                    } else {
                        chat_future.await
                    }
                }
            }
        };

            match attempt_result {
                Ok(resp) => break 'llm_attempt Ok(resp),
                Err(e) => {
                    if cancellation_token
                        .as_ref()
                        .is_some_and(|t| t.is_cancelled())
                    {
                        break 'llm_attempt Err(tool_loop_cancelled());
                    }
                    if llm_error_is_terminal(&e) {
                        if crate::providers::reliable::is_context_window_exceeded(&e)
                            && emergency_compress_attempts < MAX_EMERGENCY_COMPRESS_ATTEMPTS
                        {
                            if let Some(svc) = crate::services::try_get_services() {
                                let compression_cfg =
                                    svc.config().agent.context_compression.clone();
                                if compression_cfg.enabled {
                                    let budget_window =
                                        crate::agent::token::optimizer::global_optimizer()
                                            .map(|opt| opt.budget().context_window())
                                            .unwrap_or(usize::MAX);
                                    let model_window =
                                        crate::constants::api_limits::context_window_for_model(
                                            model,
                                        ) as usize;
                                    let context_window = emergency_context_window
                                        .unwrap_or_else(|| {
                                            model_window.min(budget_window).max(32_000)
                                        });
                                    let mut emergency_compressor =
                                        crate::agent::context::compressor::ContextCompressor::new(
                                            compression_cfg,
                                            context_window,
                                        );
                                    if let Some(h) = hooks {
                                        h.fire_pre_compact("error", 0).await;
                                    }
                                    if let Some(ref tx) = on_delta {
                                        let _ = tx
                                            .send(DraftEvent::Progress(
                                                "The model reported a context overflow; emergency-compressing history and retrying…"
                                                    .to_string(),
                                            ))
                                            .await;
                                    }
                                    let emergency_preserved: Box<
                                        crate::agent::context::compressor::PreservedIndexFn,
                                    > = Box::new(current_turn_preserved_indices);
                                    let compressed = emergency_compressor
                                        .compress_on_error(
                                            history,
                                            provider,
                                            model,
                                            &e.to_string(),
                                            Some(&*emergency_preserved),
                                        )
                                        .await
                                        .unwrap_or(false);
                                    emergency_context_window =
                                        Some(emergency_compressor.context_window());
                                    if compressed {
                                        emergency_compress_attempts += 1;
                                        prepared_messages =
                                            multimodal::prepare_messages_for_provider(
                                                history,
                                                multimodal_config,
                                            )
                                            .await?;
                                        let _ = apply_outgoing_pii_sanitization(
                                            Some(active_mode),
                                            &mut prepared_messages.messages,
                                        );
                                        if let Some(ref tx) = on_delta {
                                            let _ = tx.send(DraftEvent::Clear).await;
                                        }
                                        tracing::info!(
                                            target: "agent.context.compress",
                                            turn_id = %turn_id,
                                            attempt = emergency_compress_attempts,
                                            "context-window error recovered via emergency compression; retrying turn"
                                        );
                                        continue 'llm_attempt;
                                    }
                                }
                            }
                        }
                        break 'llm_attempt Err(e);
                    }
                    llm_resilience_attempt += 1;
                    if llm_resilience_attempt > LLM_RESILIENCE_MAX_RETRIES
                        || llm_resilience_started_at.elapsed() >= LLM_RESILIENCE_MAX_TOTAL
                    {
                        break 'llm_attempt Err(e);
                    }
                    let backoff_ms = llm_resilience_backoff_ms(llm_resilience_attempt);
                    let err_summary = crate::providers::sanitize_api_error(&e.to_string());
                    tracing::warn!(
                        target: "agent.loop.resilience",
                        turn_id = %turn_id,
                        provider = active_provider_name,
                        model = active_model,
                        attempt = llm_resilience_attempt,
                        max_attempts = LLM_RESILIENCE_MAX_RETRIES,
                        backoff_ms,
                        error = %err_summary,
                        "LLM call failed with a recoverable error; keeping turn alive and retrying instead of aborting"
                    );
                    if let Some(ref tx) = on_delta {
                        let _ = tx.send(DraftEvent::Clear).await;
                        let _ = tx
                            .send(DraftEvent::ProviderRetry {
                                attempt: llm_resilience_attempt,
                                max_attempts: LLM_RESILIENCE_MAX_RETRIES,
                                wait_ms: backoff_ms,
                                class: "transient".to_string(),
                                provider: active_provider_name.to_string(),
                                model: active_model.to_string(),
                                message: format!(
                                    "{active_provider_name} connection error; recovering and retrying automatically (attempt {llm_resilience_attempt})…"
                                ),
                            })
                            .await;
                    }
                    let sleep_dur = Duration::from_millis(backoff_ms);
                    if let Some(token) = cancellation_token.as_ref() {
                        tokio::select! {
                            biased;
                            () = token.cancelled() => break 'llm_attempt Err(tool_loop_cancelled()),
                            () = tokio::time::sleep(sleep_dur) => {}
                        }
                    } else {
                        tokio::time::sleep(sleep_dur).await;
                    }
                    continue 'llm_attempt;
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
            response_stop_reason,
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

                let recorded_cost = resp
                    .usage
                    .as_ref()
                    .and_then(|usage| record_tool_loop_cost_usage(provider_name, model, usage));

                if let Some(usage) = resp.usage.as_ref() {
                    let input_tokens = usage.input_tokens.unwrap_or(0);
                    let output_tokens = usage.output_tokens.unwrap_or(0);
                    if input_tokens + output_tokens > 0 {
                        if input_tokens > 0 {
                            let estimated_input = crate::providers::traits::estimate_total_tokens(
                                &prepared_messages.messages,
                            );
                            crate::agent::token::budget::record_usage_calibration(
                                model,
                                estimated_input,
                                input_tokens,
                            );
                        }
                        if let Some(opt) = crate::agent::token::optimizer::global_optimizer() {
                            opt.record_api_usage(input_tokens as usize, output_tokens as usize);
                        }
                        let cached_tokens = usage.cached_input_tokens.unwrap_or(0);
                        let cache_creation_tokens =
                            usage.cache_creation_input_tokens.unwrap_or(0);
                        let cost_usd = recorded_cost.map(|(_, cost)| cost).unwrap_or_else(|| {
                            let prices = TOOL_LOOP_COST_TRACKING_CONTEXT
                                .try_with(Clone::clone)
                                .ok()
                                .flatten()
                                .map(|c| c.prices)
                                .unwrap_or_default();
                            let pricing = lookup_model_pricing(&prices, provider_name, model);
                            let anthropic_family =
                                crate::agent::reward::cost_tracking::provider_uses_separate_cache_fields(
                                    provider_name,
                                );
                            let fresh_input = if anthropic_family {
                                input_tokens
                            } else {
                                input_tokens.saturating_sub(cached_tokens)
                            };
                            let cache_write_rate = if anthropic_family {
                                CostTokenUsage::CACHE_WRITE_RATE_1H
                            } else {
                                1.25
                            };
                            CostTokenUsage::new_with_cache_rates(
                                model,
                                fresh_input,
                                output_tokens,
                                cached_tokens,
                                cache_creation_tokens,
                                pricing.map_or(0.0, |e| e.input),
                                pricing.map_or(0.0, |e| e.output),
                                cache_write_rate,
                            )
                            .cost_usd
                        });
                        if let Some(bs) = crate::bootstrap::try_get_state() {
                            bs.write(|state| {
                                state.accumulate_usage(
                                    model,
                                    input_tokens,
                                    output_tokens,
                                    cache_creation_tokens,
                                    cached_tokens,
                                    cost_usd,
                                );
                                state.total_api_duration_ms +=
                                    llm_started_at.elapsed().as_millis() as u64;
                            });
                        }
                    }
                }

                let generated_tokens = resp
                    .usage
                    .as_ref()
                    .and_then(|u| u.output_tokens)
                    .filter(|t| *t > 0)
                    .unwrap_or_else(|| {
                        let text_tokens = crate::services::token_estimation::estimate_tokens(
                            resp.text_or_empty(),
                        );
                        let reasoning_tokens = resp
                            .reasoning_content
                            .as_deref()
                            .map(crate::services::token_estimation::estimate_tokens)
                            .unwrap_or(0);
                        let tool_call_tokens: u64 = resp
                            .tool_calls
                            .iter()
                            .map(|c| {
                                crate::services::token_estimation::estimate_tokens(&c.name)
                                    + crate::services::token_estimation::estimate_tokens(
                                        &c.arguments,
                                    )
                            })
                            .sum();
                        text_tokens + reasoning_tokens + tool_call_tokens
                    });
                _pacing_gov.record_generated_tokens(generated_tokens);

                let response_text = resp.text_or_empty().to_string();

                let mut calls = parse_structured_tool_calls(&resp.tool_calls);
                let mut parsed_text = String::new();

                if calls.is_empty() {
                    let known_tools: std::collections::HashSet<String> =
                        tool_specs.iter().map(|s| s.name.clone()).collect();
                    let gate = ParseGate {
                        native_tools_supported: use_native_tools,
                        known_tools: &known_tools,
                    };
                    let (fallback_text, fallback_calls) =
                        parse_tool_calls_gated(&response_text, Some(&gate));
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
                let thinking_signature = resp.thinking_signature.clone();
                let assistant_history_content = if resp.tool_calls.is_empty() {
                    if use_native_tools {
                        build_native_assistant_history_from_parsed_calls(
                            &response_text,
                            &calls,
                            reasoning_content.as_deref(),
                            thinking_signature.as_deref(),
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
                        thinking_signature.as_deref(),
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
                    resp.stop_reason,
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

            if matches!(
                response_stop_reason,
                Some(crate::providers::traits::StopReason::Length)
            ) && truncation_nudges_used < MAX_TRUNCATION_NUDGES
                && !awaiting_user_input
            {
                truncation_nudges_used += 1;
                tracing::warn!(
                    target: "agent.loop",
                    turn_id = %turn_id,
                    nudge = truncation_nudges_used,
                    max = MAX_TRUNCATION_NUDGES,
                    "provider reported stop_reason=length; response was truncated mid-output, injecting continuation nudge"
                );
                if !response_text.trim().is_empty() {
                    history.push(ChatMessage::assistant(&response_text));
                    truncation_prefix.push_str(&response_text);
                }
                history.push(ChatMessage::system(
                    "[Output Truncated] Your previous message hit the maximum output token limit and was cut off mid-response. \
                     Continue EXACTLY from where your output stopped. Do not repeat content you already produced, \
                     do not apologise, and do not restart the answer. If you were writing a file, re-issue the remaining \
                     part with a targeted edit tool call instead of rewriting the whole file."
                        .to_string(),
                ));
                continue;
            }

            if response_text.trim().is_empty()
                && display_text.trim().is_empty()
                && truncation_prefix.is_empty()
                && !awaiting_user_input
            {
                if empty_response_nudges_used < MAX_EMPTY_RESPONSE_NUDGES {
                    empty_response_nudges_used += 1;
                    tracing::warn!(
                        target: "agent.loop",
                        turn_id = %turn_id,
                        nudge = empty_response_nudges_used,
                        max = MAX_EMPTY_RESPONSE_NUDGES,
                        provider = provider_name,
                        model,
                        "provider returned an empty final response (no visible text, no tool calls); injecting retry nudge instead of ending the turn silently"
                    );
                    runtime_trace::record_event(
                        "empty_response_retry",
                        Some(channel_name),
                        Some(provider_name),
                        Some(model),
                        Some(&turn_id),
                        Some(false),
                        None,
                        serde_json::json!({
                            "iteration": iteration + 1,
                            "retry": empty_response_nudges_used,
                        }),
                    );
                    history.push(ChatMessage::system(
                        "[Empty Response] Your previous reply was completely empty: it contained no visible text \
                         and no tool calls. That is never acceptable. Respond to the user's latest request now \
                         with a concrete, user-visible answer. If you intended to call a tool, issue the tool \
                         call properly; otherwise write your answer as plain text."
                            .to_string(),
                    ));
                    continue;
                }
                tracing::error!(
                    target: "agent.loop",
                    turn_id = %turn_id,
                    provider = provider_name,
                    model,
                    retries = empty_response_nudges_used,
                    "provider kept returning empty responses; failing the turn so the user sees an explicit error instead of a missing assistant message"
                );
                return Err(anyhow::anyhow!(
                    "empty_model_response: the model '{model}' returned an empty reply (no text, no tool calls) {attempts} times in a row; \
                     the turn was aborted so nothing silent is recorded. Try again, switch models, or check the provider's status.",
                    model = model,
                    attempts = empty_response_nudges_used + 1,
                ));
            }

            if _parse_issue_detected
                && parse_issue_nudges_used < MAX_PARSE_ISSUE_NUDGES
            {
                parse_issue_nudges_used += 1;
                tracing::info!(
                    target: "agent.loop",
                    turn_id = %turn_id,
                    nudge = parse_issue_nudges_used,
                    max = MAX_PARSE_ISSUE_NUDGES,
                    "response looked like a tool call but parsed empty; injecting nudge and continuing instead of ending the turn"
                );
                if !response_text.trim().is_empty() {
                    history.push(ChatMessage::assistant(&response_text));
                }
                history.push(ChatMessage::system(
                    "Your previous message looked like it was trying to call a tool, but the tool call could not be parsed. \
                     Re-issue the tool call using the exact required format: a single JSON object wrapped in <tool_call></tool_call> tags, \
                     for example <tool_call>{\"name\":\"shell\",\"arguments\":{\"command\":\"date\"}}</tool_call>. \
                     If you did not intend to call a tool, reply with your final answer as plain text without any <tool_call> tags."
                        .to_string(),
                ));
                continue;
            }

            let in_plan_mode =
                crate::agent::plan_mode::enforcement::detect_plan_mode_active(
                    plan_mode_flag,
                );

            if matches!(
                crate::agent::plan_mode::enforcement::evaluate_plan_mode_exit(
                    in_plan_mode,
                    &plan_nudge_state,
                    awaiting_user_input,
                ),
                crate::agent::plan_mode::enforcement::PlanModeExitDecision::InjectNudge
            ) {
                tracing::info!(
                    target: "agent.plan_mode",
                    turn_id = %turn_id,
                    nudge_count = plan_nudge_state.nudge_count + 1,
                    max_nudges =
                        crate::agent::plan_mode::enforcement::MAX_PLAN_NUDGES,
                    "Plan mode: model exited without exit_plan_mode; injecting nudge"
                );

                if !response_text.trim().is_empty() {
                    history.push(ChatMessage::assistant(&response_text));
                }
                plan_nudge_state.note_stop_without_exit();
                let msg = crate::agent::plan_mode::enforcement::nudge_message(
                    &plan_nudge_state,
                );
                history.push(ChatMessage::system(msg));
                continue;
            }

            if matches!(
                crate::agent::plan_mode::execution_enforcement::evaluate_plan_execution_exit(
                    &plan_exec_nudge_state,
                    awaiting_user_input,
                ),
                crate::agent::plan_mode::execution_enforcement::PlanExecutionExitDecision::InjectNudge
            ) {
                tracing::info!(
                    target: "agent.plan_execution",
                    turn_id = %turn_id,
                    nudge_count = plan_exec_nudge_state.nudge_count + 1,
                    done = plan_exec_nudge_state.terminal_count,
                    total = plan_exec_nudge_state.total_steps,
                    "Plan execution: model exited with unfinished todos; injecting nudge"
                );
                if !response_text.trim().is_empty() {
                    history.push(ChatMessage::assistant(&response_text));
                }
                plan_exec_nudge_state.note_nudge_issued();
                let msg = crate::agent::plan_mode::execution_enforcement::nudge_message(
                    &plan_exec_nudge_state,
                );
                history.push(ChatMessage::system(msg));
                continue;
            }

            #[cfg(feature = "tool-curator")]
            {
                let curator_flag_opt = crate::services::try_get_services()
                    .map(|svc| svc.curator_mode_flag.clone());
                let in_curator_mode =
                    crate::agent::curator_mode_enforcement::detect_curator_mode_active(
                        curator_flag_opt.as_ref(),
                    );
                if matches!(
                    crate::agent::curator_mode_enforcement::evaluate_curator_mode_exit(
                        in_curator_mode,
                        &curator_nudge_state,
                        awaiting_user_input,
                    ),
                    crate::agent::curator_mode_enforcement::CuratorModeExitDecision::InjectNudge
                ) {
                    tracing::info!(
                        target: "agent.curator_mode",
                        turn_id = %turn_id,
                        nudge_count = curator_nudge_state.nudge_count + 1,
                        max_nudges =
                            crate::agent::curator_mode_enforcement::MAX_CURATOR_NUDGES,
                        "Curator mode: model exited without exit_curator_mode; injecting nudge"
                    );
                    if !response_text.trim().is_empty() {
                        history.push(ChatMessage::assistant(&response_text));
                    }
                    curator_nudge_state.note_stop_without_exit();
                    let msg = crate::agent::curator_mode_enforcement::nudge_message(
                        &curator_nudge_state,
                    );
                    history.push(ChatMessage::system(msg));
                    continue;
                }
            }

            if turn_modified_files && !awaiting_user_input {
                if let Some((feedback, retry_budget_left)) = run_auto_verify_gate(
                    verify_retries,
                    cancellation_token.as_ref(),
                )
                .await
                {
                    if retry_budget_left {
                        verify_retries += 1;
                        turn_modified_files = false;
                        runtime_trace::record_event(
                            "auto_verify_gate_retry",
                            Some(channel_name),
                            Some(provider_name),
                            Some(model),
                            Some(&turn_id),
                            Some(false),
                            None,
                            serde_json::json!({ "retry": verify_retries }),
                        );
                        if !response_text.trim().is_empty() {
                            history.push(ChatMessage::assistant(&response_text));
                        }
                        history.push(ChatMessage::system(feedback));
                        continue;
                    }
                }
            }

            if turn_modified_files && !awaiting_user_input {
                if let Some(critic) = crate::agent::flows::global_critic_context() {
                    let max_retries = critic.config().max_evaluator_retries;
                    if critic.config().enabled
                        && evaluator_retries < max_retries
                        && _pacing_gov.remaining_iterations() > 1
                    {
                        let critic_verdict = {
                            let review_fut =
                                crate::agent::self_assess::critic::IndependentCritic::review_turn(
                                    &critic,
                                    &user_msg_for_hooks,
                                    &display_text,
                                );
                            if let Some(token) = cancellation_token.as_ref() {
                                tokio::select! {
                                    biased;
                                    () = token.cancelled() => None,
                                    verdict = review_fut => verdict,
                                }
                            } else {
                                review_fut.await
                            }
                        };
                        if let Some(verdict) = critic_verdict {
                            if verdict.should_retry {
                                evaluator_retries += 1;
                                turn_modified_files = false;
                                runtime_trace::record_event(
                                    "evaluator_gate_retry",
                                    Some(channel_name),
                                    Some(provider_name),
                                    Some(model),
                                    Some(&turn_id),
                                    Some(false),
                                    None,
                                    serde_json::json!({
                                        "retry": evaluator_retries,
                                        "max": max_retries,
                                        "score": verdict.score,
                                        "findings": verdict.findings.len(),
                                    }),
                                );
                                if !response_text.trim().is_empty() {
                                    history.push(ChatMessage::assistant(&response_text));
                                }
                                history.push(ChatMessage::system(
                                    render_evaluator_feedback(&verdict),
                                ));
                                continue;
                            }
                        }
                    }
                }
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

            let full_display_text = if truncation_prefix.is_empty() {
                display_text.clone()
            } else {
                format!("{truncation_prefix}{display_text}")
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
                    if crate::agent::plan_mode::execution_enforcement::should_auto_finalize_on_exit(
                        &plan_exec_nudge_state,
                    ) {
                        let intent_window = build_intent_text_window(&display_text, history);
                        let finalized = auto_finalize_incomplete_plan_steps(
                            tools_registry,
                            tool_registry,
                            on_delta.as_ref(),
                            history,
                            &intent_window,
                        )
                        .await;
                        plan_exec_nudge_state.terminal_count = plan_exec_nudge_state
                            .terminal_count
                            .saturating_add(finalized);
                    }
                    emit_plan_progress_completion_card(
                        tools_registry,
                        tool_registry,
                        on_delta.as_ref(),
                        &plan_exec_nudge_state,
                    )
                    .await;
                    _turn_metrics.mark_ok();
                    let turn_tools_used: Vec<String> = turn_tool_results
                        .iter()
                        .map(|(name, _)| name.clone())
                        .collect();
                    fire_post_turn_hooks(
                        channel_name,
                        hooks,
                        response_cache_hook.as_ref(),
                        experience_recorder_hook.as_ref(),
                        memory_session_hook.as_ref(),
                        response_cache_key.as_ref(),
                        &user_msg_for_hooks,
                        model,
                        &full_display_text,
                        _pacing_gov.total_generated_tokens() as u32,
                        &turn_tools_used,
                        &turn_tool_results,
                        true,
                    )
                    .await;
                    return Ok(full_display_text);
                }

                let _ = tx.send(DraftEvent::Clear).await;

                let mut chunk = String::new();
                let mut delivered_chars = 0usize;
                let mut delivery_interrupted = false;
                for word in full_display_text.split_inclusive(char::is_whitespace) {
                    if cancellation_token
                        .as_ref()
                        .is_some_and(CancellationToken::is_cancelled)
                    {
                        return Err(tool_loop_cancelled());
                    }
                    chunk.push_str(word);
                    if chunk.len() >= STREAM_CHUNK_MIN_CHARS {
                        let pending = chunk.len();
                        if tx
                            .send(DraftEvent::Content(std::mem::take(&mut chunk)))
                            .await
                            .is_err()
                        {
                            delivery_interrupted = true;
                            break;
                        }
                        delivered_chars += pending;
                    }
                }
                if !delivery_interrupted && !chunk.is_empty() {
                    let pending = chunk.len();
                    if tx.send(DraftEvent::Content(chunk)).await.is_err() {
                        delivery_interrupted = true;
                    } else {
                        delivered_chars += pending;
                    }
                }
                if delivery_interrupted {
                    let total_chars = full_display_text.len();
                    history.push(ChatMessage::assistant(response_text.clone()));
                    tracing::error!(
                        target: "agent.loop",
                        turn_id = %turn_id,
                        delivered_chars,
                        total_chars,
                        "event consumer disconnected during final chunked delivery; assistant response preserved in history for reconnect/resume, turn marked interrupted"
                    );
                    _turn_metrics.mark_status("interrupted");
                    return Err(anyhow::Error::new(
                        crate::error::AgentError::StreamInterrupted(format!(
                            "final response delivery interrupted after {delivered_chars}/{total_chars} chars; response preserved in history but not confirmed delivered"
                        )),
                    ));
                }
            }
            history.push(ChatMessage::assistant(response_text.clone()));
            if crate::agent::plan_mode::execution_enforcement::should_auto_finalize_on_exit(
                &plan_exec_nudge_state,
            ) {
                let intent_window = build_intent_text_window(&display_text, history);
                let finalized = auto_finalize_incomplete_plan_steps(
                    tools_registry,
                    tool_registry,
                    on_delta.as_ref(),
                    history,
                    &intent_window,
                )
                .await;
                plan_exec_nudge_state.terminal_count = plan_exec_nudge_state
                    .terminal_count
                    .saturating_add(finalized);
            }
            emit_plan_progress_completion_card(
                tools_registry,
                tool_registry,
                on_delta.as_ref(),
                &plan_exec_nudge_state,
            )
            .await;
            _turn_metrics.mark_ok();
            let turn_tools_used: Vec<String> = turn_tool_results
                .iter()
                .map(|(name, _)| name.clone())
                .collect();
            fire_post_turn_hooks(
                channel_name,
                hooks,
                response_cache_hook.as_ref(),
                experience_recorder_hook.as_ref(),
                memory_session_hook.as_ref(),
                response_cache_key.as_ref(),
                &user_msg_for_hooks,
                model,
                &full_display_text,
                _pacing_gov.total_generated_tokens() as u32,
                &turn_tools_used,
                &turn_tool_results,
                true,
            )
            .await;
            return Ok(full_display_text);
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
        let mut final_args_by_index: Vec<Option<serde_json::Value>> =
            (0..tool_calls.len()).map(|_| None).collect();
        let allow_parallel_execution = should_execute_tools_in_parallel(&tool_calls, approval);
        let mut executable_indices: Vec<usize> = Vec::new();
        let mut executable_calls: Vec<ParsedToolCall> = Vec::new();
        let mut executable_pre_cleared: Vec<bool> = Vec::new();
        let mut executable_runtime_approved: Vec<bool> = Vec::new();

        let mut deferred_system_after_tool_batch: Vec<String> = Vec::new();
        let mut batch_edit_diagnostics_dirty = false;

        let tool_burst_cap =
            crate::constants::tool_limits::MAX_TOOL_CALLS_PER_TURN as usize;
        let storm_truncated = tool_calls.len().saturating_sub(tool_burst_cap);
        if storm_truncated > 0 {
            tracing::warn!(
                target: "agent.tool_storm",
                turn_id = %turn_id,
                iteration = iteration + 1,
                requested = tool_calls.len(),
                cap = tool_burst_cap,
                "tool burst cap exceeded; truncating excess calls"
            );
            runtime_trace::record_event(
                "tool_burst_capped",
                Some(channel_name),
                Some(provider_name),
                Some(model),
                Some(&turn_id),
                Some(false),
                Some("excess tool calls in single iteration"),
                serde_json::json!({
                    "iteration": iteration + 1,
                    "requested": tool_calls.len(),
                    "cap": tool_burst_cap,
                    "dropped": storm_truncated,
                }),
            );
        }

        for (idx, call) in tool_calls.iter().enumerate() {

            if idx >= tool_burst_cap {
                let capped = format!(
                    "[Capped] Tool call '{}' rejected: per-iteration cap of {} exceeded ({} excess calls dropped). \
                    Re-issue fewer tool calls or split the work across turns.",
                    call.name, tool_burst_cap, storm_truncated
                );
                ordered_results[idx] = Some((
                    call.name.clone(),
                    call.tool_call_id.clone(),
                    ToolExecutionOutcome {
                        output: capped.clone(),
                        success: false,
                        error_reason: Some(capped),
                        duration: Duration::ZERO,
                    },
                ));
                continue;
            }

            let mut tool_name = call.name.clone();
            let mut tool_args = call.arguments.clone();

            if call.parse_error {
                tracing::warn!(
                    tool = %call.name,
                    "Tool call arguments failed JSON parsing; rejecting before execution \
                     with a structured re-emit request instead of running with empty args"
                );
                let schema = find_tool(tools_registry, &call.name, tool_registry)
                    .map(|handle| handle.as_tool().parameters_schema());
                let feedback = crate::agent::tool_handler::arg_validate::parse_error_feedback(
                    &call.name,
                    schema.as_ref(),
                );
                if let Some(ref tx) = on_delta {
                    let _ = tx
                        .send(DraftEvent::Progress(format!(
                            "\u{274c} {}: invalid JSON arguments; asked model to re-emit\n",
                            call.name
                        )))
                        .await;
                }
                ordered_results[idx] = Some((
                    call.name.clone(),
                    call.tool_call_id.clone(),
                    ToolExecutionOutcome {
                        output: feedback.clone(),
                        success: false,
                        error_reason: Some(feedback),
                        duration: Duration::ZERO,
                    },
                ));
                continue;
            }

            let mut pre_hook_user_approved = false;
            let pre_hook_guardrail_cleared = {
                let coding_label = Some(
                    crate::agent::coding_mode::active_coding_mode()
                        .label()
                        .to_string(),
                );
                let coding_label_lc = coding_label.as_deref().map(str::to_ascii_lowercase);
                let perm_mode_lc = crate::gateway::ws::desktop::active_permission_mode();
                let tool_lc = call.name.to_ascii_lowercase();
                let guardrail_ctx = crate::guardrails::GuardrailContext {
                    coding_mode: coding_label_lc.as_deref(),
                    permission_mode: Some(&perm_mode_lc),
                    tool_name: Some(&tool_lc),
                };
                let pre_hook_denial: Option<String> = match crate::guardrails::evaluate_tool_guardrails(
                    &call.name,
                    Some(&guardrail_ctx),
                ) {
                    crate::guardrails::GuardrailDecision::Allow => None,
                    crate::guardrails::GuardrailDecision::Deny(reason) => {
                        Some(format!("Blocked by guardrails: {reason}"))
                    }
                    crate::guardrails::GuardrailDecision::RequireApproval(reason) => {
                        let mode_auto_approved =
                            crate::agent::mode::effects::mode_auto_approves(
                                crate::agent::coding_mode::active_coding_mode(),
                            ) && approval
                                .is_none_or(|m| m.mode_auto_approve_allows(&call.name));
                        if mode_auto_approved {
                            None
                        } else if let Some(mgr) = approval {
                            let request = ApprovalRequest {
                                tool_name: call.name.clone(),
                                arguments: call.arguments.clone(),
                            };
                            match request_session_tool_approval(
                                mgr,
                                &request,
                                on_delta.as_ref(),
                                cancellation_token.as_ref(),
                                &format!("Guardrail approval required: {reason}"),
                            )
                            .await
                            {
                                Some(crate::approval::SessionApprovalVerdict::Decision(
                                    decision,
                                )) => {
                                    mgr.record_decision(
                                        &call.name,
                                        &call.arguments,
                                        decision,
                                        "guardrail",
                                    );
                                    if decision == ApprovalResponse::No {
                                        Some(format!(
                                            "Blocked by guardrails: approval required but not \
                                             granted ({reason})"
                                        ))
                                    } else {
                                        pre_hook_user_approved = true;
                                        None
                                    }
                                }
                                Some(crate::approval::SessionApprovalVerdict::Cancelled) => {
                                    mgr.record_decision(
                                        &call.name,
                                        &call.arguments,
                                        ApprovalResponse::No,
                                        "guardrail",
                                    );
                                    return Err(tool_loop_cancelled());
                                }
                                Some(crate::approval::SessionApprovalVerdict::TimedOut) => {
                                    mgr.record_decision(
                                        &call.name,
                                        &call.arguments,
                                        ApprovalResponse::No,
                                        "guardrail",
                                    );
                                    Some(approval_timeout_denial(&call.name))
                                }
                                None => {
                                    if mgr.is_non_interactive() {
                                        Some(format!(
                                            "Blocked by guardrails: approval required but not \
                                             granted ({reason})"
                                        ))
                                    } else {
                                        let decision = mgr.prompt_cli_async(&request).await;
                                        mgr.record_decision(
                                            &call.name,
                                            &call.arguments,
                                            decision,
                                            "guardrail",
                                        );
                                        if decision == ApprovalResponse::No {
                                            Some(format!(
                                                "Blocked by guardrails: approval required but \
                                                 not granted ({reason})"
                                            ))
                                        } else {
                                            pre_hook_user_approved = true;
                                            None
                                        }
                                    }
                                }
                            }
                        } else {
                            Some(format!(
                                "Blocked by guardrails: approval required but not granted \
                                 ({reason})"
                            ))
                        }
                    }
                };
                if let Some(denial) = pre_hook_denial {
                    runtime_trace::record_event(
                        "tool_call_result",
                        Some(channel_name),
                        Some(provider_name),
                        Some(model),
                        Some(&turn_id),
                        Some(false),
                        Some(&denial),
                        serde_json::json!({
                            "iteration": iteration + 1,
                            "tool": call.name.clone(),
                            "arguments": scrub_credentials(&call.arguments.to_string()),
                            "guardrail_pre_hook": true,
                        }),
                    );
                    if let Some(ref tx) = on_delta {
                        let _ = tx
                            .send(DraftEvent::Progress(format!(
                                "\u{274c} {}: {}\n",
                                call.name,
                                truncate_with_ellipsis(&denial, 200)
                            )))
                            .await;
                    }
                    ordered_results[idx] = Some((
                        call.name.clone(),
                        call.tool_call_id.clone(),
                        ToolExecutionOutcome {
                            output: denial.clone(),
                            success: false,
                            error_reason: Some(denial),
                            duration: Duration::ZERO,
                        },
                    ));
                    continue;
                }
                true
            };

            let mut hook_forced_approval: Option<String> = None;
            if let Some(hooks) = hooks {
                match hooks
                    .run_before_tool_call(tool_name.clone(), tool_args.clone())
                    .await
                {
                    crate::hooks::HookResult::RequireApproval((name, args), message) => {
                        tool_name = name;
                        tool_args = args;
                        hook_forced_approval = Some(message.unwrap_or_else(|| {
                            "manual approval required by hooks.json".to_string()
                        }));
                    }
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

            final_args_by_index[idx] = Some(tool_args.clone());

            let allowlist_denial = {
                let mode = crate::agent::coding_mode::active_coding_mode();
                mode.allowed_tools().and_then(|allowed| {
                    if allowed.contains(tool_name.as_str()) {
                        None
                    } else {
                        let mut listed: Vec<&str> = allowed.iter().copied().collect();
                        listed.sort_unstable();
                        let preview: String = listed
                            .iter()
                            .take(12)
                            .copied()
                            .collect::<Vec<_>>()
                            .join(", ");
                        let extra = if listed.len() > 12 {
                            format!(", ... ({} more)", listed.len() - 12)
                        } else {
                            String::new()
                        };
                        let hint = if matches!(
                            mode,
                            crate::agent::coding_mode::CodingMode::Plan
                        ) {
                            " To produce or update the plan document call \
                             `update_plan(action=\"set\"|\"add\"|\"save\", ...)`. \
                             When planning is finished call `exit_plan_mode` so the \
                             user can press the Build button to switch to Agent mode \
                             for execution."
                        } else {
                            ""
                        };
                        Some((
                            mode,
                            format!(
                                "Tool '{}' is not permitted in {} mode.{} Allowed tools: {}{}",
                                tool_name,
                                mode.label(),
                                hint,
                                preview,
                                extra
                            ),
                        ))
                    }
                })
            };
            if let Some((denied_mode, denial_message)) = allowlist_denial {
                crate::agent::mode::effects::record_mode_intercept(
                    crate::agent::mode::effects::ModeInterceptReason::ToolNotAllowed,
                    &crate::agent::mode::effects::ModeInterceptContext {
                        mode: denied_mode,
                        channel: Some(channel_name),
                        provider: Some(provider_name),
                        model: Some(model),
                        turn_id: Some(&turn_id),
                        tool: Some(&tool_name),
                        tool_call_id: call.tool_call_id.as_deref(),
                        iteration: Some(iteration + 1),
                        message: Some(&denial_message),
                    },
                );
                if let Some(ref tx) = on_delta {
                    let _ = tx
                        .send(DraftEvent::Progress(format!(
                            "\u{274c} {}: {}\n",
                            tool_name,
                            truncate_with_ellipsis(&denial_message, 200)
                        )))
                        .await;
                }
                ordered_results[idx] = Some((
                    tool_name.clone(),
                    call.tool_call_id.clone(),
                    ToolExecutionOutcome {
                        output: denial_message.clone(),
                        success: false,
                        error_reason: Some(denial_message),
                        duration: Duration::ZERO,
                    },
                ));
                continue;
            }

            let intercept = {
                let mode = crate::agent::coding_mode::active_coding_mode();
                crate::agent::mode::effects::mode_blocks_tool(mode, &tool_name)
                    .map(|reason| (mode, reason))
            };
            if let Some((intercepted_mode, reason)) = intercept {
                crate::agent::mode::effects::record_mode_intercept(
                    crate::agent::mode::effects::ModeInterceptReason::ReadOnlyPolicy,
                    &crate::agent::mode::effects::ModeInterceptContext {
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

            let mode_auto_approved = crate::agent::mode::effects::mode_auto_approves(
                crate::agent::coding_mode::active_coding_mode(),
            ) && approval.is_none_or(|m| m.mode_auto_approve_allows(&tool_name));

            let already_user_approved = pre_hook_user_approved
                && tool_name == call.name
                && tool_args == call.arguments;

            if approval.is_none() {
                if let Some(message) = hook_forced_approval.take() {
                    let cancelled = format!(
                        "Cancelled by hook: hooks.json requested user confirmation but no \
                         approval surface is available: {message}"
                    );
                    if let Some(ref tx) = on_delta {
                        let _ = tx
                            .send(DraftEvent::Progress(format!(
                                "\u{274c} {}: {}\n",
                                tool_name,
                                truncate_with_ellipsis(&scrub_credentials(&cancelled), 200)
                            )))
                            .await;
                    }
                    ordered_results[idx] = Some((
                        tool_name.clone(),
                        call.tool_call_id.clone(),
                        ToolExecutionOutcome {
                            output: cancelled.clone(),
                            success: false,
                            error_reason: Some(scrub_credentials(&cancelled)),
                            duration: Duration::ZERO,
                        },
                    ));
                    continue;
                }
            }

            let mut runtime_approved = mode_auto_approved
                || already_user_approved
                || approval.is_some_and(|m| m.is_explicitly_granted(&tool_name, &tool_args));

            if let Some(mgr) = approval {
                if hook_forced_approval.is_some()
                    || (!mode_auto_approved
                        && !already_user_approved
                        && mgr.needs_approval_with_args(&tool_name, &tool_args))
                {
                    let request = ApprovalRequest {
                        tool_name: tool_name.clone(),
                        arguments: tool_args.clone(),
                    };

                    let approval_description = match hook_forced_approval.as_deref() {
                        Some(message) => {
                            format!("hooks.json requires approval for '{tool_name}': {message}")
                        }
                        None => {
                            format!("Tool '{tool_name}' requires approval before execution")
                        }
                    };
                    let mut timeout_denied = false;
                    let decision = match request_session_tool_approval(
                        mgr,
                        &request,
                        on_delta.as_ref(),
                        cancellation_token.as_ref(),
                        &approval_description,
                    )
                    .await
                    {
                        Some(crate::approval::SessionApprovalVerdict::Decision(decision)) => {
                            decision
                        }
                        Some(crate::approval::SessionApprovalVerdict::Cancelled) => {
                            mgr.record_decision(
                                &tool_name,
                                &tool_args,
                                ApprovalResponse::No,
                                channel_name,
                            );
                            return Err(tool_loop_cancelled());
                        }
                        Some(crate::approval::SessionApprovalVerdict::TimedOut) => {
                            timeout_denied = true;
                            ApprovalResponse::No
                        }
                        None => {
                            if mgr.is_non_interactive() {
                                ApprovalResponse::No
                            } else {
                                mgr.prompt_cli_async(&request).await
                            }
                        }
                    };

                    mgr.record_decision(&tool_name, &tool_args, decision, channel_name);

                    if decision != ApprovalResponse::No {
                        runtime_approved = true;
                    }

                    if decision == ApprovalResponse::No {
                        let denied = if timeout_denied {
                            approval_timeout_denial(&tool_name)
                        } else {
                            "Denied by user.".to_string()
                        };
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

            let canonical_tool_args =
                crate::agent::loop_::detector::canonicalise_args_string(&tool_args);

            {
                let consecutive_failures = loop_state
                    .consecutive_identical_failures_canonical(&tool_name, &canonical_tool_args);
                if consecutive_failures >= 2 {
                    let refusal = format!(
                        "[Loop guard] Tool '{}' has already failed {} times in a row with the \
                         exact same arguments in this session. Refusing to execute the same call \
                         again. Choose a different approach (different arguments, a different \
                         tool, or ask the user for guidance). Do NOT retry this command verbatim.",
                        tool_name, consecutive_failures
                    );
                    runtime_trace::record_event(
                        "tool_call_result",
                        Some(channel_name),
                        Some(provider_name),
                        Some(model),
                        Some(&turn_id),
                        Some(false),
                        Some(&refusal),
                        serde_json::json!({
                            "iteration": iteration + 1,
                            "tool": tool_name.clone(),
                            "arguments": scrub_credentials(&tool_args.to_string()),
                            "loop_guard": true,
                            "consecutive_failures": consecutive_failures,
                        }),
                    );
                    if let Some(ref tx) = on_delta {
                        let _ = tx
                            .send(DraftEvent::Progress(format!(
                                "\u{1f6d1} {}: loop guard refused identical retry (#{}{})\n",
                                tool_name,
                                consecutive_failures + 1,
                                if consecutive_failures >= 4 { ", aborting" } else { "" }
                            )))
                            .await;
                    }
                    ordered_results[idx] = Some((
                        tool_name.clone(),
                        call.tool_call_id.clone(),
                        ToolExecutionOutcome {
                            output: refusal.clone(),
                            success: false,
                            error_reason: Some(refusal.clone()),
                            duration: Duration::ZERO,
                        },
                    ));
                    let _ = loop_state.record_per_tool_with_failure_canonical(
                        &tool_name,
                        &canonical_tool_args,
                        &refusal,
                        true,
                    );
                    if consecutive_failures >= 4 {
                        let abort_msg = format!(
                            "tool '{}' refused after {} identical failures",
                            tool_name,
                            consecutive_failures + 1
                        );
                        if loop_recovery_used >= MAX_LOOP_RECOVERY {
                            _turn_metrics.mark_status("aborted");
                            return Ok(finalize_loop_recovery(
                                &abort_msg,
                                history,
                                on_delta.as_ref(),
                            )
                            .await);
                        }
                        loop_recovery_used += 1;
                        history.push(ChatMessage::system(loop_recovery_nudge(&abort_msg)));
                    }
                    continue;
                }
            }

            let signature = (
                tool_name.trim().to_ascii_lowercase(),
                canonical_tool_args.clone(),
            );
            let dedup_exempt = dedup_exempt_tools.iter().any(|e| e == &tool_name);
            if !dedup_exempt && !seen_tool_signatures.insert(signature) {

                let deduplicated = format!(
                    "{DEDUP_RESULT_MARKER}{tool_name}' with identical arguments was already \
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
                        tool_call_id: call.tool_call_id.clone(),
                    })
                    .await;
            }

            executable_indices.push(idx);
            executable_pre_cleared.push(
                pre_hook_guardrail_cleared
                    && tool_name == call.name
                    && tool_args == call.arguments,
            );
            executable_runtime_approved.push(runtime_approved);
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
                            &executable_pre_cleared,
                            &executable_runtime_approved,
                            tools_registry,
                            tool_registry,
                            activated_tools,
                            observer,
                            cancellation_token.as_ref(),
                            rbac_engine,
                            rbac_identity,
                            approval,
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
                            &executable_pre_cleared,
                            &executable_runtime_approved,
                            tools_registry,
                            tool_registry,
                            activated_tools,
                            observer,
                            cancellation_token.as_ref(),
                            rbac_engine,
                            rbac_identity,
                            approval,
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

            if let Some(ref tx) = on_delta {
                let secs = outcome.duration.as_secs();
                let progress_msg = if outcome.success {
                    format!("\u{2705} {} ({secs}s)\n", call.name)
                } else if let Some(ref reason) = outcome.error_reason {
                    if reason.chars().count() > 200 {
                        tracing::debug!(
                            target: "loop.tool_error_truncated",
                            tool = %call.name,
                            seconds = secs,
                            reason_full = %reason,
                            "tool error reason truncated for progress draft; full content logged at debug",
                        );
                    }
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
                        success: outcome.success,
                        tool_call_id: call.tool_call_id.clone(),
                    })
                    .await;
            }

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

            if outcome.success
                && crate::agent::mode::effects::is_file_mutation_tool(call.name.as_str())
            {
                turn_modified_files = true;
                {
                    let mode = crate::agent::coding_mode::active_coding_mode();
                    if let Some(nudge) =
                        crate::agent::mode::effects::file_mod_auto_verify_nudge(mode)
                    {
                        deferred_system_after_tool_batch.push(nudge.to_string());
                    }
                }
            }

            if outcome.success && call.name == "exit_plan_mode" {
                plan_nudge_state.note_exit_plan_mode_success();
                plan_finalized_this_iter = true;
            }
            #[cfg(feature = "tool-curator")]
            if outcome.success && call.name == "exit_curator_mode" {
                curator_nudge_state.note_exit_curator_mode_success();
                curator_finalized_this_iter = true;
            }

            let mut outcome = outcome;

            if outcome.success {
                crate::agent::tool_handler::focus::note_tool_focus_paths(
                    &call.name,
                    &call.arguments,
                );
            }

            if outcome.success
                && crate::agent::verification::post_edit::is_checkable_mutation(
                    call.name.as_str(),
                )
            {
                match tokio::time::timeout(
                    Duration::from_secs(3),
                    crate::agent::verification::post_edit::post_edit_check(
                        &call.name,
                        &call.arguments,
                        &outcome.output,
                    ),
                )
                .await
                {
                    Ok(Some(report)) => {
                        batch_edit_diagnostics_dirty = true;
                        outcome.output.push_str("\n\n");
                        outcome.output.push_str(&report);
                    }
                    Ok(None) => {}
                    Err(_) => {
                        tracing::debug!(
                            tool = %call.name,
                            "post-edit check timed out; skipping diagnostics append"
                        );
                    }
                }
            }

            if crate::agent::plan_mode::enforcement::is_ask_question_pause(
                &call.name,
                &outcome.output,
            ) {
                awaiting_user_input = true;
                outcome.output = crate::agent::plan_mode::enforcement::ASK_QUESTION_PAUSE_NOTICE
                    .to_string();
                if let Some(ref tx) = on_delta {
                    let request_id = call
                        .tool_call_id
                        .clone()
                        .unwrap_or_else(|| Uuid::new_v4().to_string());
                    crate::approval::register_pending_gateway_approval(request_id.clone());
                    let description = match call.name.as_str() {
                        "ask_question" => Some(
                            "Assistant is asking you a clarifying question before proceeding."
                                .to_string(),
                        ),
                        "ask_user" => Some(
                            "Assistant is asking you a free-form question to gather more context."
                                .to_string(),
                        ),
                        _ => None,
                    };
                    let _ = tx
                        .send(DraftEvent::PermissionRequest {
                            request_id,
                            tool_name: call.name.clone(),
                            input: call.arguments.clone(),
                            description,
                        })
                        .await;
                }
            }

            ordered_results[idx] = Some((call.name.clone(), call.tool_call_id.clone(), outcome));
        }

        use std::hash::{Hash, Hasher};
        let mut detection_fingerprint_hasher =
            std::collections::hash_map::DefaultHasher::new();
        let mut detection_has_payload = false;
        let mut batch_had_success = false;

        for (result_index, (tool_name, tool_call_id, outcome)) in ordered_results
            .into_iter()
            .enumerate()
            .filter_map(|(i, opt)| opt.map(|v| (i, v)))
        {
            if !loop_ignore_tools.contains(tool_name.as_str()) {
                let args = final_args_by_index
                    .get(result_index)
                    .and_then(|o| o.as_ref())
                    .or_else(|| tool_calls.get(result_index).map(|c| &c.arguments))
                    .unwrap_or(&serde_json::Value::Null);

                if plan_exec_nudge_state.active && tool_name == "update_plan" {
                    plan_exec_nudge_state.observe_update_plan_call_at(
                        &tool_name,
                        args,
                        &outcome.output,
                        outcome.success,
                        Some(iteration),
                    );
                }

                detection_has_payload = true;
                let canonical_result_args =
                    crate::agent::loop_::detector::canonicalise_args_string(args);
                tool_name.hash(&mut detection_fingerprint_hasher);
                canonical_result_args.hash(&mut detection_fingerprint_hasher);
                outcome.output.hash(&mut detection_fingerprint_hasher);
                let det_result = loop_state.record_per_tool_with_failure_canonical(
                    &tool_name,
                    &canonical_result_args,
                    &outcome.output,
                    !outcome.success,
                );
                match det_result {
                    crate::agent::loop_::detector::LoopDetectionResult::Ok => {}
                    crate::agent::loop_::detector::LoopDetectionResult::Warning(ref msg) => {
                        tracing::warn!(tool = %tool_name, %msg, "loop detector warning");

                        deferred_system_after_tool_batch.push(format!("[Loop Detection] {msg}"));
                    }
                    crate::agent::loop_::detector::LoopDetectionResult::Block(ref msg) => {
                        tracing::warn!(tool = %tool_name, %msg, "loop detector blocked tool call");

                        deferred_system_after_tool_batch.push(format!(
                            "[Loop Detection  -  BLOCKED] {msg}"
                        ));
                    }
                    crate::agent::loop_::detector::LoopDetectionResult::Break(msg) => {
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
                        if loop_recovery_used >= MAX_LOOP_RECOVERY {
                            _turn_metrics.mark_status("aborted");
                            return Ok(finalize_loop_recovery(
                                &msg,
                                history,
                                on_delta.as_ref(),
                            )
                            .await);
                        }
                        loop_recovery_used += 1;
                        deferred_system_after_tool_batch.push(loop_recovery_nudge(&msg));
                    }
                }
            }

            turn_tool_results.push((tool_name.clone(), outcome.success));
            let is_dedup = outcome.output.starts_with(DEDUP_RESULT_MARKER);
            batch_had_success |= outcome.success && !is_dedup;

            crate::agent::profile::runtime_hooks::publish_tool_event(
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
                let mut summary = svc.tool_use_summary.lock();
                summary.record(crate::services::tool_telemetry::use_summary::ToolInvocation {
                    tool_name: tool_name.clone(),
                    turn: iteration as u32,
                    duration_ms: outcome.duration.as_millis() as u64,
                    success: outcome.success,
                    input_tokens: 0,
                    output_tokens: 0,
                });
            }

            let safe_output =
                crate::services::governance::credential_vault::redact_for_audit_optional(&outcome.output);
            individual_results.push((tool_call_id, safe_output.clone()));
            let _ = writeln!(
                tool_results,
                "<tool_result name=\"{}\">\n{}\n</tool_result>",
                tool_name, safe_output
            );
        }

        if batch_had_success && !batch_edit_diagnostics_dirty {
            _pacing_gov.note_progress();
        }

        if plan_exec_nudge_state.inline_progress_reminder_due(iteration + 1) {
            plan_exec_nudge_state.inline_reminder_count += 1;
            let msg = crate::agent::plan_mode::execution_enforcement::inline_progress_reminder_message(
                &plan_exec_nudge_state,
            );
            deferred_system_after_tool_batch.push(msg);
        }


        if plan_finalized_this_iter {
            tracing::info!(
                target: "agent.plan_mode",
                turn_id = %turn_id,
                "Halting turn: exit_plan_mode succeeded; waiting for user's Build  - Switch click"
            );
            append_turn_records_to_history(
                history,
                &assistant_history_content,
                &native_tool_calls,
                &individual_results,
                use_native_tools,
                &tool_results,
            );
            let halt_text = "_Plan finalised. Waiting for the user to click \
                **Build** in the plan card to switch to Agent mode and start \
                executing._"
                .to_string();
            history.push(ChatMessage::assistant(&halt_text));
            _turn_metrics.mark_ok();
            if let Some(ref tx) = on_delta {
                let _ = tx.send(DraftEvent::Clear).await;
            }
            fire_post_turn_hooks(
                channel_name,
                hooks,
                response_cache_hook.as_ref(),
                experience_recorder_hook.as_ref(),
                memory_session_hook.as_ref(),
                response_cache_key.as_ref(),
                &user_msg_for_hooks,
                model,
                &halt_text,
                _pacing_gov.total_generated_tokens() as u32,
                &[],
                &[],
                false,
            )
            .await;
            return Ok(halt_text);
        }

        #[cfg(feature = "tool-curator")]
        if curator_finalized_this_iter {
            tracing::info!(
                target: "agent.curator_mode",
                turn_id = %turn_id,
                "Halting turn: exit_curator_mode succeeded; waiting for user's Build click"
            );
            append_turn_records_to_history(
                history,
                &assistant_history_content,
                &native_tool_calls,
                &individual_results,
                use_native_tools,
                &tool_results,
            );
            let halt_text = "_Curator deliverable saved. Waiting for the user to click \
                **Build** in the curator card to switch to Agent mode and execute \
                `impl_blueprint.md` verbatim._"
                .to_string();
            history.push(ChatMessage::assistant(&halt_text));
            _turn_metrics.mark_ok();
            if let Some(ref tx) = on_delta {
                let _ = tx.send(DraftEvent::Clear).await;
            }
            fire_post_turn_hooks(
                channel_name,
                hooks,
                response_cache_hook.as_ref(),
                experience_recorder_hook.as_ref(),
                memory_session_hook.as_ref(),
                response_cache_key.as_ref(),
                &user_msg_for_hooks,
                model,
                &halt_text,
                _pacing_gov.total_generated_tokens() as u32,
                &[],
                &[],
                false,
            )
            .await;
            return Ok(halt_text);
        }

        if awaiting_user_input {
            tracing::info!(
                target: "agent.plan_mode",
                turn_id = %turn_id,
                "Pausing turn: ask_question is awaiting user reply (plan nudge suppressed)"
            );
            append_turn_records_to_history(
                history,
                &assistant_history_content,
                &native_tool_calls,
                &individual_results,
                use_native_tools,
                &tool_results,
            );
            let pause_text =
                "_Waiting for the user's reply to the clarifying question(s) above._"
                    .to_string();

            history.push(ChatMessage::assistant(&pause_text));
            _turn_metrics.mark_ok();
            if let Some(ref tx) = on_delta {
                let _ = tx.send(DraftEvent::Clear).await;
                let _ = tx
                    .send(DraftEvent::Content(pause_text.clone()))
                    .await;
            }
            fire_post_turn_hooks(
                channel_name,
                hooks,
                response_cache_hook.as_ref(),
                experience_recorder_hook.as_ref(),
                memory_session_hook.as_ref(),
                response_cache_key.as_ref(),
                &user_msg_for_hooks,
                model,
                &pause_text,
                _pacing_gov.total_generated_tokens() as u32,
                &[],
                &[],
                false,
            )
            .await;
            return Ok(pause_text);
        }

        let loop_detection_active = match pacing.loop_detection_min_elapsed_secs {
            Some(min_secs) => loop_started_at.elapsed() >= Duration::from_secs(min_secs),
            None => true,
        };

        if loop_detection_active && detection_has_payload {
            let current_hash = detection_fingerprint_hasher.finish();
            if let Err(abort_msg) = loop_state.check_iteration_fingerprint(current_hash) {
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
                        "consecutive_identical": loop_state.consecutive_identical_outputs(),
                        "threshold": loop_state.identical_output_threshold(),
                    }),
                );
                let abort_text = abort_msg.to_string();
                if loop_recovery_used >= MAX_LOOP_RECOVERY {
                    _turn_metrics.mark_status("aborted");
                    return Ok(finalize_loop_recovery(
                        &abort_text,
                        history,
                        on_delta.as_ref(),
                    )
                    .await);
                }
                loop_recovery_used += 1;
                history.push(ChatMessage::system(loop_recovery_nudge(&abort_text)));
                continue;
            }
        }

        append_turn_records_to_history(
            history,
            &assistant_history_content,
            &native_tool_calls,
            &individual_results,
            use_native_tools,
            &tool_results,
        );

        for body in deferred_system_after_tool_batch {
            history.push(ChatMessage::system(body));
        }

        if cancellation_token
            .as_ref()
            .is_some_and(|t| t.is_cancelled())
        {
            return Err(tool_loop_cancelled());
        }

        {
            let mode = crate::agent::coding_mode::active_coding_mode();
            if let Some(msg) = crate::agent::mode::effects::post_tool_batch_message(mode) {
                history.push(ChatMessage::system(msg));
            }
        }

        let pair_break_mode = {
            let mode = crate::agent::coding_mode::active_coding_mode();
            mode.breaks_turn_after_tool_batch().then_some(mode)
        };
        if let Some(intercepted_mode) = pair_break_mode {
            let pair_text = "_Pair Checkpoint: tool batch complete. Pausing for your \
                input  -  type to continue or redirect, or press the input box to send \
                the next instruction._"
                .to_string();
            crate::agent::mode::effects::record_mode_intercept(
                crate::agent::mode::effects::ModeInterceptReason::PairCheckpoint,
                &crate::agent::mode::effects::ModeInterceptContext {
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
            fire_post_turn_hooks(
                channel_name,
                hooks,
                response_cache_hook.as_ref(),
                experience_recorder_hook.as_ref(),
                memory_session_hook.as_ref(),
                response_cache_key.as_ref(),
                &user_msg_for_hooks,
                model,
                &pair_text,
                _pacing_gov.total_generated_tokens() as u32,
                &[],
                &[],
                false,
            )
            .await;
            return Ok(pair_text);
        }
    }

    let exhausted_reason = match pacing_break_reason {
        Some(crate::agent::executor_core::PacingExceeded::TotalTimeout { .. }) => {
            "agent turn exceeded total time budget"
        }
        Some(crate::agent::executor_core::PacingExceeded::AbsoluteIterations { .. }) => {
            "agent turn reached the absolute per-turn iteration ceiling"
        }
        Some(crate::agent::executor_core::PacingExceeded::TokenBudget { .. }) => {
            "agent burned the no-progress token budget without a successful tool call"
        }
        Some(crate::agent::executor_core::PacingExceeded::IterationBudget { .. }) | None => {
            "agent made no forward progress for too many consecutive iterations"
        }
    };
    runtime_trace::record_event(
        "tool_loop_exhausted",
        Some(channel_name),
        Some(provider_name),
        Some(model),
        Some(&turn_id),
        Some(false),
        Some(exhausted_reason),
        serde_json::json!({
            "max_iterations": max_iterations,
            "no_progress_limit": pacing.no_progress_iteration_limit,
            "iterations_used": _pacing_gov.iteration(),
            "generated_tokens": _pacing_gov.total_generated_tokens(),
        }),
    );
    if crate::agent::plan_mode::execution_enforcement::should_auto_finalize_on_exit(
        &plan_exec_nudge_state,
    ) {
        let intent_window = build_intent_text_window("", history);
        let _ = auto_finalize_incomplete_plan_steps(
            tools_registry,
            tool_registry,
            on_delta.as_ref(),
            history,
            &intent_window,
        )
        .await;
    }
    emit_plan_progress_completion_card(
        tools_registry,
        tool_registry,
        on_delta.as_ref(),
        &plan_exec_nudge_state,
    )
    .await;
    let overflow_text = match pacing_break_reason {
        Some(crate::agent::executor_core::PacingExceeded::TotalTimeout { limit }) => format!(
            "This turn exceeded the total time limit ({}s), so it was stopped safely to avoid running indefinitely. Completed work has been kept; continue the conversation to build on the current progress.",
            limit.as_secs()
        ),
        Some(crate::agent::executor_core::PacingExceeded::AbsoluteIterations { limit }) => format!(
            "This turn reached the absolute per-turn iteration limit ({limit}), so it was stopped safely. Completed work has been kept; send any message to continue from the current progress."
        ),
        Some(crate::agent::executor_core::PacingExceeded::TokenBudget { used, limit }) => format!(
            "About {used} tokens were spent since the last progress (no-progress hard limit {limit}) without any successful tool call, so this turn was stopped safely to control cost. Completed work has been kept; continue the conversation to build on the current progress."
        ),
        Some(crate::agent::executor_core::PacingExceeded::IterationBudget { limit }) => format!(
            "No progress was made for {limit} consecutive iterations, so this turn was stopped safely to avoid spinning. Completed work has been kept; continue the conversation to build on the current progress."
        ),
        None => format!(
            "No progress was made across many consecutive iterations (no-progress limit {max_iterations}), so this turn was stopped safely to avoid spinning. Completed work has been kept; continue the conversation to build on the current progress."
        ),
    };
    _turn_metrics.mark_ok();
    history.push(ChatMessage::assistant(overflow_text.clone()));
    if let Some(ref tx) = on_delta {
        let _ = tx.send(DraftEvent::Content(overflow_text.clone())).await;
    }
    fire_post_turn_hooks(
        channel_name,
        hooks,
        response_cache_hook.as_ref(),
        experience_recorder_hook.as_ref(),
        memory_session_hook.as_ref(),
        response_cache_key.as_ref(),
        &user_msg_for_hooks,
        model,
        &overflow_text,
        _pacing_gov.total_generated_tokens() as u32,
        &[],
        &[],
        false,
    )
    .await;
    Ok(overflow_text)
}

fn render_evaluator_feedback(
    verdict: &crate::agent::self_assess::critic::CriticVerdict,
) -> String {
    let mut out = String::from("<evaluator_feedback>\n");
    out.push_str(
        "An independent reviewer (separate from you, with no access to your reasoning) evaluated \
         your last response and judged it does not yet meet the bar. Address the following \
         concretely, then finalize.\n",
    );
    if !verdict.rationale.trim().is_empty() {
        out.push_str("Reviewer summary: ");
        out.push_str(verdict.rationale.trim());
        out.push('\n');
    }
    if verdict.findings.is_empty() {
        out.push_str(
            "- The response was judged low quality; re-examine the task and improve correctness \
             and completeness.\n",
        );
    } else {
        for f in &verdict.findings {
            out.push_str(&format!("- [{}] {}\n", f.severity, f.message));
        }
    }
    out.push_str("</evaluator_feedback>");
    out
}

fn loop_recovery_nudge(reason: &str) -> String {
    let mut nudge = format!(
        "[Loop Recovery] {reason}\n\nYou are repeating the same unproductive action. Do NOT repeat \
         it verbatim. Take a fundamentally different approach; if you genuinely cannot make \
         progress, stop and clearly summarize what you have done and what is blocking you, then \
         wait for the user instead of looping."
    );
    if reason.contains("tool 'shell'") {
        nudge.push_str(
            "\n\nIf you are building or editing a file through the shell (echo, `>>`/`>` \
             redirection, `cat <<EOF` heredocs, or a helper script that emits the file), STOP \
             immediately: shell redirection mangles `<`, `>`, quotes and braces and makes no \
             progress. Write the file in ONE call with the `file_write` tool (it accepts arbitrary \
             content verbatim), or use `file_edit` to append/insert. Do not retry the shell \
             approach.",
        );
    }
    nudge
}

async fn finalize_loop_recovery(
    reason: &str,
    history: &mut Vec<ChatMessage>,
    on_delta: Option<&tokio::sync::mpsc::Sender<DraftEvent>>,
) -> String {
    let text = format!(
        "\u{26a0}\u{fe0f} Stopped a repeated ineffective action automatically: {reason}.\n\n\
         To avoid spinning, I did not keep forcing the same action. \
         Adjust the request, add missing details, or pick another direction and I will continue."
    );
    history.push(ChatMessage::assistant(text.clone()));
    if let Some(tx) = on_delta {
        let _ = tx.send(DraftEvent::Content(text.clone())).await;
    }
    text
}

pub(crate) fn build_tool_instructions(
    tools_registry: &[Box<dyn Tool>],
    tool_descriptions: Option<&ToolDescriptions>,
) -> String {
    let allowed = crate::agent::coding_mode::active_coding_mode().allowed_tools();
    build_tool_instructions_filtered(tools_registry, tool_descriptions, allowed.as_ref())
}

pub(crate) fn build_tool_instructions_filtered(
    tools_registry: &[Box<dyn Tool>],
    tool_descriptions: Option<&ToolDescriptions>,
    allowed: Option<&std::collections::HashSet<&'static str>>,
) -> String {
    let mut instructions = String::new();
    instructions.push_str("\n## Tool Use Protocol\n\n");
    instructions.push_str("To use a tool, wrap a JSON object in <tool_call></tool_call> tags:\n\n");
    instructions.push_str("```\n<tool_call>\n{\"name\": \"tool_name\", \"arguments\": {\"param\": \"value\"}}\n</tool_call>\n```\n\n");
    instructions.push_str(
        "CRITICAL: Output actual <tool_call> tags - never describe steps or give examples.\n\n",
    );
    instructions.push_str("Example: User says \"what's the date?\". You MUST respond with:\n<tool_call>\n{\"name\":\"shell\",\"arguments\":{\"command\":\"date\"}}\n</tool_call>\n\n");
    instructions.push_str("You may use multiple tool calls in a single response. ");
    instructions.push_str("After tool execution, results appear in <tool_result> tags. ");
    instructions
        .push_str("Continue reasoning with the results until you can give a final answer.\n\n");
    instructions.push_str("### Available Tools\n\n");

    for tool in tools_registry {
        if let Some(allow) = allowed
            && !allow.contains(tool.name())
        {
            continue;
        }
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

struct CodingModeRestoreGuard {
    previous: Option<crate::agent::coding_mode::CodingMode>,
}

impl CodingModeRestoreGuard {
    fn new(next: Option<crate::agent::coding_mode::CodingMode>) -> Self {
        let previous = match next {
            Some(ov) => match crate::services::try_get_services() {
                Some(svc) => {
                    let prev = *svc.coding_mode.read();
                    *svc.coding_mode.write() = ov;
                    Some(prev)
                }
                None => None,
            },
            None => None,
        };
        Self { previous }
    }
}

impl Drop for CodingModeRestoreGuard {
    fn drop(&mut self) {
        let Some(prev) = self.previous else {
            return;
        };
        if let Some(svc) = crate::services::try_get_services() {
            *svc.coding_mode.write() = prev;
        }
    }
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
    coding_mode_override: Option<crate::agent::coding_mode::CodingMode>,
) -> Result<String> {

    let observer: Arc<dyn Observer> = crate::agent::cli_runtime::build_observer(&config);
    let runtime: Arc<dyn runtime::RuntimeAdapter> =
        crate::agent::cli_runtime::build_runtime(&config)?;
    let security = crate::agent::cli_runtime::build_security(&config);

    let _ = crate::services::init_services(
        crate::services::container::ServiceContainerConfig::default(),
    );

    let _coding_mode_guard = CodingModeRestoreGuard::new(coding_mode_override);

    if let Some(svc) = crate::services::try_get_services() {
        svc.set_max_context_tokens(config.agent.max_context_tokens);
    }

    crate::event_bus::integration::init_global_bus(
        config
            .config_path
            .parent()
            .map(|p| p.join("event_audit.jsonl")),
    );

    let mem: Arc<dyn Memory> = {
        let memory_config = config.clone();
        tokio::task::spawn_blocking(move || {
            crate::agent::cli_runtime::build_memory(&memory_config)
        })
        .await
        .map_err(|e| anyhow::anyhow!("memory initialization task failed: {e}"))??
    };
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
    if let Ok(prefixes) = TOOL_DENY_PREFIXES.try_with(|p| p.clone()) {
        if !prefixes.is_empty() {
            let before = tools_registry.len();
            tools_registry.retain(|t| {
                let name = t.name();
                !prefixes
                    .iter()
                    .any(|prefix| name.starts_with(prefix.as_str()))
            });
            tracing::info!(
                denied_prefixes = prefixes.len(),
                before,
                retained = tools_registry.len(),
                "Applied tool deny-prefix filter"
            );
        }
    }

    let mut deferred_section = String::new();
    let mut activated_handle: Option<
        std::sync::Arc<parking_lot::Mutex<crate::tools::ActivatedToolSet>>,
    > = None;
    if config.mcp.enabled && !config.mcp.servers.is_empty() {
        tracing::info!(
            "Initializing MCP client  - {} server(s) configured",
            config.mcp.servers.len()
        );
        match crate::tools::McpRegistry::connect_all(&config.mcp.servers).await {
            Ok(registry) => {
                let registry = std::sync::Arc::new(registry);
                crate::tools::mcp::client::register_global_registry(std::sync::Arc::clone(
                    &registry,
                ));
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
                        crate::tools::mcp::deferred::build_deferred_tools_section(&deferred_set);
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

    let (builtin_deferred_enabled, _mcp_deferred_enabled) =
        crate::tools::deferred_loading_effective(&config);
    let deferred_builtin_set = if builtin_deferred_enabled {
        let workspace_key = crate::session::workspace_key_from_path(
            &config.workspace_dir,
            "default",
        );
        let allowlist = config.permissions.tool_allowlist.clone();
        let gate: Option<crate::security::permissions::ToolActivationGateHandle> = Some(
            std::sync::Arc::new(crate::security::permissions::CliStdinGate)
                as crate::security::permissions::ToolActivationGateHandle,
        );
        let options = crate::tools::BuiltinDeferredRegistrationOptions {
            workspace_key,
            allowlist,
            gate,
            config: Some(&config),
        };
        crate::tools::apply_builtin_deferred_registration_with_options(
            &mut tools_registry,
            &mut deferred_section,
            crate::tools::ToolSurfaceBaseline::Cli,
            &mut activated_handle,
            options,
        )
    } else {
        crate::tools::DeferredBuiltinToolSet::new()
    };
    if let Some(svc) = crate::services::try_get_services() {
        let names = deferred_builtin_set
            .stubs
            .iter()
            .map(|stub| stub.name.clone())
            .collect();
        svc.set_deferred_builtin_names(names);
    }
    if let (Some(handle), Some(svc)) = (
        activated_handle.as_ref(),
        crate::services::try_get_services(),
    ) {
        let workspace_key = crate::session::workspace_key_from_path(
            &config.workspace_dir,
            "default",
        );
        if let Ok(names) = svc.tool_activation_store.load(&workspace_key).await {
            if !names.is_empty() {
                let mut guard = handle.lock();
                for name in &names {
                    if guard.is_activated(name) {
                        continue;
                    }
                    if let Some(spec) = deferred_builtin_set.tool_spec(name) {
                        guard.activate_spec(name.clone(), spec);
                    }
                }
            }
        }
    }

    let mut provider_name = provider_override
        .as_deref()
        .or(config.default_provider.as_deref())
        .unwrap_or("openrouter")
        .to_string();
    let resolved_provider_name = providers::resolve_runtime_provider_name(&provider_name, &config);

    let mut model_name = match model_override
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(m) => m.to_string(),
        None => providers::resolve_default_model(&config)?,
    };

    let provider_runtime_options = providers::provider_runtime_options_from_config(&config);

    let mut provider: std::sync::Arc<dyn Provider> = std::sync::Arc::from(
        providers::create_routed_provider_with_options_async(
            resolved_provider_name.clone(),
            config.api_key.clone(),
            config.api_url.clone(),
            config.reliability.clone(),
            config.model_routes.clone(),
            model_name.clone(),
            provider_runtime_options.clone(),
        )
        .await?,
    );

    let _model_switch_callback = get_model_switch_state();

    crate::agent::flows::set_global_agent_handle(std::sync::Arc::new(
        crate::agent::flows::ProviderAgentHandle::new(
            std::sync::Arc::clone(&provider),
            model_name.clone(),
            config.default_temperature,
        ),
    ));

    let critic_eval_provider: Option<std::sync::Arc<dyn Provider>> = match config
        .self_eval
        .evaluator_model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(eval_model) => {
            match crate::tools::media::credentials::provider_for_model(&config, eval_model) {
                Some(eval_provider_id) if eval_provider_id != provider_name => {
                    let resolved = crate::tools::media::credentials::resolve(
                        &config,
                        Some(&eval_provider_id),
                        eval_model,
                    );
                    let eval_wire_name =
                        providers::resolve_runtime_provider_name(&eval_provider_id, &config);
                    match providers::create_resilient_provider_with_options_async(
                        eval_wire_name,
                        resolved.api_key.clone(),
                        Some(resolved.base_url.clone()),
                        config.reliability.clone(),
                        provider_runtime_options.clone(),
                    )
                    .await
                    {
                        Ok(p) => Some(std::sync::Arc::from(p)),
                        Err(e) => {
                            tracing::warn!(
                                provider = eval_provider_id.as_str(),
                                model = eval_model,
                                error = %e,
                                "failed to build dedicated evaluator provider; reusing main provider"
                            );
                            None
                        }
                    }
                }
                _ => None,
            }
        }
        None => None,
    };

    crate::agent::flows::set_global_critic_context(
        crate::agent::self_assess::critic::CriticContext::new(
            std::sync::Arc::clone(&provider),
            model_name.clone(),
            config.self_eval.clone(),
        )
        .with_eval_provider(critic_eval_provider),
    );

    observer.record_event(&ObserverEvent::AgentStart {
        provider: provider_name.to_string(),
        model: model_name.to_string(),
    });

    crate::agent::profile::runtime_hooks::publish_lifecycle_event("started");

    if crate::agent::multi_agent_runtime::global_runtime().is_none() {
        let _ = crate::agent::multi_agent_runtime::init_global_runtime();
    }

    if config.security.estop.enabled {
        if let Some(config_dir) = config.config_path.parent() {
            match crate::security::EstopManager::load(&config.security.estop, config_dir) {
                Ok(_) => {}
                Err(e) => tracing::warn!(
                    target: "security.estop",
                    error = %e,
                    "failed to hydrate persisted emergency-stop state at agent startup",
                ),
            }
        }
    } else {
        crate::security::estop::publish_runtime_state(crate::security::EstopState::default());
    }

    let _ = crate::cost::CostTracker::get_or_init_global(
        config.cost.clone(),
        &config.workspace_dir,
    );

    let hardware_rag: Option<crate::rag::HardwareRag> = if let Some(dir) = config
        .peripherals
        .datasheet_dir
        .as_ref()
        .filter(|d| !d.trim().is_empty())
    {
        let workspace_dir = config.workspace_dir.clone();
        let datasheet_dir = dir.trim().to_string();
        tokio::task::spawn_blocking(move || {
            crate::rag::HardwareRag::load(&workspace_dir, &datasheet_dir)
        })
        .await
        .ok()
        .and_then(Result::ok)
        .filter(|r: &crate::rag::HardwareRag| !r.is_empty())
    } else {
        None
    };
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

    let skills = {
        let wd = config.workspace_dir.clone();
        let cfg = config.clone();
        tokio::task::spawn_blocking(move || crate::skills::load_skills_with_config(&wd, &cfg))
            .await
            .unwrap_or_default()
    };

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
        "read_user_rule",
        "Load the full body of a user instruction rule by name. Use when: an entry in <available_user_rules> looks relevant and you need its complete content.",
    ));
    #[cfg(feature = "tool-cron")]
    {
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
    }
    #[cfg(feature = "tool-image")]
    {
        tool_descs.push((
            "screenshot",
            "Capture a screenshot of the current screen. Returns file path and base64-encoded PNG. Use when: visual verification, UI inspection, debugging displays.",
        ));
        tool_descs.push((
            "image_info",
            "Read image file metadata (format, dimensions, size) and optionally base64-encode it. Use when: inspecting images, preparing visual data for analysis.",
        ));
    }
    if config.browser.enabled {
        tool_descs.push((
            "browser_open",
            "Open approved HTTPS URLs in system browser (allowlist-only, no bulk content extraction)",
        ));
    }
    if config.composio.enabled {
        tool_descs.push((
            "composio",
            "Execute actions on 1000+ apps via Composio (Gmail, Notion, GitHub, Slack, etc.). Use action='list' to discover, 'execute' to run (optionally with connected_account_id), 'connect' to OAuth.",
        ));
    }
    #[cfg(feature = "tool-cron")]
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
    let coding_mode_label_owned =
        Some(crate::agent::coding_mode::active_coding_mode().label().to_string());
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

    {
        let mode = crate::agent::coding_mode::active_coding_mode();
        let mode_prompt = mode.system_prompt_injection();
        system_prompt.push_str(&mode_prompt);
    }

    let approval_manager = if interactive {
        let audit_path = config
            .config_path
            .parent()
            .map(|p| p.join("approval_audit.jsonl"));
        Some(ApprovalManager::for_surface(
            &config.autonomy,
            true,
            audit_path,
        ))
    } else {
        None
    };
    let channel_name = if interactive { "cli" } else { "daemon" };
    let stdout_is_user_facing = !crate::gateway::lifecycle::is_running();
    let memory_session_id = session_state_file
        .as_deref()
        .and_then(memory_session_id_from_state_file);

    let start = Instant::now();

    let mut final_output = String::new();

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

        let autosave_fut = async {
            if config.memory.auto_save
                && effective_msg.chars().count() >= AUTOSAVE_MIN_MESSAGE_CHARS
                && !memory::should_skip_autosave_content(&effective_msg)
            {
                let user_key = autosave_content_key("user_msg", &effective_msg);
                let _ = mem
                    .store(
                        &user_key,
                        &effective_msg,
                        MemoryCategory::Conversation,
                        memory_session_id.as_deref(),
                    )
                    .await;
            }
        };
        let recall_fut = build_context(
            mem.as_ref(),
            &effective_msg,
            config.memory.min_relevance_score,
            memory_session_id.as_deref(),
        );
        let expansion_fut = crate::agent::context::expansion::expand_input(
            &effective_msg,
            &config.workspace_dir,
            crate::context::builder::FocusPathRegistry::current(),
            String::new(),
        );
        let ((), mem_context, expanded_msg) =
            tokio::join!(autosave_fut, recall_fut, expansion_fut);
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
        let companion = build_cli_turn_companion(&effective_msg, &expanded_msg, &context);

        let mut history = vec![
            ChatMessage::system(&system_prompt),
            ChatMessage::user(&effective_msg).with_turn_companion(companion),
        ];

        if config.agent.history_pruning.enabled {
            let _stats = crate::agent::history::pruner::prune_history(
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

        let cli_hooks = crate::hooks::build_runner(&config, &config.workspace_dir);
        let model_switch_callback = get_model_switch_state();
        let response = scope_model_switch(async {
            let mut current_provider = std::sync::Arc::clone(&provider);
            let mut current_provider_name = provider_name.to_string();
            let mut current_model_name = model_name.to_string();

            loop {
                let policy = crate::agent::loop_::policy::PolicyBundle::cli(
                    current_provider.as_ref(),
                    &tools_registry,
                    observer.as_ref(),
                    &current_provider_name,
                    &current_model_name,
                    &config.multimodal,
                    &config.pacing,
                    &excluded_tools,
                    &config.agent.tool_call_dedup_exempt,
                )
                .with_temperature(effective_temperature)
                .with_silent(!stdout_is_user_facing)
                .with_approval(approval_manager.as_ref())
                .with_channel_name(channel_name)
                .with_max_iterations(config.agent.max_tool_iterations)
                .with_activated_tools(activated_handle.as_ref())
                .with_model_switch_callback(Some(model_switch_callback.clone()))
                .with_rbac(rbac_engine_ref, rbac_identity_ref)
                .with_plan_mode_flag(Some(&plan_mode_flag))
                .with_hooks(cli_hooks.as_deref())
                .with_tool_descriptions(Some(&i18n_descs));
                let result = crate::agent::loop_::unified::UnifiedLoop::new(policy)
                    .run(&mut history)
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
                            let resolved_new_provider =
                                providers::resolve_runtime_provider_name(
                                    &new_provider_name,
                                    &config,
                                );
                            match providers::create_routed_provider_with_options_async(
                                resolved_new_provider.clone(),
                                config.api_key.clone(),
                                config.api_url.clone(),
                                config.reliability.clone(),
                                config.model_routes.clone(),
                                new_model_name.clone(),
                                provider_runtime_options.clone(),
                            )
                            .await
                            {
                                Ok(new_provider) => {
                                    current_provider = std::sync::Arc::from(new_provider);
                                    current_provider_name = new_provider_name;
                                    let old_model_for_patch = current_model_name.clone();
                                    current_model_name = new_model_name;
                                    patch_history_runtime_model(
                                        &mut history,
                                        &old_model_for_patch,
                                        &current_model_name,
                                    );
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
        if stdout_is_user_facing {
            println!("{final_output}");
        }
        observer.record_event(&ObserverEvent::TurnComplete);
        return Ok(final_output);
    }

    if message.is_none() {
        if !crate::util::is_bare_mode() {
            println!("\u{1F9F5} SenWeaverCoding Interactive Mode");
            println!("Type /help for commands.\n");
        }
        let _cli = crate::channels::CliChannel::new();
        let _command_registry = crate::services::container::register_all_commands();

        let mut history = if let Some(path) = session_state_file.as_deref() {
            load_interactive_session_history_async(path, &system_prompt).await?
        } else {
            vec![ChatMessage::system(&system_prompt)]
        };

        loop {

            let prompt_prefix = {
                let model_hint = model_name.split('/').next_back().unwrap_or(&model_name);
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
                let mode_badge = {
                    let mode = crate::agent::coding_mode::active_coding_mode();
                    if mode != crate::agent::coding_mode::CodingMode::Vibe {
                        format!(" \x1b[1;33m[{}]\x1b[0m", mode.display_name())
                    } else {
                        String::new()
                    }
                };
                let vim_badge = if crate::commands::vim::is_vim_enabled() {
                    " \x1b[1;35m[VIM]\x1b[0m"
                } else {
                    ""
                };
                let voice_badge = if crate::util::get_runtime_var("SEN_VOICE").as_deref()
                    == Some("on")
                {
                    " \x1b[1;34m[VOICE]\x1b[0m"
                } else {
                    ""
                };
                format!(
                    "\x1b[1;36m{model_hint}{cost_hint}\x1b[0m{token_hint}{mode_badge}{vim_badge}{voice_badge} \x1b[1;32m>\x1b[0m "
                )
            };
            print!("{prompt_prefix}");
            let _ = std::io::stdout().flush();

            let read_result = tokio::task::spawn_blocking(|| {
                use std::io::BufRead;
                let mut full_input = String::new();
                loop {
                    let mut raw = Vec::new();
                    match std::io::stdin().lock().read_until(b'\n', &mut raw) {
                        Ok(0) => return Ok(None),
                        Ok(_) => {
                            let line = String::from_utf8_lossy(&raw);
                            if full_input.is_empty() && line.trim_end().ends_with('\\') {
                                full_input.push_str(line.trim_end().trim_end_matches('\\'));
                                full_input.push('\n');
                                continue;
                            }
                            full_input.push_str(&line);
                            return Ok(Some(full_input));
                        }
                        Err(e) => return Err(e),
                    }
                }
            })
            .await;

            let full_input = match read_result {
                Ok(Ok(Some(input))) => input,
                Ok(Ok(None)) => return Ok(final_output),
                Ok(Err(e)) => {
                    tracing::error!("Read error: {e}");
                    return Err(anyhow::anyhow!("{e}"));
                }
                Err(join_err) => {
                    tracing::error!("stdin read task failed: {join_err}");
                    return Err(anyhow::anyhow!("{join_err}"));
                }
            };

            let mut effective_input = full_input.trim().to_string();
            if effective_input.is_empty() {
                continue;
            }

            match crate::commands::dispatch::dispatch_slash_input(&effective_input).await {
                crate::commands::dispatch::SlashOutcome::NotCommand => {}
                crate::commands::dispatch::SlashOutcome::Quit => break,
                crate::commands::dispatch::SlashOutcome::Clear => {
                    history.retain(|m| m.role == "system");
                    print!("\x1b[2J\x1b[H");
                    let _ = std::io::stdout().flush();
                    continue;
                }
                crate::commands::dispatch::SlashOutcome::Handled { success, message } => {
                    if success {
                        println!("{message}");
                    } else {
                        eprintln!("\x1b[31m{message}\x1b[0m");
                    }
                    continue;
                }
                crate::commands::dispatch::SlashOutcome::Followup { message, prompt } => {
                    if let Some(msg) = message {
                        println!("{msg}");
                    }
                    effective_input = prompt;
                }
            }

            let thinking_level =
                crate::agent::thinking::resolve_thinking_level(None, None, &config.agent.thinking);
            let thinking_params = crate::agent::thinking::apply_thinking_level(thinking_level);
            let effective_temperature = crate::agent::thinking::clamp_temperature(
                temperature + thinking_params.temperature_adjustment,
            );

            let recall_fut = build_context(
                mem.as_ref(),
                &effective_input,
                config.memory.min_relevance_score,
                memory_session_id.as_deref(),
            );
            let expansion_fut = crate::agent::context::expansion::expand_input(
                &effective_input,
                &config.workspace_dir,
                crate::context::builder::FocusPathRegistry::current(),
                String::new(),
            );
            let (mem_context, expanded_input) = tokio::join!(recall_fut, expansion_fut);
            let hw_context = if !board_names.is_empty() {
                let rag_limit = if config.agent.compact_context { 2 } else { 5 };
                hardware_rag
                    .as_ref()
                    .map(|r| build_hardware_context(r, &effective_input, &board_names, rag_limit))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            let context = format!("{mem_context}{hw_context}");
            let companion =
                build_cli_turn_companion(&effective_input, &expanded_input, &context);

            if config.agent.history_pruning.enabled {
                let stats = crate::agent::history::pruner::prune_history(
                    &mut history,
                    &config.agent.history_pruning,
                );
                if stats.dropped_messages > 0 || stats.collapsed_pairs > 0 {
                    tracing::debug!(
                        target: "cli.history",
                        dropped = stats.dropped_messages,
                        collapsed = stats.collapsed_pairs,
                        "pruned interactive session history before turn"
                    );
                }
            }

            let history_len_before_turn = history.len();
            history.push(ChatMessage::user(&effective_input).with_turn_companion(companion));

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
                    const SPINNER: &[&str] = &[
                        "\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}",
                        "\u{2834}", "\u{2826}", "\u{2827}", "\u{2807}", "\u{280f}",
                    ];
                    let mut spinner_active = is_tty;
                    let content_was_streamed = content_was_streamed_clone;
                    let mut spinner_frame = 0usize;
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
                            Ok(None) => {
                                if spinner_active {
                                    let _ =
                                        write!(std::io::stderr(), "\r\x1b[2mThinking… done\x1b[0m");
                                    let _ = writeln!(std::io::stderr());
                                    let _ = std::io::stderr().flush();
                                }
                                break;
                            }
                            Err(_) => {
                                if spinner_active {
                                    spinner_frame = (spinner_frame + 1) % SPINNER.len();
                                    let _ = write!(
                                        std::io::stderr(),
                                        "\r\x1b[2m{} Thinking…\x1b[0m",
                                        SPINNER[spinner_frame]
                                    );
                                    let _ = std::io::stderr().flush();
                                }
                            }
                        }
                    }
                });

            let response = scope_model_switch(async {
            let model_switch_callback = get_model_switch_state();
            if let Some(bs) = crate::bootstrap::try_get_state() {
                if let Some(requested) = bs.read(|s| s.main_loop_model_override.clone()) {
                    match resolve_model_override_target(&requested, &config) {
                        Some((provider_override, target_model)) => {
                            let target_provider =
                                provider_override.unwrap_or_else(|| provider_name.clone());
                            if target_model != model_name || target_provider != provider_name {
                                *model_switch_callback.lock() =
                                    Some((target_provider, target_model));
                            }
                        }
                        None => {
                            eprintln!(
                                "\x1b[31mNo usable fast model configuration found: add a model_routes entry with hint=\"fast\", or set agent_runtime.fast_apply_model.\x1b[0m"
                            );
                            bs.write(|s| s.main_loop_model_override = None);
                        }
                    }
                }
            }
            let cli_hooks = crate::hooks::build_runner(&config, &config.workspace_dir);
            let turn_response = loop {
                let policy = crate::agent::loop_::policy::PolicyBundle::cli(
                    provider.as_ref(),
                    &tools_registry,
                    observer.as_ref(),
                    &provider_name,
                    &model_name,
                    &config.multimodal,
                    &config.pacing,
                    &excluded_tools,
                    &config.agent.tool_call_dedup_exempt,
                )
                .with_temperature(effective_temperature)
                .with_silent(true)
                .with_channel_name(channel_name)
                .with_max_iterations(config.agent.max_tool_iterations)
                .with_on_delta(Some(delta_tx.clone()))
                .with_activated_tools(activated_handle.as_ref())
                .with_model_switch_callback(Some(model_switch_callback.clone()))
                .with_rbac(rbac_engine_ref, rbac_identity_ref)
                .with_hooks(cli_hooks.as_deref())
                .with_tool_descriptions(Some(&i18n_descs));
                match crate::agent::loop_::unified::UnifiedLoop::new(policy)
                    .run(&mut history)
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
                            let resolved_new_provider =
                                providers::resolve_runtime_provider_name(&new_provider, &config);
                            provider = std::sync::Arc::from(
                                providers::create_routed_provider_with_options_async(
                                    resolved_new_provider.clone(),
                                    config.api_key.clone(),
                                    config.api_url.clone(),
                                    config.reliability.clone(),
                                    config.model_routes.clone(),
                                    new_model.clone(),
                                    provider_runtime_options.clone(),
                                )
                                .await?,
                            );
                            provider_name = new_provider;
                            let old_model_for_patch = model_name.clone();
                            model_name = new_model;
                            patch_history_runtime_model(
                                &mut history,
                                &old_model_for_patch,
                                &model_name,
                            );
                            clear_model_switch_request();
                            continue;
                        }
                        eprintln!(
                            "\x1b[1;31m✖ This turn's request failed: {e}\x1b[0m\n\x1b[2m  The input was not written to session history; resend it as-is or adjust and retry (network/rate-limit issues usually succeed after a short wait).\x1b[0m"
                        );
                        if history.len() > history_len_before_turn {
                            history.truncate(history_len_before_turn);
                        }
                        break String::new();
                    }
                }
            };
            Ok::<String, anyhow::Error>(turn_response)
            })
            .await?;

            drop(delta_tx);
            match tokio::time::timeout(
                std::time::Duration::from_secs(5),
                consumer_handle.into_inner(),
            )
            .await
            {
                Ok(_) => {}
                Err(_) => {
                    tracing::debug!(
                        "interactive CLI delta consumer did not drain within 5s; continuing"
                    );
                }
            }

            if !response.is_empty() {
                if !content_was_streamed.load(std::sync::atomic::Ordering::Relaxed) {
                    println!("{response}");
                }
                let already_in_history = history
                    .last()
                    .is_some_and(|m| m.role == "assistant" && m.content == response);
                if !already_in_history {
                    history.push(ChatMessage::assistant(&response));
                }
            }

            for msg in history.iter_mut() {
                msg.strip_ephemeral_context();
            }

            if let Some(path) = session_state_file.as_deref() {
                if let Err(e) = save_interactive_session_history_async(path, &history).await {
                    tracing::error!(
                        target: "cli.session",
                        path = %path.display(),
                        error = %e,
                        "failed to save interactive session history for this turn"
                    );
                    eprintln!(
                        "\x1b[33mwarning: failed to save session history ({e}); this turn may be lost on restart\x1b[0m"
                    );
                }
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

    crate::agent::profile::runtime_hooks::publish_lifecycle_event("stopped");

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
    let approval_manager = {
        let audit_path = config
            .config_path
            .parent()
            .map(|p| p.join("approval_audit.jsonl"));
        ApprovalManager::for_surface(&config.autonomy, false, audit_path)
    };
    let mem: Arc<dyn Memory> = Arc::from(
        memory::create_memory_with_storage_and_routes_async(
            config.memory.clone(),
            config.embedding_routes.clone(),
            Some(config.storage.provider.config.clone()),
            config.workspace_dir.clone(),
            config.api_key.clone(),
        )
        .await?,
    );

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
            "Initializing MCP client  - {} server(s) configured",
            config.mcp.servers.len()
        );
        match crate::tools::McpRegistry::connect_all(&config.mcp.servers).await {
            Ok(registry) => {
                let registry = std::sync::Arc::new(registry);
                crate::tools::mcp::client::register_global_registry(std::sync::Arc::clone(
                    &registry,
                ));
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
                        crate::tools::mcp::deferred::build_deferred_tools_section(&deferred_set);
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

    let (builtin_deferred_enabled_pm, _mcp_deferred_enabled_pm) =
        crate::tools::deferred_loading_effective(&config);
    let deferred_builtin_set_pm = if builtin_deferred_enabled_pm {
        let workspace_key = crate::session::workspace_key_from_path(
            &config.workspace_dir,
            "default",
        );
        let allowlist = config.permissions.tool_allowlist.clone();
        let gate: Option<crate::security::permissions::ToolActivationGateHandle> = Some(
            std::sync::Arc::new(crate::security::permissions::CliStdinGate)
                as crate::security::permissions::ToolActivationGateHandle,
        );
        let options = crate::tools::BuiltinDeferredRegistrationOptions {
            workspace_key,
            allowlist,
            gate,
            config: Some(&config),
        };
        crate::tools::apply_builtin_deferred_registration_with_options(
            &mut tools_registry,
            &mut deferred_section,
            crate::tools::ToolSurfaceBaseline::Cli,
            &mut activated_handle_pm,
            options,
        )
    } else {
        crate::tools::DeferredBuiltinToolSet::new()
    };
    if let Some(svc) = crate::services::try_get_services() {
        let names = deferred_builtin_set_pm
            .stubs
            .iter()
            .map(|stub| stub.name.clone())
            .collect();
        svc.set_deferred_builtin_names(names);
    }
    if let (Some(handle), Some(svc)) = (
        activated_handle_pm.as_ref(),
        crate::services::try_get_services(),
    ) {
        let workspace_key = crate::session::workspace_key_from_path(
            &config.workspace_dir,
            "default",
        );
        if let Ok(names) = svc.tool_activation_store.load(&workspace_key).await {
            if !names.is_empty() {
                let mut guard = handle.lock();
                for name in &names {
                    if guard.is_activated(name) {
                        continue;
                    }
                    if let Some(spec) = deferred_builtin_set_pm.tool_spec(name) {
                        guard.activate_spec(name.clone(), spec);
                    }
                }
            }
        }
    }

    let provider_name = config.default_provider.as_deref().unwrap_or("openrouter");
    let resolved_provider_name =
        providers::resolve_runtime_provider_name(provider_name, &config);
    let model_name = providers::resolve_default_model(&config)?;
    let provider_runtime_options = providers::provider_runtime_options_from_config(&config);
    let provider: std::sync::Arc<dyn Provider> = std::sync::Arc::from(
        providers::create_routed_provider_with_options_async(
            resolved_provider_name.clone(),
            config.api_key.clone(),
            config.api_url.clone(),
            config.reliability.clone(),
            config.model_routes.clone(),
            model_name.clone(),
            provider_runtime_options.clone(),
        )
        .await?,
    );

    let hardware_rag: Option<crate::rag::HardwareRag> = if let Some(dir) = config
        .peripherals
        .datasheet_dir
        .as_ref()
        .filter(|d| !d.trim().is_empty())
    {
        let workspace_dir = config.workspace_dir.clone();
        let datasheet_dir = dir.trim().to_string();
        tokio::task::spawn_blocking(move || {
            crate::rag::HardwareRag::load(&workspace_dir, &datasheet_dir)
        })
        .await
        .ok()
        .and_then(Result::ok)
        .filter(|r: &crate::rag::HardwareRag| !r.is_empty())
    } else {
        None
    };
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

    let skills = {
        let wd = config.workspace_dir.clone();
        let cfg = config.clone();
        tokio::task::spawn_blocking(move || crate::skills::load_skills_with_config(&wd, &cfg))
            .await
            .unwrap_or_default()
    };

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
    tool_descs.push((
        "read_user_rule",
        "Load the full body of a user instruction rule by name.",
    ));
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
    let coding_mode_label_owned =
        Some(crate::agent::coding_mode::active_coding_mode().label().to_string());
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
    let expanded_message = crate::agent::context::expansion::expand_input(
        &effective_message,
        &config.workspace_dir,
        crate::context::builder::FocusPathRegistry::current(),
        String::new(),
    )
    .await;
    let companion = build_cli_turn_companion(&effective_message, &expanded_message, &context);

    let mut history = vec![
        ChatMessage::system(&system_prompt),
        ChatMessage::user(&effective_message).with_turn_companion(companion),
    ];
    let mut excluded_tools = compute_excluded_mcp_tools(
        &tools_registry,
        &config.agent.tool_filter_groups,
        effective_msg_ref,
    );
    if config.autonomy.level != AutonomyLevel::Full {
        excluded_tools.extend(config.autonomy.non_cli_excluded_tools.iter().cloned());
    }

    let daemon_hooks = crate::hooks::build_runner(&config, &config.workspace_dir);
    let policy = crate::agent::loop_::policy::PolicyBundle::cli(
        provider.as_ref(),
        &tools_registry,
        observer.as_ref(),
        provider_name,
        &model_name,
        &config.multimodal,
        &config.pacing,
        &excluded_tools,
        &config.agent.tool_call_dedup_exempt,
    )
    .with_temperature(effective_temperature)
    .with_silent(true)
    .with_approval(Some(&approval_manager))
    .with_channel_name("daemon")
    .with_max_iterations(config.agent.max_tool_iterations)
    .with_activated_tools(activated_handle_pm.as_ref())
    .with_hooks(daemon_hooks.as_deref())
    .with_tool_descriptions(Some(&i18n_descs));
    crate::agent::loop_::unified::UnifiedLoop::new(policy)
        .run(&mut history)
        .await
}
