// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Mode-specific behavioural helpers shared by `run_tool_call_loop`
//! (canonical CLI/daemon path) and `Agent::turn_streamed` (GUI path).
//!
//! Without this module, the two loops would drift: the canonical loop
//! injects context-budget notes, mode-specific auto-verify nudges,
//! Pair-mode checkpoints, ContextEng impact-analysis reminders, and
//! honours `ModeApprovalPolicy::AutoApprove`, while `turn_streamed`
//! historically only handled the system-prompt injection and a
//! generic auto-verify message.
//!
//! The helpers below are pure functions over the read-only state
//! (mode + history + max-context) and return `Option<String>` system
//! messages. Callers decide where to push the message into their
//! history.

use super::coding_mode::{CodingMode, ModeApprovalPolicy, PostToolBehavior};
use crate::observability::runtime_trace;
use crate::providers::traits::ChatMessage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeInterceptReason {
    ReadOnlyPolicy,
    ToolNotAllowed,
    PairCheckpoint,
}

impl ModeInterceptReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnlyPolicy => "mode_read_only",
            Self::ToolNotAllowed => "mode_tool_not_allowed",
            Self::PairCheckpoint => "mode_pair_checkpoint",
        }
    }
}

pub struct ModeInterceptContext<'a> {
    pub mode: CodingMode,
    pub channel: Option<&'a str>,
    pub provider: Option<&'a str>,
    pub model: Option<&'a str>,
    pub turn_id: Option<&'a str>,
    pub tool: Option<&'a str>,
    pub tool_call_id: Option<&'a str>,
    pub iteration: Option<usize>,
    pub message: Option<&'a str>,
}

pub fn record_mode_intercept(reason: ModeInterceptReason, ctx: &ModeInterceptContext<'_>) {
    let mut payload = serde_json::Map::new();
    payload.insert(
        "reason".to_string(),
        serde_json::Value::String(reason.as_str().to_string()),
    );
    payload.insert(
        "mode".to_string(),
        serde_json::Value::String(ctx.mode.label().to_string()),
    );
    if let Some(tool) = ctx.tool {
        payload.insert(
            "tool".to_string(),
            serde_json::Value::String(tool.to_string()),
        );
    }
    if let Some(call_id) = ctx.tool_call_id {
        payload.insert(
            "tool_call_id".to_string(),
            serde_json::Value::String(call_id.to_string()),
        );
    }
    if let Some(iter) = ctx.iteration {
        payload.insert(
            "iteration".to_string(),
            serde_json::Value::Number(serde_json::Number::from(iter)),
        );
    }
    let success = matches!(reason, ModeInterceptReason::PairCheckpoint).then_some(true);
    runtime_trace::record_event(
        "mode_intercept",
        ctx.channel,
        ctx.provider,
        ctx.model,
        ctx.turn_id,
        success.or(Some(false)),
        ctx.message,
        serde_json::Value::Object(payload),
    );
}

pub fn mode_auto_approves(mode: CodingMode) -> bool {
    mode.approval_policy() == ModeApprovalPolicy::AutoApprove
}

pub fn mode_blocks_tool(mode: CodingMode, tool_name: &str) -> Option<String> {
    if mode.approval_policy() == ModeApprovalPolicy::ReadOnly
        && !crate::security::permissions::is_read_only_tool(tool_name)
    {
        return Some(format!(
            "Tool '{tool_name}' is blocked by ReadOnly approval policy in {} mode. \
             {} mode permits only read-only tools. Re-think the user request without \
             mutations, or ask the user to switch to a write-capable mode.",
            mode.label(),
            mode.label()
        ));
    }
    None
}

fn estimate_tokens_filtered(history: &[ChatMessage], is_system: bool) -> usize {
    history
        .iter()
        .filter(|m| (m.role == "system") == is_system)
        .map(|m| m.content.len().div_ceil(4) + 4)
        .sum()
}

