// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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
    _history: &[ChatMessage],
    _max_context_tokens: usize,
) -> Option<String> {
    let _ = mode;
    None
}

pub fn is_file_mutation_tool(name: &str) -> bool {
    matches!(
        name,
        "file_write"
            | "file_edit"
            | "multi_edit"
            | "notebook_edit"
            | "patch_apply"
            | "glob_edit"
            | "code_xfile_refactor"
            | "lsp_rename"
            | "lsp_format"
            | "restore_file"
            | "copy_path"
            | "move_path"
            | "delete_path"
            | "create_directory"
    )
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
        CodingMode::Vibe => {
            "[Vibe Mode] File modified. Vibe is fast, NOT careless: run \
             the project's check command (cargo check / npm test / tsc \
             --noEmit) before moving on. Silently skipping a red verify \
             is a Vibe-mode bug, not a feature."
        }
        CodingMode::Architect => {
            "[Architect Mode] File modified. After a cross-module batch \
             (`glob_edit` / `patch_apply` / multi-file refactor) run \
             `cargo check` (or the equivalent) AND \
             `code_to_spec(action=\"analyze\", paths=[\"./src\"])` to \
             confirm the dependency graph still matches the design. \
             Quote the verify output before the next batch."
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
             code before a failing test exists. \
             External info (unfamiliar API / verbatim error string) → `web_search` \
             FIRST, then `web_fetch`; `browser` is UI-only, never a search tool.",
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
             that turn out unnecessary — never silently leave them `pending`.\n\n\
             [Curator Handoff — CRITICAL] If the user has just clicked Build on a Curator \
             document (the workspace contains a fresh `.senweavercoding/curators/<slug>/final.md` + \
             `impl_blueprint.md`, or the user said \"implement the curator doc / build it / \
             ship the report\"), `.senweavercoding/curators/<slug>/impl_blueprint.md` is the BINDING contract — \
             read it FIRST via `file_read`, then read `final.md` and the entries in \
             `sources.md` / `research_notes.md` for context. The implementation that lands \
             this turn MUST mirror impl_blueprint.md verbatim: every module, every \
             interface, every build & verification command stated in the blueprint must \
             be honoured. Do NOT silently substitute a different design. If the blueprint \
             is ambiguous or contradicts itself, surface that with `ask_question` BEFORE \
             writing code; do not improvise an alternate spec. The final deliverable is a \
             fully runnable engineering project that materialises the document.",
        ),
        CodingMode::Pair => Some(
            "[Pair Reminder] After every tool batch the runtime WILL pause and return \
             control to the user (real, hard break — not a soft prompt). Use the \
             assistant message BEFORE the pause to: (1) summarize what just changed, \
             (2) state the verification result (pass/fail/skipped), (3) propose the \
             next step in one sentence and ask if the user wants to proceed. Do NOT \
             schedule additional tool calls expecting them to run this turn. \
             External-fact questions → `web_search` FIRST (then `web_fetch`); cite \
             the URL in the next checkpoint summary. `browser` is UI-only, never a \
             search tool — the post-batch pause makes a wasted browser round trip \
             especially expensive.",
        ),
        CodingMode::Architect => Some(
            "[Architect Reminder] Before any cross-module edit: (1) run \
             `code_to_spec(action=\"analyze\", paths=[\"./src\"])` to map dependencies; \
             (2) propose the design in one paragraph; (3) only THEN call `glob_edit` / \
             `patch_apply` for batch changes (NOT one-by-one `file_edit` for each \
             callsite). After edits, run `incremental_optimize(action=\"report\", ...)` \
             to summarize impact. For web-facing architecture, validate end-to-end via \
             the embedded `browser` dock. \
             Architectural references (RFCs / changelogs / pattern catalogs / CVEs) → \
             `web_search` FIRST (then `web_fetch` on the canonical URL); quote the \
             cited URL in the design narrative. NEVER use `browser` to query a \
             search engine.",
        ),
        CodingMode::ContextEng => Some(
            "[Context Eng Reminder] STRICT four-phase: Explore → Map → Plan → Strike. \
             Forbidden: writing code before Explore + Map are complete. Each Strike \
             MUST be a precision edit to a SINGLE file; after the Strike the post-tool \
             ImpactAnalysis hook will require listing every downstream dependent and \
             confirming their tests still pass. Do NOT batch unrelated edits in one Strike. \
             The Explore phase is dual-track: local (`dir_list` / `code_search` / \
             `Read`) AND web (`web_search` then `web_fetch`) when the task touches \
             external APIs / specs. Web evidence is explore-only — it never enters \
             a Strike. NEVER use `browser` for search.",
        ),
        CodingMode::Mvai => Some(
            "[MVAI Reminder] Interface-first: write/extend the public interface \
             (trait / abstract / typed contract) in a SEPARATE `file_write` BEFORE any \
             implementation `file_write`. Forbidden: implementation edits when the \
             interface for that contract has not been written or read this session. \
             After every implementation file_write, run boundary tests via `shell` / \
             `diagnostics` to confirm observable behaviour matches the interface. \
             When the contract mirrors an external standard (OpenAPI / JSON-RPC / \
             stdlib trait / RFC), anchor it via `web_search` then `web_fetch` and \
             quote the canonical URL in the interface file's leading doc-comment. \
             `browser` is not in MVAI's allowlist — never try.",
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
             skip a failing test or check; (4) external info (library version / spec / \
             error string) → `web_search` FIRST, then `web_fetch`; `browser` is for \
             live UI only, not search.",
        ),
        CodingMode::Debug => Some(
            "[Debug Reminder] STRICT four-stage protocol — do NOT skip steps: \
             (1) Reproduce: run the failing command/test FIRST and quote its output \
                 verbatim BEFORE editing anything. \
             (2) Hypothesize: list at most 3 ranked hypotheses with rationale tied to \
                 the captured evidence. \
             (3) Isolate: add diagnostics (logging / asserts / `diagnostics` tool) for \
                 the top hypothesis BEFORE patching code; gather more evidence. \
             (4) Fix & Verify: apply ONE minimal change, re-run the original failing \
                 command, then run the project's full check / test command. \
             For web-facing bugs or QA automation, drive the embedded `browser` dock (open → \
             snapshot → action → screenshot) before AND after the fix and quote the \
             comparison. For structured QA runs, bootstrap with `debug_test_report` \
             action=start, record cases/findings, and finalize into report.md. \
             凭据仅可使用 ${cred.*} 占位符；密码与 token 绝不写入任何文本或报告。 \
             若用户已在内置浏览器中登录（提到「已登录 / cookies are set / I am already logged in」或直接给出 URL 无凭据），\
             先 `browser action=list_tabs` 枚举所有标签（含 owner=user 的用户标签），按 URL 选中目标后 \
             `browser action=attach_tab tab_id=<id>` 绑定为本轮默认目标；不要再索取凭据，也不要在用户标签上 \
             clear_storage。对全站覆盖请求，按同源 BFS（max depth=3, max pages=20），每页执行 snapshot → \
             assert console_clean → screenshot → network_errors → debug_test_report add_coverage_entry → \
             collect_links 入队后继 URL，最后 finalize 渲染「覆盖率」表。 \
             Forbidden in this turn: calling `file_edit` / `file_write` / `multi_edit` / \
             `patch_apply` / `glob_edit` / `code_xfile_refactor` BEFORE Stage-1 evidence \
             has been produced and quoted in the assistant message.",
        ),
        CodingMode::Ask => Some(
            "[Ask Reminder] Pure read-only Q&A — your single deliverable this turn is \
             a clear answer with citations. \
             (1) Cite real `path:line-range` references when explaining code; never \
                 paraphrase without locating the source. \
             (2) Prefer narrow lookups over reading whole files: `code_search`, \
                 `grep` / `content_search`, `code_outline`, `code_graph_query`, \
                 `lsp_symbols`, `glob_search`. \
             (3) If the user's intent is genuinely ambiguous, call `ask_question` \
                 (you DO have it in this mode) — bundle related clarifications into \
                 ONE call. \
             (4) Forbidden: any mutation tool (`file_write` / `file_edit` / \
                 `multi_edit` / `patch_apply` / `glob_edit` / `shell` / \
                 `git_operations`), and writing or saving any plan document. \
             (5) Stay in answering voice; do NOT propose execution steps as if they \
                 will run — Ask mode never executes. If the user clearly wants edits, \
                 explain what would change and suggest switching to Agent / Harness, \
                 but do not perform it.",
        ),
        CodingMode::Curator => Some(
            "[Curator Reminder] You are authoring a research-grade document, NOT code. \
             This turn MUST end with EITHER continued curator-only tool calls OR the \
             final `exit_curator_mode(slug=..., template=...)`. \
             \n\nEARLY-EXIT RULE (read this BEFORE planning more research): \
             If `sources.md` already has ≥ 5 distinct `[Sn]` entries AND `draft.md` is \
             fleshed out with real prose (not just an outline), your VERY NEXT action \
             this turn MUST be `exit_curator_mode` with the polished `final_content` \
             and `impl_blueprint` arguments. Do NOT spend another full turn deliberating \
             whether to add a 6th search round — write the full document NOW in one pass. \
             Long thinking with short output is a known failure mode; the cure is to \
             commit and emit the complete `final.md` in a single response.\n\n\
             HARD QUALITY GATES (only the minimum bar before exit is allowed; collect \
             more ONLY if these are not yet met):\n\
             - At least 5 distinct `web_search` calls covering different query angles \
               (vary keywords, language zh/en, category web/academic/code/cn/news, \
               time_range). Prefer `multi=true` (the default) so each call fans out \
               across 5-6 complementary engines automatically. ONE `curator_deep_collect` \
               call typically satisfies this in a single shot.\n\
             - At least 8 long-form web pages fetched via `web_fetch` (or via \
               `curator_deep_collect`, which combines search + top-N web_fetch in one \
               call — strongly preferred for first-pass collection).\n\
             - At least 1 `workspace_deep_search` over any local sources the intent \
               references (use it whenever the user mentions a workspace path).\n\
             - Every kept source recorded in `sources.md` with id `[Sn]`, title, URL, \
               `accessed_at` timestamp, and a one-line `takeaway`. `curator_collect` \
               and `curator_deep_collect` already do this — never invent `[Sn]` ids \
               by hand.\n\n\
             STRICT WORKFLOW (collapse phases together when the early-exit rule fires): \
             (1) Intent → `enter_curator_mode` if not already active. \
             (2) Web Collect (preferred entrypoint): \
                 `curator_deep_collect(query=..., max_sources=5, snippet_chars=2500)` \
                 — this runs multi-engine search + auto web_fetch on the top URLs \
                 and writes to research_notes.md + sources.md in one shot. Use \
                 `web_search` + `web_fetch` + `curator_collect` only for narrow \
                 follow-up drill-downs the deep collect pass missed. \
             (3) Local Collect (only if the intent references the workspace): \
                 `workspace_deep_search` then narrower `content_search` / \
                 `glob_search` / `file_read`; capture excerpts via \
                 `curator_collect(kind=\"note\", path=..., lines=..., excerpt=...)`. \
             (4) Write the COMPLETE `final.md` in ONE response — do NOT stop at an \
                 outline. Then write `impl_blueprint.md` describing the implementation \
                 contract precisely. Drafts are an internal artifact, not the goal. \
             (5) Cite every non-trivial claim as either `[Sn]` from `sources.md` or \
                 `path:lineStart-lineEnd` from the workspace. \
             (6) Call `exit_curator_mode` IMMEDIATELY after `final.md` and \
                 `impl_blueprint.md` are written; the user will switch to Agent mode \
                 next and the implementation must mirror the blueprint verbatim.\n\n\
             DELIVERABLE PIPELINE CONTRACT (executed atomically inside `exit_curator_mode`): \
             input validation → quality gate → evidence gate → write final.md → \
             write impl_blueprint.md → render final.docx with the chosen template → \
             verify the DOCX (size + ZIP magic) → emit Review-Panel file_edit events for \
             ALL three artifacts → flip mode flag and surface the Curator card. \
             Any failure short-circuits and returns `success=false` with a concrete \
             remediation hint — the Curator card is NEVER shown unless every artifact is \
             present and verified. NEVER pass `allow_docx_skip=true` unless the user has \
             EXPLICITLY accepted a Markdown-only deliverable; doing so produces a \
             degraded Curator card and is treated as an SLA violation.",
        ),
    }
}

