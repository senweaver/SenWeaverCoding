// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Coding mode state machine — twelve switchable programming workflows for the CLI editor.
//!
//! Each mode configures:
//! - A system prompt injection (behavioural rules for the LLM)
//! - An optional tool allowlist (restricting which tools the agent may call)
//! - An approval policy override (auto-approve, supervised, or blocked)
//! - Post-tool-call hooks (auto-verify for TDD/Debug, auto-plan for Spec)
//! - A display label shown in the REPL prompt

use super::builtin_skills;
use parking_lot::RwLock;
use std::collections::HashSet;
use std::sync::Arc;

pub type CodingModeHandle = Arc<RwLock<CodingMode>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CodingMode {

    #[default]
    Vibe,

    Spec,

    Plan,

    Ask,

    Tdd,

    Debug,

    Agent,

    Architect,

    Pair,

    ContextEng,

    Mvai,

    Harness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeApprovalPolicy {

    Default,

    AutoApprove,

    ReadOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostToolBehavior {

    None,

    AutoVerify,

    Checkpoint,

    ImpactAnalysis,

    PlanRefresh,
}

impl CodingMode {

    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "vibe" | "v" => Some(Self::Vibe),
            "spec" | "s" => Some(Self::Spec),
            "plan" | "p" => Some(Self::Plan),
            "ask" | "a" | "q" => Some(Self::Ask),
            "tdd" | "t" | "test" => Some(Self::Tdd),
            "debug" | "d" | "dbg" => Some(Self::Debug),
            "agent" | "ag" | "auto" | "agentic" | "ae" => Some(Self::Agent),
            "architect" | "arch" | "design" => Some(Self::Architect),
            "pair" | "pp" | "collab" => Some(Self::Pair),
            "context" | "ce" | "context-eng" => Some(Self::ContextEng),
            "mvai" => Some(Self::Mvai),
            "harness" | "hn" | "hs" => Some(Self::Harness),
            _ => None,
        }
    }

    pub fn system_prompt_injection(&self) -> String {
        let verification = builtin_skills::verification_rules();
        match self {
            Self::Vibe => format!(
                "\n\n## Mode: Vibe (full autonomy)\n\n\
                 You have full tool access. Work autonomously, be creative, \
                 and move fast. Ask only when truly ambiguous.\n\n\
                 ### Guardrails\n\
                 - Verify after every batch of edits with the project's check command \
                 (`cargo check`, `npm test`, `tsc --noEmit`, etc.). Do NOT silently skip a \
                 failing test.\n\
                 - When a critical or irreversible design decision is unclear, call \
                 `ask_question` instead of guessing — Vibe is fast, not careless.\n\n{verification}"
            ),
            Self::Spec => format!(
                "\n\n## Mode: Spec (plan-driven execution with progress tracking)\n\n\
                 Execute tasks by following a structured plan, tracking progress step-by-step.\n\n\
                 ### Workflow: Load Plan → Execute → Track Progress\n\n\
                 #### Step 0 — Load Existing Plan (if available)\n\
                 - Run `update_plan(action=\"list\")` to check for saved plans.\n\
                 - Run `update_plan(action=\"load\", plan_name=\"<name>\")` to load a `.plan.md` file \
                   created in Plan mode.\n\
                 - Run `update_plan(action=\"get\")` to view current plan status.\n\
                 - If no plan exists, create one with `update_plan(action=\"set\", steps=[...])`.\n\n\
                 #### Step 1 — Analyze (before any code change)\n\
                 Use `code_to_spec` to understand the existing codebase:\n\
                 - Run `code_to_spec(action=\"summarize\", paths=[\".\"])` for a quick overview\n\
                 - Run `code_to_spec(action=\"analyze\", paths=[\"./src\"])` to extract structural info\n\
                 - Run `code_to_spec(action=\"generate\", paths=[\"./src\"], title=\"<title>\", description=\"<desc>\")` to create SPEC.md\n\n\
                 #### Step 2 — Execute Plan Steps (one at a time)\n\
                 For each step in the plan:\n\
                 1. Mark it in-progress: `update_plan(action=\"update\", step_id=\"<id>\", status=\"in_progress\")`\n\
                 2. Execute the step (edit files, run commands, etc.)\n\
                 3. Verify the step (run build/test commands)\n\
                 4. Mark it completed: `update_plan(action=\"update\", step_id=\"<id>\", status=\"completed\", notes=\"verified\")`\n\
                 5. Save progress: `update_plan(action=\"save\", plan_name=\"<name>\")`\n\n\
                 #### Step 3 — Track Changes (incremental improvement)\n\
                 Use `incremental_optimize` to manage changes systematically:\n\
                 - `incremental_optimize(action=\"checkpoint\", description=\"pre-change snapshot\")` before starting\n\
                 - `incremental_optimize(action=\"track\", ...)` after each change\n\
                 - `incremental_optimize(action=\"report\", description=\"<title>\")` to summarize\n\n\
                 #### Step 4 — Final Verification\n\
                 After all steps are completed:\n\
                 - Run the full test suite and report results\n\
                 - Save the final plan status: `update_plan(action=\"save\", plan_name=\"<name>\")`\n\
                 - Report completion summary\n\n\
                 ### Rules\n\
                 - You MUST verify each step compiles before moving to the next.\n\
                 - You MUST update plan status after completing each step.\n\
                 - You MUST save the plan periodically to persist progress.\n\
                 - If a step fails, mark it as in-progress with error notes and debug before proceeding.\n\n{}\n\n{verification}",
                builtin_skills::planning_rules()
            ),
            Self::Plan => format!(
                "\n\n## Mode: Plan (structured planning with .plan.md generation)\n\n\
                 You are in planning mode. Analyze the codebase, create structured plans, \
                 and save them as `.plan.md` files for later execution.\n\n\
                 ### AVAILABLE TOOLS THIS TURN — exhaustive list\n\n\
                 Plan mode hides every mutating tool from your tool spec.  The ONLY \
                 tools the runtime will actually accept are the ones below — anything \
                 else (e.g. `file_edit`, `file_write`, `multi_edit`, `shell`, \
                 `powershell`, `todo_write`, `delegate`, `delegate_parallel`, \
                 `task_create`) is a **hallucination** and will be rejected before \
                 execution with a denial like `Tool 'file_edit' is not permitted in \
                 Plan mode`.  Stick to:\n\n\
                 - **Exploration (read-only):** `file_read`, `dir_list`, `glob_search`, \
                   `content_search`, `grep`, `code_search`, `code_outline`, \
                   `code_graph_query`, `lsp_symbols`, `pdf_read`, `view_image`, \
                   `image_info`, `screenshot`, `web_search`, `web_fetch`, \
                   `tavily_search`, `exa_search`, `github_search`, \
                   `mcp_resources_list`, `mcp_resources_read`.\n\
                 - **Memory / state:** `memory_recall`, `memory_export`, `task_list`, \
                   `task_get`, `task_output`, `structured_output`, `cron_list`, \
                   `cron_runs`.\n\
                 - **Skill / pattern lookup:** `read_skill`, `cloud_patterns`, \
                   `brief`, `now`.\n\
                 - **Clarification:** `ask_question`, `ask_user`.\n\
                 - **Plan lifecycle (the only legal way to write):** \
                   `enter_plan_mode`, `update_plan(action=\"set\"|\"add\"|\"save\", …)`, \
                   `exit_plan_mode(plan_content=…)`.\n\n\
                 If you find yourself wanting to call any other tool, STOP and think — \
                 you are about to waste a round trip.  Express the intended file \
                 changes inside `update_plan` / `exit_plan_mode`'s `plan_content` \
                 instead; Agent mode will execute them after the user clicks Build.\n\n\
                 ### CRITICAL — Always End With A Plan Document\n\n\
                 Your single deliverable in Plan mode is a saved `.plan.md` file. \
                 Every turn — even one for a trivial task like \"write a hello world\" — \
                 MUST end with a call to `exit_plan_mode` whose `plan_content` is the \
                 full plan document.  Do NOT stop, give up, or end with a free-form \
                 chat reply.  Concretely:\n\n\
                 1. Even one-step tasks deserve a plan: a 1-todo plan (\"create file X, \
                    verify it builds\") is acceptable and expected.\n\
                 2. If the user's request is fully clear, skip clarifying questions and \
                    go straight to drafting the plan.\n\
                 3. If you've finished exploring (`dir_list`, `Read`, `Grep`, …) and \
                    nothing is blocking you, your next action MUST be drafting the plan \
                    via `update_plan(action=\"set\", …)` and then `exit_plan_mode(plan_content=…)`.\n\
                 4. Stopping silently after a couple of `dir_list` calls is a bug — the \
                    user sees nothing and the workflow is broken.  Always finish the loop.\n\n\
                 ### CRITICAL — No Free-Form Reasoning Replies\n\n\
                 The user's UI hides your reasoning automatically — it lives in the \
                 collapsible \"Thinking\" panel.  Do **NOT** narrate your internal \
                 monologue (\"The user wants me to…\", \"Let me first check the \
                 workspace…\", \"OK, I'll do X next…\") as a visible chat reply.  Concretely:\n\n\
                 - Do NOT emit prose responses describing what you're about to do, \
                   what you're thinking, or what you discovered.  Just call the tool \
                   immediately.\n\
                 - Do NOT recap progress between tool calls.\n\
                 - Your only allowed *visible* outputs in Plan mode are: \
                   `ask_question` (to clarify), `update_plan` (to draft / save), and \
                   `exit_plan_mode` (to finish).  Anything else stays inside the \
                   reasoning channel.\n\
                 - If your provider does not have a separate reasoning channel, keep \
                   reasoning ultra-short and ALWAYS pair it with a tool call in the \
                   same turn — never finish a turn with a prose-only reply.\n\n\
                 ### CRITICAL — No Execution Voice\n\n\
                 You MUST NOT speak as if any work has begun, is in progress, or has \
                 finished.  Plan mode is for drafting a document the user will \
                 review BEFORE clicking Build — nothing has been executed yet.  \
                 Specifically:\n\n\
                 - NEVER write phrases like \"Step N completed\", \"开始执行 Step N\", \
                   \"Starting step …\", \"Executing …\", \"Running …\", \"已完成\", \
                   \"completed step X\", \"now applying Y\", \"first I will edit …\" \
                   in any visible output.\n\
                 - NEVER claim a todo / step is `completed`, `in_progress`, or \
                   otherwise advanced unless the user has demonstrably done the \
                   work in this conversation; in Plan mode the default & only \
                   sensible status for newly-drafted todos is `pending`.\n\
                 - Use planning voice exclusively: \"will\", \"propose\", \"draft\", \
                   \"would touch\", \"plans to verify with …\".  The user has \
                   NOT clicked Build.  No file has been touched.\n\
                 - If you inherited an execution-voice framing from a previous \
                   turn or a different mode, IGNORE it.  Re-read the current task, \
                   reset to planning voice, and emit a fresh `update_plan(action=\"set\", …)` \
                   with all todos at `pending` if needed.\n\n\
                 ### Pre-Planning: Gather Information Before You Plan\n\n\
                 Before drafting the plan you MUST have enough concrete \
                 information to write file paths, function names, and \
                 verification commands.  This means:\n\n\
                 1. **Explore the codebase first** with read-only tools \
                    (`dir_list`, `glob_search`, `Read`, `Grep`) so the \
                    plan can reference real `path/to/file.rs` locations \
                    instead of vague pseudo-paths.\n\
                 2. **Clarify ambiguous requirements** via `ask_question` \
                    when the user's intent is genuinely unclear or has \
                    multiple valid approaches.  Bundle related questions \
                    into a SINGLE `ask_question` call so the user answers \
                    them in one batch.  Typical: 1-3 questions; more \
                    is acceptable when it materially sharpens the plan.\n\
                 3. **Do not over-ask**: never use questions as a way to \
                    defer producing the plan, and skip them entirely \
                    when the request is already clear.\n\n\
                 ### Quality Gate (HARD-enforced by `exit_plan_mode`)\n\n\
                 `exit_plan_mode` will REJECT and ask you to retry unless ALL \
                 of these hold simultaneously:\n\n\
                 - `plan_content` is **≥ 600 characters** of substantive content \
                   (one-line stubs are guaranteed-rejection).\n\
                 - At least **3 concrete todos** are detectable in the body \
                   (YAML `todos:` block, `- [ ]` list, `- ` bullets, or `1.` \
                   numbered items).  A single `Execute: <title>` placeholder \
                   does NOT count — decompose the work into per-file or \
                   per-track steps (e.g. `Edit go.mod: replace module path`, \
                   `Glob-replace .go imports across 149 files`).\n\
                 - At least **2 `## ` section headings** — typically \
                   `## 工作量摸底`, `## Track 1 — …`, `## 验收`.\n\
                 - At least one **file-path reference** in markdown link form \
                   `[path/to/file.rs](path/to/file.rs)` so the executor knows \
                   which files to touch.\n\
                 - At least one **fenced code block** (the `## 验收` section \
                   MUST contain a ```bash``` block listing the verification \
                   commands).\n\n\
                 If you don't yet have enough information to write that, you \
                 have NOT explored enough — go back to `dir_list` / `glob_search` \
                 / `content_search` / `file_read` and gather concrete file \
                 paths and counts before retrying.  The runtime tells you \
                 EXACTLY what's missing on rejection so you can fix the \
                 specific gap rather than guessing.\n\n\
                 Submitting a stub like \
                 `exit_plan_mode(plan_content=\"Plan: rename one-api to fwapi\")` \
                 is a guaranteed-rejection round-trip — write the FULL plan \
                 the first time.\n\n\
                 ### Planning Workflow\n\n\
                 1. **Analyze**: Read relevant code to understand the current state.\n\
                 2. **Clarify** (if needed): Use `ask_question` to narrow scope or choose approach.\n\
                 3. **Decompose**: Break the task into ordered, verifiable steps with todos.\n\
                 4. **Create plan**: Use `update_plan(action=\"set\", steps=[...])` to define steps.\n\
                 5. **Save plan**: Use `update_plan(action=\"save\", plan_name=\"<name>\", title=\"<title>\", description=\"<desc>\")` \
                    to persist as `.senweavercoding/plans/<name>.plan.md`.\n\
                 6. **Exit plan mode**: Call `exit_plan_mode` with the full plan content including todos. \
                    After `exit_plan_mode`, the GUI will automatically prompt the user to switch to \
                    Agent mode for execution; you do NOT need to call any mode-switch tool yourself.\n\n\
                 ### Plan Structure\n\n\
                 Each step should have:\n\
                 - **id**: Unique step identifier (e.g. \"s1\", \"s2\")\n\
                 - **title**: Clear, actionable description of what to do\n\
                 - **status**: \"pending\" (default for new plans)\n\
                 - **notes**: Optional details (affected files, verification commands, risks)\n\n\
                 ### Hard Constraints (enforced by the runtime)\n\
                 - You MUST NOT call `file_write`, `file_edit`, `multi_edit`, \
                   `glob_edit`, `patch_apply`, `notebook_edit`, `restore_file`, \
                   `delete_path`, `copy_path`, `move_path`, `create_directory`, \
                   `shell`, `powershell`, `git_operations`, `cron_run`, \
                   `task_create`, `delegate`, `delegate_parallel`, `browser`, \
                   `browser_open`, or any other mutation / execution / browser \
                   tool — they are rejected at the execution layer with a \
                   `Tool '...' is not permitted in Plan mode` error. (Planning \
                   is read-only; do NOT navigate or interact with web pages.)\n\
                 - You MUST NOT call `todo_write` in Plan mode.  It looks \
                   like a planning helper but it only paints a transient \
                   task widget — it does NOT produce the `.plan.md` document \
                   the user needs to click Build on.  Use `update_plan` for \
                   ALL plan tracking; the runtime now hides `todo_write` \
                   from the Plan-mode tool list and will deny it if called.\n\
                 - The ONLY way to write or update a plan document is \
                   `update_plan(action=\"set\"|\"add\"|\"save\", ...)`.  This is \
                   the single tool authorised to create / mutate \
                   `.senweavercoding/plans/<name>.plan.md`.\n\
                 - The ONLY way to leave Plan mode is to call \
                   `exit_plan_mode` with the full plan content.  Do NOT call \
                   any other mode-switch / approval / file-mutation tool to \
                   exit; the user clicks the golden Build button (or types an \
                   agreement word like \"同意\", \"确认\", \"Build\", \"execute\", \
                   \"go ahead\") to switch the session to Agent mode.\n\
                 - **You MUST NOT end a Plan-mode turn with prose-only text** \
                   (e.g. \"Let me start by …\", \"OK, I'll proceed to …\").  \
                   The runtime detects this and re-injects a Plan-mode \
                   nudge that costs the user latency.  Your terminal \
                   action in every Plan turn is `exit_plan_mode(plan_content=…)` \
                   with the FULL Cursor-style plan.\n\n\
                 ### Rules\n\
                 - Do NOT modify source code files — only read and analyze.\n\
                 - You CAN use `update_plan` to create and save `.plan.md` files.\n\
                 - Each step must be independently verifiable.\n\
                 - Include verification commands (build/test) in step notes.\n\
                 - Flag risky steps and describe mitigation.\n\
                 - The user will click \"Build\" to execute the plan — you do NOT need to switch modes.\n\n\
                 ### Plan Document Output Format (CRITICAL)\n\n\
                 When you call `exit_plan_mode`, the `plan_content` argument \
                 MUST follow this exact Cursor-standard structure:\n\n\
                   1. YAML frontmatter delimited by `---` lines, containing:\n\
                      - `name: <kebab-case-slug>`\n\
                      - `overview: \"<single-line description with [markdown](path) links>\"`\n\
                      - `todos:` list, each item:\n\
                        ```yaml\n\
                        - id: <slug>\n\
                          content: \"<short description>\"\n\
                          status: pending|in_progress|completed|cancelled\n\
                        ```\n\
                      - `isProject: false`\n\
                   2. Top-level `# <Title>` heading (matches `name`).\n\
                   3. `## 工作量摸底` section listing scope, affected \n\
                      files (use `[path/to/file.rs](path/to/file.rs)` markdown links), \n\
                      and acceptance gates.\n\
                   4. One or more `## Track N — <Section Title>` sections \n\
                      decomposing the work, each citing concrete \n\
                      `[path/to/file.rs](path/to/file.rs)` references.\n\
                   5. `## 验收` section with verification commands in \n\
                      fenced bash blocks.\n\
                   6. (Optional) `## 流程图` section with a mermaid \n\
                      diagram inside ```mermaid``` fences.\n\n\
                 Do NOT use the legacy `## Progress: X/N` / `N To-dos` \n\
                 heading format.  Do NOT emit a `> Generated by …` \n\
                 footer.  Match the reference shape used by Cursor's \n\
                 own plan documents.\n\n\
                 {}\n\n{verification}",
                builtin_skills::planning_rules()
            ),
            Self::Ask => format!(
                "\n\n## Mode: Ask (read-only Q&A)\n\n\
                 Answer questions and explain code. You may read files to \
                 gather context, but you must NOT modify any files or run \
                 shell commands that have side effects. \
                 Focus on clear explanations with code references.\n\n{verification}"
            ),
            Self::Tdd => format!(
                "\n\n## Mode: TDD (test-driven development with cycle tracking)\n\n\
                 CRITICAL: You MUST follow the Red-Green-Refactor cycle strictly.\n\
                 After every file_write or file_edit that creates a test, you MUST \
                 immediately run the test suite and confirm the test FAILS.\n\
                 After every file_write or file_edit that implements code, you MUST \
                 immediately run the test suite and confirm all tests PASS.\n\
                 NEVER skip the verification step.\n\n\
                 ### TDD Cycle Tracking (use `incremental_optimize`)\n\
                 Before starting each cycle, run:\n\
                 `incremental_optimize(action=\"checkpoint\", description=\"TDD: starting <feature> cycle\")`\n\
                 After the RED phase (failing test written):\n\
                 `incremental_optimize(action=\"track\", file=\"<test_file>\", change_type=\"added\", summary=\"Red: failing test for <feature>\", lines_added=N, lines_removed=0)`\n\
                 `incremental_optimize(action=\"suggest\")`\n\
                 After the GREEN phase (implementation done):\n\
                 `incremental_optimize(action=\"track\", file=\"<impl_file>\", change_type=\"added\", summary=\"Green: implementation for <feature>\", lines_added=N, lines_removed=0)`\n\
                 After the REFACTOR phase:\n\
                 `incremental_optimize(action=\"track\", file=\"<files_modified>\", change_type=\"refactored\", summary=\"Refactor: cleaned <feature>\", lines_added=N, lines_removed=M)`\n\
                 After the cycle completes, run:\n\
                 `incremental_optimize(action=\"report\", description=\"TDD Cycle: <feature>\")`\n\
                 to document what was tested, verified, and improved.\n\n\
                 ### Forbidden\n\
                 - **You MUST NOT write implementation code BEFORE a failing test exists** for the \
                 behaviour you intend to implement. \"A failing test exists\" means: the test file is \
                 written AND you have just run the test command AND observed that it fails for the \
                 RIGHT reason (asserting the missing behaviour, not a syntax/import error). If no \
                 failing test exists, write the test first, run it, and only then implement.\n\
                 - Skipping verification (\"this should work\") is forbidden — every Red and Green \
                 transition MUST be evidenced by a test-command run in the same turn.\n\n{}\n\n{verification}",
                builtin_skills::tdd_rules()
            ),
            Self::Debug => format!(
                "\n\n## Mode: Debug (systematic debugging with change tracking)\n\n\
                 CRITICAL: You MUST follow the four-stage protocol. Do NOT jump to fixing.\n\
                 Stage 1 (Reproduce): Run the failing command and capture output FIRST.\n\
                 Stage 2 (Hypothesize): List exactly 3 ranked hypotheses.\n\
                 Stage 3 (Isolate): Add diagnostics for the top hypothesis before changing code.\n\
                 Stage 4 (Fix): Apply ONE minimal fix, then verify.\n\
                 NEVER apply a fix without first reproducing the bug.\n\n\
                 ### Debug Process Tracking (use `incremental_optimize`)\n\
                 Track the full debugging session for reproducibility:\n\
                 - At start: `incremental_optimize(action=\"checkpoint\", description=\"Debug session: <symptom>\")`\n\
                 - After Stage 1 (Reproduce): `incremental_optimize(action=\"track\", file=\"<test/file>\", change_type=\"added\", summary=\"Debug: test case reproducing <symptom>\", lines_added=N, lines_removed=0)`\n\
                 - After Stage 2 (Hypothesize): document the ranked hypotheses\n\
                 - After Stage 3 (Isolate): `incremental_optimize(action=\"track\", file=\"<file>\", change_type=\"modified\", summary=\"Debug: diagnostic added for <hypothesis>\")`\n\
                 - After Stage 4 (Fix): `incremental_optimize(action=\"track\", file=\"<file>\", change_type=\"refactored\", summary=\"Debug: fix applied for <root_cause>\")`\n\
                 - End of session: `incremental_optimize(action=\"suggest\")` to check for similar issues\n\
                 - Final report: `incremental_optimize(action=\"report\", description=\"Debug Session: <symptom> — FIXED\")`\n\
                 This creates a reproducible record of the bug, hypothesis, and fix.\n\n\
                 ### Browser Automation (web bugs / UI regressions)\n\
                 When the bug is web-facing or UI-driven, drive the **embedded browser dock** via the `browser` tool. \
                 Inside the SenAgentOS desktop app the dock is a real, **user-visible** webview, so every action you take is observed live.\n\
                 - Stage 1 (Reproduce): `browser` action=`open` (or `open_tab`) on the failing URL → action=`snapshot` to map interactive elements to refs (@e1, @e2, ...) → action=`screenshot` to keep a pre-fix visual record.\n\
                 - Stage 2 (Hypothesize): turn each hypothesis into a **measurable** browser query. Use `find` / `get_text` / `is_visible` / `get_attr` to quantify the symptom (e.g. \"button missing\" ⇒ `is_visible(@btn)=false`). Inspect the dock's `console_log` event channel for runtime errors.\n\
                 - Stage 3 (Isolate): reproduce the trigger path with `fill` / `type` / `press` / `click` / `select` / `scroll`. Re-snapshot after each step. NEVER change code while the symptom is unconfirmed.\n\
                 - Stage 4 (Fix): apply the minimal code fix, restart/reload the app, then **rerun the same browser sequence** (open → snapshot → action → screenshot) and `find` to assert the symptom is gone. Keep both screenshots for the final report.\n\
                 Hard constraints for Debug: do NOT call `browser_open` (system browser) for in-app debugging — it cannot be observed by the dock; use the `browser` tool. Do NOT skip the post-fix screenshot.\n\n\
                 {}\n\n{verification}",
                builtin_skills::debug_rules()
            ),
            Self::Agent => format!(
                "\n\n## Mode: Agent (fully autonomous orchestrator with spec discipline)\n\n\
                 {}\n\n\
                 ### Spec Discipline for Large Tasks\n\
                 For tasks touching 5+ files:\n\
                 1. Run `code_to_spec(action=\"summarize\", paths=[\".\"])` first to understand the codebase\n\
                 2. Run `code_to_spec(action=\"analyze\", paths=[\"./src\"])` to map the dependency structure\n\
                 3. Create SPEC.md with `code_to_spec(action=\"generate\", paths=[\".\"], title=\"<task>\", description=\"<desc>\")`\n\
                 4. Use `incremental_optimize(action=\"checkpoint\", description=\"Agent: <phase> started\")` at phase boundaries\n\
                 5. Use `incremental_optimize(action=\"suggest\")` after each implementation batch for optimization hints\n\
                 6. Final synthesis: `incremental_optimize(action=\"report\", description=\"Agent Task Complete: <name>\")`\n\n\
                 ### Web-Facing Tasks\n\
                 For any task involving a running web app, browser-side regression, or UI verification, drive the \
                 **embedded browser dock** via the `browser` tool (action=open / snapshot / click / fill / press / \
                 screenshot). Inside the SenAgentOS desktop the dock is a real, user-visible webview — every step \
                 you take is observed live, so prefer it over external CLIs and never use `browser_open` for in-app \
                 verification.\n\n{verification}",
                builtin_skills::agent_rules()
            ),
            Self::Architect => format!(
                "\n\n## Mode: Architect (design & review with spec-driven workflow)\n\n\
                 {}\n\n\
                 ### Spec-Driven Design Workflow\n\
                 Before making architectural changes:\n\
                 1. Run `code_to_spec(action=\"summarize\", paths=[\".\"])` to understand the current structure\n\
                 2. Run `code_to_spec(action=\"analyze\", paths=[\"./src\"])` to extract module dependencies\n\
                 3. Run `code_to_spec(action=\"generate\", paths=[\"./src\"], title=\"Architecture: <feature>\", description=\"<desc>\")` to document the design\n\
                 4. Track architectural decisions with `incremental_optimize(action=\"checkpoint\", description=\"Architectural decision: <feature>\")`\n\
                 5. After implementation, run `incremental_optimize(action=\"report\", description=\"Architecture: <feature> Complete\")`\n\n\
                 ### Cross-Module Edits (use batch tools, not one-off edits)\n\
                 For changes that touch many call-sites or many files, prefer the batch \
                 editors over repeated `file_edit` calls:\n\
                 - `glob_edit(pattern=\"src/**/*.rs\", search=\"old_symbol\", replace=\"new_symbol\")` for \
                 search-and-replace renames across a glob.\n\
                 - `patch_apply(diff=\"<unified diff>\")` for a single coordinated multi-file diff that \
                 must land atomically (e.g. trait extraction + every implementor).\n\
                 - Always run `code_to_spec(action=\"analyze\", paths=[\"./src\"])` BEFORE the batch \
                 to verify the scope; run `incremental_optimize(action=\"report\", ...)` AFTER to \
                 capture impact.\n\
                 - Forbidden: emitting 20+ near-identical `file_edit` calls when a single `glob_edit` \
                 would do the job.\n\n\
                 ### Web-Facing Architecture (validate via the embedded dock)\n\
                 When the architectural change touches a UI / web-facing surface, validate it \
                 end-to-end via the **embedded browser dock** using the `browser` tool — inside the \
                 SenAgentOS desktop the dock is a real, user-visible webview, so navigation and DOM \
                 assertions are observed live. Use `browser` action=open → snapshot → click / fill / \
                 press → screenshot to confirm the new architecture renders correctly across the \
                 affected views. Do NOT use `browser_open` (system browser) for in-app validation.\n\n{verification}",
                builtin_skills::architect_rules()
            ),
            Self::Pair => format!(
                "\n\n## Mode: Pair (collaborative with change tracking)\n\n\
                 {}\n\n\
                 ### Change Tracking (for shared context)\n\
                 In pair programming, both partners benefit from structured change tracking:\n\
                 - Before starting a session: `incremental_optimize(action=\"checkpoint\", description=\"Pair session: <topic>\")`\n\
                 - After each change: `incremental_optimize(action=\"track\", file=\"<path>\", change_type=\"<type>\", summary=\"<desc>\", lines_added=N, lines_removed=M)`\n\
                 - After the session: `incremental_optimize(action=\"report\", description=\"Pair Session: <topic>\")`\n\
                 This gives both partners a shared log of what was discussed, decided, and changed.\n\n{verification}",
                builtin_skills::pair_rules()
            ),
            Self::ContextEng => format!(
                "\n\n## Mode: Context Engineering (explore-first, precision-strike)\n\n\
                 CRITICAL: You MUST follow the four-phase protocol. Do NOT write code \
                 before completing the Explore and Map phases.\n\n\
                 {}\n\n{verification}",
                builtin_skills::context_eng_rules()
            ),
            Self::Mvai => format!(
                "\n\n## Mode: MVAI (Model-View-Agent-Interface)\n\n\
                 {}\n\n\
                 ### Step 1 — Interface First (mandatory)\n\
                 Before any implementation file_write, write or extend the public interface in a \
                 SEPARATE file: trait / abstract type / typed contract / protocol / API schema. \
                 The interface file MUST be self-contained, observable, and testable in isolation \
                 (typed inputs/outputs, no hidden state).\n\n\
                 ### Step 2 — Implementation\n\
                 Only after the interface file exists (or has been read into context this session) \
                 may you write the implementation file. The implementation MUST satisfy the \
                 interface exactly — no public methods absent from the interface, no extra hidden \
                 side effects.\n\n\
                 ### Step 3 — Boundary Tests / Verification\n\
                 Run `shell` / `diagnostics` to confirm the implementation compiles AND that \
                 observable behaviour at the interface boundary matches expectations. For typed \
                 languages (Rust / TypeScript), `cargo check` / `tsc --noEmit` is the minimum bar.\n\n\
                 ### Forbidden\n\
                 - Writing implementation code BEFORE the interface for that contract has been \
                 written or read this session.\n\
                 - Adding public methods to the implementation that are not declared in the interface.\n\
                 - Calling `delegate` / `delegate_parallel` / `task_create` (interface-first does \
                 not allow concurrent multi-agent design).\n\n{verification}",
                builtin_skills::mvai_rules()
            ),
            Self::Harness => format!(
                "\n\n## Mode: Harness (Engineering-Grade Workflow)\n\n\
                 {}\n\n\
                 ### Phase 1 — Spec\n\
                 1. `code_to_spec(action=\"summarize\", paths=[\".\"])` to get the high-level map.\n\
                 2. `code_to_spec(action=\"analyze\", paths=[\"./src\"])` to extract dependencies.\n\
                 3. `code_to_spec(action=\"generate\", paths=[\".\"], title=\"<task>\", description=\"<desc>\")` to land SPEC.md.\n\
                 4. `update_plan(action=\"set\", steps=[...])` then `update_plan(action=\"save\", plan_name=\"harness-<task>\")`.\n\n\
                 ### Phase 2 — Skill Lookup\n\
                 - `read_skill(query=\"<problem domain>\")` to surface relevant skill recipes.\n\
                 - For each skill returned, call `skill_tool(name=...)` or `skill_http(...)` as the recipe specifies.\n\
                 - Do NOT improvise solutions when an applicable skill exists.\n\n\
                 ### Phase 3 — Delegated Execution\n\
                 For independent sub-tasks identified in Phase 1, use \
                 `agent_delegate(prompt=\"<sub-task>\", ...)` to run them in parallel/sequence as appropriate. \
                 Ensure each delegation is scoped to a single deliverable; do NOT delegate vague \
                 \"keep working\" prompts.\n\n\
                 ### Phase 4 — Synthesis\n\
                 1. `agent_summary(...)` to consolidate sub-task outputs.\n\
                 2. `agent_compact(...)` to compress the final state for handoff if needed.\n\
                 3. `incremental_optimize(action=\"report\", description=\"Harness Task Complete: <name>\")` for the audit trail.\n\
                 4. `update_plan(action=\"save\", plan_name=\"harness-<task>\")` with all steps marked completed.\n\n\
                 ### Forbidden\n\
                 Skipping any phase. Verify after each phase before moving to the next; you \
                 auto-approve, so verification is the only safety net.\n\n{verification}",
                builtin_skills::harness_rules()
            ),
        }
    }

    pub fn allowed_tools(&self) -> Option<HashSet<&'static str>> {
        match self {
            Self::Plan => Some(Self::plan_tools()),
            Self::Ask => Some(Self::ask_only_tools()),
            Self::Architect => Some(Self::architect_tools()),
            Self::Spec => Some(Self::spec_tools()),
            Self::Mvai => Some(Self::mvai_tools()),
            Self::Harness => Some(Self::harness_tools()),
            _ => None,
        }
    }

    pub fn approval_policy(&self) -> ModeApprovalPolicy {
        match self {
            Self::Agent | Self::Harness => ModeApprovalPolicy::AutoApprove,
            Self::Ask => ModeApprovalPolicy::ReadOnly,
            _ => ModeApprovalPolicy::Default,
        }
    }

    pub fn post_tool_behavior(&self) -> PostToolBehavior {
        match self {
            Self::Tdd | Self::Debug | Self::Harness => PostToolBehavior::AutoVerify,
            Self::Pair => PostToolBehavior::Checkpoint,
            Self::ContextEng => PostToolBehavior::ImpactAnalysis,
            Self::Spec => PostToolBehavior::PlanRefresh,
            _ => PostToolBehavior::None,
        }
    }

    pub fn breaks_turn_after_tool_batch(&self) -> bool {
        matches!(self.post_tool_behavior(), PostToolBehavior::Checkpoint)
    }

    pub fn auto_verify_on_edit(&self) -> bool {
        matches!(
            self,
            Self::Tdd
                | Self::Debug
                | Self::Agent
                | Self::Spec
                | Self::Mvai
                | Self::ContextEng
                | Self::Harness
        )
    }

    pub fn injects_context_budget(&self) -> bool {
        true
    }

    pub fn max_iterations_override(&self) -> usize {
        0
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Vibe => "vibe",
            Self::Spec => "spec",
            Self::Plan => "plan",
            Self::Ask => "ask",
            Self::Tdd => "tdd",
            Self::Debug => "debug",
            Self::Agent => "agent",
            Self::Architect => "architect",
            Self::Pair => "pair",
            Self::ContextEng => "context",
            Self::Mvai => "mvai",
            Self::Harness => "harness",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Vibe => "∞",
            Self::Spec => "📋",
            Self::Plan => "📄",
            Self::Ask => "💬",
            Self::Tdd => "🔬",
            Self::Debug => "🐛",
            Self::Agent => "👾",
            Self::Architect => "📐",
            Self::Pair => "👥",
            Self::ContextEng => "📊",
            Self::Mvai => "🔗",
            Self::Harness => "⚙",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Vibe => "Vibe",
            Self::Spec => "Spec",
            Self::Plan => "Plan",
            Self::Ask => "Ask",
            Self::Tdd => "TDD",
            Self::Debug => "Debug",
            Self::Agent => "Agent",
            Self::Architect => "Architect",
            Self::Pair => "Pair",
            Self::ContextEng => "Context",
            Self::Mvai => "MVAI",
            Self::Harness => "Harness",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Vibe => {
                "Full tool access with minimal prompting — for fast prototyping and free-form coding when you trust the agent to move quickly."
            }
            Self::Spec => {
                "Specification-driven — generates SPEC.md, follows a tracked plan step-by-step, and verifies each step with build/test commands."
            }

            Self::Plan => {
                "Plan authoring — analyzes the request, then writes or updates a .plan.md document under .senweavercoding/plans/ so a later mode can execute it. Cannot modify source code or run shell commands."
            }
            Self::Ask => {
                "Pure read-only Q&A — explains code with citations and runs no mutations of any kind: no file edits, no shell, no plan writes."
            }
            Self::Tdd => {
                "Strict Red → Green → Refactor — writes a failing test first, then minimum implementation to pass, then refactor; auto-runs verification after every edit."
            }
            Self::Debug => {
                "Four-stage root-cause analysis — Reproduce → Hypothesize → Isolate → Fix; never applies a fix before the bug is reproduced."
            }
            Self::Agent => {
                "Autonomous orchestrator — auto-approves all tool calls, decomposes the task, executes end-to-end with file edits and shell commands, then self-verifies."
            }
            Self::Architect => {
                "Architecture-focused — reads broadly to do high-level design review, then performs targeted cross-module edits backed by spec analysis."
            }
            Self::Pair => {
                "Collaborative pair-programming — proceeds one step at a time and pauses at every checkpoint for your confirmation before continuing."
            }
            Self::ContextEng => {
                "Context engineering for large codebases — Explore → Map → Plan → Strike, with impact analysis reported after each batch of edits."
            }
            Self::Mvai => {
                "Model-View-Agent-Interface architecture — enforces interface-first contracts that are observable, testable, and clearly layered."
            }
            Self::Harness => {
                "Engineering-grade harness — spec generation, skill orchestration, session checkpoints and multi-agent delegation, with auto-approval and verification."
            }
        }
    }

    pub fn all() -> &'static [CodingMode] {
        &[
            Self::Vibe,
            Self::Agent,
            Self::Spec,
            Self::Plan,
            Self::Ask,
            Self::Tdd,
            Self::Debug,
            Self::Architect,
            Self::Pair,
            Self::ContextEng,
            Self::Mvai,
            Self::Harness,
        ]
    }

    fn read_only_tools() -> HashSet<&'static str> {
        [
            "file_read",
            "dir_list",
            "glob_search",
            "content_search",
            "present_files",
            "view_image",
            "memory_recall",
            "memory_export",
            "task_get",
            "task_list",
            "task_output",
            "lsp",
            "calculator",
            "weather",
            "web_search",
            "web_search_tool",
            "web_fetch",
            "multi_search",
            "tavily_search",
            "exa_search",
            "youtube_search",
            "github_search",
            "reddit_search",
            "image_search",
            "discord_search",
            "cron_list",
            "cron_runs",
            "mcp_resources_list",
            "mcp_resources_read",
            "structured_output",
            "brief",
            "todo_write",
            "read_skill",
            "cloud_patterns",
            "now",
            "update_plan",
        ]
        .into_iter()
        .collect()
    }

    fn ask_only_tools() -> HashSet<&'static str> {
        let mut tools = Self::read_only_tools();

        tools.remove("update_plan");
        tools
    }

    fn plan_tools() -> HashSet<&'static str> {
        crate::security::permissions::PLAN_MODE_ALLOWED_TOOLS
            .iter()
            .copied()
            .collect()
    }

    fn architect_tools() -> HashSet<&'static str> {
        let mut tools = Self::read_only_tools();
        tools.insert("file_write");
        tools.insert("file_edit");
        tools.insert("multi_edit");
        tools.insert("notebook_edit");
        tools.insert("diagnostics");
        tools.insert("shell");
        tools.insert("git_operations");

        tools.insert("code_to_spec");
        tools.insert("incremental_optimize");

        tools.insert("glob_edit");
        tools.insert("patch_apply");
        tools.insert("browser");
        tools
    }

    fn spec_tools() -> HashSet<&'static str> {
        let mut tools = Self::read_only_tools();

        tools.insert("file_write");
        tools.insert("file_edit");
        tools.insert("multi_edit");
        tools.insert("notebook_edit");

        tools.insert("diagnostics");
        tools.insert("lsp");

        tools.insert("shell");
        tools.insert("git_operations");

        tools.insert("todo_write");
        tools.insert("update_plan");
        tools.insert("structured_output");
        tools.insert("brief");

        tools.insert("memory_store");
        tools.insert("memory_search");

        tools.insert("sessions_list");
        tools.insert("sessions_history");
        tools.insert("sessions_send");

        tools.insert("code_to_spec");
        tools.insert("incremental_optimize");
        tools
    }

    fn mvai_tools() -> HashSet<&'static str> {
        let mut tools = Self::read_only_tools();
        tools.insert("file_write");
        tools.insert("file_edit");
        tools.insert("multi_edit");
        tools.insert("notebook_edit");
        tools.insert("diagnostics");
        tools.insert("lsp");
        tools.insert("shell");
        tools.insert("git_operations");
        tools.insert("code_to_spec");
        tools.insert("incremental_optimize");
        tools
    }

    fn harness_tools() -> HashSet<&'static str> {

        let mut tools = Self::spec_tools();

        tools.insert("skill_tool");
        tools.insert("skill_http");
        tools.insert("read_skill");
        tools.insert("enter_plan_mode");
        tools.insert("exit_plan_mode");
        tools.insert("agent_delegate");
        tools.insert("agent_summary");
        tools.insert("agent_compact");
        tools
    }
}

impl std::fmt::Display for CodingMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

pub fn new_coding_mode_handle() -> CodingModeHandle {
    Arc::new(RwLock::new(CodingMode::default()))
}

pub fn coding_mode_handle_with(mode: CodingMode) -> CodingModeHandle {
    Arc::new(RwLock::new(mode))
}