fn extract_reminder_marker(msg: &str) -> Option<&str> {
    let trimmed = msg.trim_start();
    if !trimmed.starts_with('[') {
        return None;
    }
    let end = trimmed.find(']')?;
    Some(&trimmed[..=end])
}

pub fn replace_or_push_system_reminder(history: &mut Vec<ChatMessage>, msg: String) {
    if let Some(marker) = extract_reminder_marker(&msg) {
        let marker_owned = marker.to_string();
        history.retain(|m| {
            !(m.role == "system" && m.content.trim_start().starts_with(&marker_owned))
        });
    }
    history.push(ChatMessage::system(msg));
}

pub fn build_context_budget_message(
    mode: CodingMode,
    history: &[ChatMessage],
    max_context_tokens: usize,
) -> Option<String> {
    if !mode.injects_context_budget() {
        return None;
    }
    let sys_tokens = estimate_tokens_filtered(history, true);
    let hist_tokens = estimate_tokens_filtered(history, false);
    let total = sys_tokens + hist_tokens;
    let remaining = max_context_tokens.saturating_sub(total);
    let pct = if max_context_tokens > 0 {
        (remaining * 100) / max_context_tokens
    } else {
        0
    };
    let warning = if pct < 20 {
        " WARNING: Context budget low. Summarize or drop old context before proceeding."
    } else {
        ""
    };

    let mut read_files: Vec<String> = Vec::new();
    for msg in history.iter() {
        if let Some(idx) = msg.content.find("\"file_read\"") {
            if let Some(path_start) = msg.content[idx..].find("\"path\"") {
                let after = &msg.content[idx + path_start..];
                if let Some(val_start) = after.find('"') {
                    let rest = &after[val_start + 1..];
                    if let Some(val_end) = rest.find('"') {
                        let path = &rest[..val_end];
                        if !path.is_empty() && !read_files.contains(&path.to_string()) {
                            read_files.push(path.to_string());
                        }
                    }
                }
            }
        }
    }
    let files_note = if read_files.is_empty() {
        String::new()
    } else {
        format!(
            " Files already in context ({}): {}",
            read_files.len(),
            read_files.join(", ")
        )
    };
    Some(format!(
        "[Context Budget] System: ~{}k tokens. History: ~{}k tokens. \
         Remaining: ~{}k tokens ({pct}% free).{warning}{files_note}",
        sys_tokens / 1000,
        hist_tokens / 1000,
        remaining / 1000,
    ))
}

pub fn file_mod_auto_verify_nudge(mode: CodingMode) -> Option<&'static str> {
    if !mode.auto_verify_on_edit() {
        return None;
    }
    let msg = match mode {
        CodingMode::Tdd => {
            "[TDD Mode] File modified. You MUST now run the test suite \
             and report whether the relevant test passes or fails."
        }
        CodingMode::Debug => {
            "[Debug Mode] File modified. (1) Re-run the originally failing \
             command now to check the fix. (2) If the bug is web-facing, \
             re-run the browser repro: `browser` action=open → action=snapshot \
             → action=screenshot, then `find`/`is_visible`/`get_text` to \
             assert the symptom is gone, and compare against the pre-fix \
             screenshot. Do NOT declare the bug fixed without this evidence."
        }
        CodingMode::Agent => {
            "[Agent Mode] File modified. Verify this subtask: run the \
             relevant check/test command and confirm success before \
             proceeding to the next subtask."
        }
        CodingMode::Spec => {
            "[Spec Mode] File modified per plan step. Run the step's \
             verification command now to confirm it compiles and passes \
             before moving to the next step."
        }
        CodingMode::Mvai => {
            "[MVAI Mode] File modified. Validate the change against \
             the interface contract and run boundary tests to ensure \
             observable, deterministic behavior."
        }
        CodingMode::ContextEng => {
            "[Context Eng] File modified — precision strike. Now: \
             1) Re-read the changed file to update your context. \
             2) Run the relevant check/test to verify. \
             3) Check if downstream dependents need updates."
        }
        _ => {
            "[Auto-verify] File modified. Run the project's check/build \
             command (e.g. cargo check, npm run build) to verify."
        }
    };
    Some(msg)
}