pub fn pinned_test_target_reminder(mode: CodingMode) -> Option<String> {
    if mode != CodingMode::Debug {
        return None;
    }
    let session_id = crate::session::current_session_context()?.session_id;
    let tab_id = crate::tools::browser::current_test_target_tab(&session_id)?;
    Some(format!(
        "[Debug Test Target] User has pinned tab #{tab_id} as the QA target. \
         Drive ALL automated testing on tab_id={tab_id}: pass `tab_id={tab_id}` to every \
         `browser` action that supports it (snapshot/click/fill/get_text/screenshot/etc.) \
         and treat that tab's existing login/cookies/session as authoritative — DO NOT call \
         `browser action=clear_storage` against it, do NOT prompt the user for credentials, \
         and do NOT navigate it to an unrelated origin without explicit permission. \
         Use `browser action=get_test_target` to re-confirm the pin if you lose track. \
         The user's UI also displays a `Test Target` badge so they can verify."
    ))
}

pub fn prototype_ref_reminder(mode: CodingMode) -> Option<String> {
    if mode != CodingMode::Debug {
        return None;
    }
    let session_id = crate::session::current_session_context()?.session_id;
    let proto_tab = crate::tools::browser::current_prototype_ref_tab(&session_id)?;
    Some(format!(
        "[Prototype Reference] User has bound tab #{proto_tab} as the UI prototype reference. \
         This tab contains the target design from a prototype tool (Modao/Figma/Axure/etc.). \
         When testing UI implementation, you MUST take a screenshot of the prototype tab \
         (use `browser action=screenshot tab_id={proto_tab}`) and compare it against the \
         test target tab to verify layout, spacing, colors, typography, and interactions \
         match the prototype design. Report any deviations between the implementation and \
         the prototype as professional QA findings. This is SEPARATE from the test target — \
         the test target (Tab 绑定) is the page being tested, while this prototype reference \
         is the design spec to compare against."
    ))
}

pub fn web_research_active_reminder(
    _mode: CodingMode,
    web_search_enabled: bool,
    _web_fetch_enabled: bool,
) -> Option<&'static str> {
    if !web_search_enabled {
        return None;
    }
    Some(
        "[Web Research Routing] When the user asks about external facts (`今日热点 / 热点新闻 / \
         trending / what's new today / latest <X> / recent <Y> 2026 / hot search`), follow this \
         protocol strictly:\n\
         (1) ALWAYS call `web_search` FIRST with the user's intent verbatim. Use the appropriate \
             `category`:\n   \
             - News / hot topics / today's events  \u{2192} `category=\"news\"`\n   \
             - Tech blog posts / Chinese tech blogs \u{2192} `category=\"lifestyle\"` (CSDN / Juejin / SegmentFault)\n   \
             - Academic / paper / research          \u{2192} `category=\"academic\"`\n   \
             - Forum / Q&A / opinion                \u{2192} `category=\"forum\"`\n   \
             - Code / library / SDK                 \u{2192} `category=\"code\"`\n\
         (2) DO NOT directly `web_fetch` hot-search/news aggregator pages such as \
             `top.baidu.com`, `tophub.today`, `s.weibo.com`, `trends.google.com`, `news.qq.com`, \
             `news.sina.com.cn`, `news.163.com`, `toutiao.com`, `36kr.com`, `thepaper.cn`, \
             `news.ycombinator.com`. These return raw HTML and the runtime URL-guard will refuse \
             them when `web_search` has not been tried yet (the refusal carries the corrected \
             call).\n\
         (3) `web_search` does multi-engine concurrent fan-out and returns structured per-page \
             results with title + URL + snippet. ONLY after that, if you need the full article \
             body, call `web_fetch` on the specific result URL the user is interested in. ONE \
             call per URL \u{2014} do NOT spawn parallel `web_fetch` against every result.\n\
         (4) Cite the search engine name + URL alongside any factual claim in the assistant \
             reply.",
    )
}