pub fn pre_turn_reminder(mode: CodingMode) -> Option<&'static str> {
    match mode {
        CodingMode::Plan => Some(
            "[Plan-Mode Reminder] This turn MUST end with a tool call to \
             `exit_plan_mode(plan_content=\"...\")`. Do NOT exit with \
             free-form text. The `plan_content` body MUST follow the YAML \
             frontmatter + Markdown structure described in the system \
             prompt (## Overview / ## Steps / ## Verification / ## Risks). \
             A 1-todo plan for trivial tasks is acceptable — but the plan \
             document is mandatory.",
        ),
        CodingMode::Spec => Some(
            "[Spec Reminder] Each iteration MUST be REAL-TIME, ONE step at a time: \
             1) `update_plan(action=\"update\", step_id=<id>, status=\"in_progress\")` \
             BEFORE any work; 2) implement the step; 3) verify with the step's check \
             command (cargo check / npm test / etc.); 4) `update_plan(... \
             status=\"completed\", notes=\"verified: <evidence>\")` IMMEDIATELY after \
             the verification, NOT at the end of the turn; 5) finally \
             `update_plan(action=\"save\", plan_name=...)`. NEVER batch multiple \
             status updates back-to-back without doing the actual work in between — \
             the user's progress UI updates from each call.",
        ),
        CodingMode::Tdd => Some(
            "[TDD Reminder] STRICT Red-Green-Refactor: (1) write a FAILING test FIRST \
             and CONFIRM it fails by running the test command; (2) only then write \
             the minimal implementation to turn it GREEN, and run the test command \
             to confirm it passes; (3) refactor while keeping tests green. After \
             every `file_write`/`file_edit`/`patch_apply` you MUST run the test \
             command IMMEDIATELY in the same turn. Forbidden: writing implementation \
             code before a failing test exists.",
        ),
        CodingMode::Agent => Some(
            "[Agent Reminder] You auto-approve all tool calls — every action is real. \
             For each subtask: plan → execute → self-verify (run the relevant \
             check/test command and confirm success) BEFORE moving on. For tasks \
             touching 5+ files, run `code_to_spec(action=\"summarize\")` first to \
             build a spec map. For web-facing work, drive the embedded browser dock \
             via the `browser` tool — the user sees the dock live. \
             \n\n\
             [Plan Sync — CRITICAL] If a saved plan (.plan.md) is being executed this turn \
             (the user said \"continue / proceed / keep going\", or you handed off from \
             Plan-mode auto-run), the plan is the source of truth. You MUST call \
             `update_plan` in REAL TIME, ONE step at a time: \
             (1) `update_plan(action=\"update\", step_id=<id>, status=\"in_progress\")` \
                 BEFORE starting that step's real work; \
             (2) do the actual edits / shell / verification for THAT one step; \
             (3) `update_plan(action=\"update\", step_id=<id>, status=\"completed\", \
                 notes=\"verified: <evidence>\")` IMMEDIATELY after verification — \
                 NEVER batch status flips at the end of the run. The user's progress UI \
                 is fed by every single call; batching freezes the bar at 0/N then jumps \
                 to N/N at the end, which is exactly what they DON'T want. \
             If you loaded the plan from disk and the in-memory tracker is empty, fire \
             `update_plan(action=\"set\", steps=[…])` ONCE at the very start to seed it, \
             then proceed step-by-step. Use `skipped` (with a `notes` reason) for steps \
             that turn out unnecessary — never silently leave them `pending`.",
        ),
        CodingMode::Pair => Some(
            "[Pair Reminder] After every tool batch the runtime WILL pause and return \
             control to the user (real, hard break — not a soft prompt). Use the \
             assistant message BEFORE the pause to: (1) summarize what just changed, \
             (2) state the verification result (pass/fail/skipped), (3) propose the \
             next step in one sentence and ask if the user wants to proceed. Do NOT \
             schedule additional tool calls expecting them to run this turn.",
        ),
        CodingMode::Architect => Some(
            "[Architect Reminder] Before any cross-module edit: (1) run \
             `code_to_spec(action=\"analyze\", paths=[\"./src\"])` to map dependencies; \
             (2) propose the design in one paragraph; (3) only THEN call `glob_edit` / \
             `patch_apply` for batch changes (NOT one-by-one `file_edit` for each \
             callsite). After edits, run `incremental_optimize(action=\"report\", ...)` \
             to summarize impact. For web-facing architecture, validate end-to-end via \
             the embedded `browser` dock.",
        ),
        CodingMode::ContextEng => Some(
            "[Context Eng Reminder] STRICT four-phase: Explore → Map → Plan → Strike. \
             Forbidden: writing code before Explore + Map are complete. Each Strike \
             MUST be a precision edit to a SINGLE file; after the Strike the post-tool \
             ImpactAnalysis hook will require listing every downstream dependent and \
             confirming their tests still pass. Do NOT batch unrelated edits in one Strike.",
        ),
        CodingMode::Mvai => Some(
            "[MVAI Reminder] Interface-first: write/extend the public interface \
             (trait / abstract / typed contract) in a SEPARATE `file_write` BEFORE any \
             implementation `file_write`. Forbidden: implementation edits when the \
             interface for that contract has not been written or read this session. \
             After every implementation file_write, run boundary tests via `shell` / \
             `diagnostics` to confirm observable behaviour matches the interface.",
        ),
        CodingMode::Harness => Some(
            "[Harness Reminder] Engineering-grade pipeline, four phases — DO NOT skip: \
             (1) Spec: `code_to_spec(summarize|analyze|generate)` + `update_plan(set|save)`; \
             (2) Skill: `read_skill` then `skill_tool` / `skill_http` for the looked-up \
             skills; (3) Delegate: `agent_delegate` for parallel sub-tasks; \
             (4) Synthesis: `agent_summary` / `agent_compact` + `incremental_optimize(report)`. \
             You auto-approve; verify after each phase before moving on.",
        ),
        CodingMode::Vibe => Some(
            "[Vibe Reminder] Full autonomy — move fast, but: (1) verify after every batch \
             (cargo check / npm test / equivalent); (2) call `ask_question` for \
             irreversible architectural decisions instead of guessing; (3) never silently \
             skip a failing test or check.",
        ),
        _ => None,
    }
}