pub fn web_research_disabled_reminder(
    _mode: CodingMode,
    web_search_enabled: bool,
    web_fetch_enabled: bool,
) -> Option<&'static str> {
    if web_search_enabled && web_fetch_enabled {
        return None;
    }
    match (web_search_enabled, web_fetch_enabled) {
        (false, false) => Some(
            "[Web Research] DISABLED — both `web_search` and `web_fetch` are turned OFF in \
             Settings -> Tools & MCPs -> Web Research. Do NOT call those tools or pretend to \
             fetch URLs. Answer using only local context, and if external facts are required, \
             tell the user that web research is currently disabled and ask them to enable it.",
        ),
        (false, true) => Some(
            "[Web Research] `web_search` is DISABLED in Settings -> Tools & MCPs -> Web Research. \
             Do NOT call `web_search`. You may still use `web_fetch` on URLs the user supplied or \
             you already have, but you cannot discover new URLs this turn.",
        ),
        (true, false) => Some(
            "[Web Research] `web_fetch` is DISABLED in Settings -> Tools & MCPs -> Web Research. \
             You may run `web_search` to find candidate URLs, but you CANNOT call `web_fetch` to \
             read their content this turn — quote the snippet that came back from search instead.",
        ),
        (true, true) => None,
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
        PostToolBehavior::HarnessGate => Some(
            "[Harness Gate] Tools executed. Before the next layer:\n\
             1. State which Harness layer just completed (Spec / Skill / Session / \
             Multi-Agent / Capability / Trellis).\n\
             2. Quote the verification command output (cargo check / clippy / test / \
             tsc / lint) verbatim — no \"looks fine\" claims.\n\
             3. If a phase boundary was crossed, update STATE.md / ROADMAP.md / \
             TASKS.md (or `.opencode/plans/*.md`) so the structured artifacts stay \
             in sync with reality.\n\
             4. Persist key decisions via `memory_store` and synthesize via \
             `incremental_optimize(action=\"report\", description=\"…\")` before \
             advancing.\n\
             5. If verification failed, debug it now — never advance with a broken \
             check.",
        ),
        _ => None,
    }
}