pub fn post_tool_batch_message(mode: CodingMode) -> Option<&'static str> {
    match mode.post_tool_behavior() {
        PostToolBehavior::Checkpoint => Some(
            "[Pair Checkpoint] Tools executed. Before continuing:\n\
             1. Summarize what just changed and why.\n\
             2. Verify the change worked (run tests/build if applicable).\n\
             3. Propose the next step and ask the user if they'd like to proceed.",
        ),
        PostToolBehavior::ImpactAnalysis => Some(
            "[Context Eng — Impact Analysis] Tools executed. Before proceeding:\n\
             1. List every file that was read or modified in this batch.\n\
             2. For each modified file, identify downstream dependents (imports/callers).\n\
             3. Confirm all affected tests still pass.\n\
             4. Update your context map — which files are now stale in history?",
        ),
        PostToolBehavior::PlanRefresh => Some(
            "[Spec Checkpoint] Tools executed. Before continuing:\n\
             1. Mark completed plan steps via `update_plan(action=\"set\", steps=[...])` so the \
             plan card reflects reality (every finished step `status=\"completed\"`).\n\
             2. If the next step changed, append it now via `update_plan` — do NOT improvise \
             off-plan work.\n\
             3. Persist the latest plan with `update_plan(action=\"save\", plan_name=\"<task>\")` \
             so the user sees the live progress.",
        ),
        _ => None,
    }
}
