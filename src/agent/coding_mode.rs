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

/// Shared handle for the current coding mode.
pub type CodingModeHandle = Arc<RwLock<CodingMode>>;

/// Eleven switchable programming workflows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodingMode {
    /// Fully autonomous, minimal prompting — fast prototyping and free coding.
    Vibe,
    /// Specification-driven development — brainstorm, plan, then execute with verification.
    Spec,
    /// Read-only planning mode — analysis and architecture without file modifications.
    Plan,
    /// Read-only Q&A mode — explain code, no file changes, no shell.
    Ask,
    /// Test-driven development — strict Red-Green-Refactor discipline.
    Tdd,
    /// Systematic debugging — four-stage root-cause analysis.
    Debug,
    /// Fully autonomous agent — auto-approves everything, decomposes tasks, orchestrates
    /// multi-step execution with self-correction and quality gates.
    Agent,
    /// High-level design and review — reads everything, targeted edits for architecture.
    Architect,
    /// Interactive pair programming — collaborative with explicit checkpoints.
    Pair,
    /// Context Engineering — explore-first, precision-strike development for large codebases.
    ContextEng,
    /// MVAI — Model-View-Agent-Interface: structured contracts, observable, testable.
    Mvai,
    /// Harness — engineering-grade workflow: spec → skill orchestration → session checkpoints → verification.
    Harness,
}

/// How the mode affects the approval flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeApprovalPolicy {
    /// Use the default approval manager (config-driven supervised).
    Default,
    /// Auto-approve all tool calls — no interactive prompts.
    AutoApprove,
    /// Block all write operations at the tool filtering level.
    ReadOnly,
}

/// Post-tool-call behavior hooks controlled by the mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostToolBehavior {
    /// No special post-tool behavior.
    None,
    /// After file_write/file_edit, auto-run the project's check/build command.
    AutoVerify,
    /// After each step, prompt for user confirmation before proceeding (Pair mode).
    Checkpoint,
    /// After each batch, analyze impact scope and report context changes (ContextEng).
    ImpactAnalysis,
}

impl CodingMode {
    /// Parse a mode name from user input (case-insensitive).
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

    /// System prompt fragment for this mode.
    pub fn system_prompt_injection(&self) -> String {
        let verification = builtin_skills::verification_rules();
        match self {
            Self::Vibe => format!(
                "\n\n## Mode: Vibe (full autonomy)\n\n\
                 You have full tool access. Work autonomously, be creative, \
                 and move fast. Ask only when truly ambiguous.\n\n{verification}"
            ),
            Self::Spec => format!(
                "\n\n## Mode: Spec (specification-driven with incremental optimization)\n\n\
                 Follow this structured workflow for every task:\n\n\
                 ### Step 1 — Analyze and Spec (before any code change)\n\
                 Use `code_to_spec` to understand the existing codebase:\n\
                 - Run `code_to_spec(action=\"summarize\", paths=[\".\"])` for a quick overview\n\
                 - Run `code_to_spec(action=\"analyze\", paths=[\"./src\"])` to extract structural info\n\
                 - Run `code_to_spec(action=\"generate\", paths=[\"./src\"], title=\"<title>\", description=\"<desc>\")` to create SPEC.md\n\
                 - Before modifying existing code, run `code_to_spec(action=\"compare\", spec_path=\"SPEC.md\")` to check for gaps\n\n\
                 ### Step 2 — Track and Optimize (incremental improvement)\n\
                 Use `incremental_optimize` to manage changes systematically:\n\
                 - Run `incremental_optimize(action=\"checkpoint\", description=\"pre-change snapshot\")` before starting\n\
                 - Run `incremental_optimize(action=\"track\", file=\"<path>\", change_type=\"modified\", summary=\"<desc>\", lines_added=N, lines_removed=M)` after each change\n\
                 - Run `incremental_optimize(action=\"suggest\")` to get optimization recommendations\n\
                 - Run `incremental_optimize(action=\"verify\", change_id=N, suggestion_id=\"<id>\")` to mark improvements as applied\n\
                 - Run `incremental_optimize(action=\"report\", description=\"<title>\")` to summarize the optimization loop\n\n\
                 ### Step 3 — Execute and Verify (standard spec-driven workflow)\n\
                 1. Clarify requirements and edge cases with the user\n\
                 2. Create a detailed plan using `todo_write` with file-level changes\n\
                 3. Execute the plan step-by-step, verifying each step with build/test commands\n\
                 4. After completing all steps, run the full test suite and report results\n\n\
                 You MUST create a SPEC.md before making significant code changes.\n\
                 You MUST verify each step compiles before moving to the next.\n\
                 You MUST track changes with `incremental_optimize` for any non-trivial modification.\n\n{}\n\n{verification}",
                builtin_skills::planning_rules()
            ),
            Self::Plan => format!(
                "\n\n## Mode: Plan (read-only)\n\n\
                 You are in read-only mode. Analyze code, create plans, \
                 and provide recommendations — but do NOT modify any files. \
                 Only read-only tools are available.\n\n\
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
                 to document what was tested, verified, and improved.\n\n{}\n\n{verification}",
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
                 This creates a reproducible record of the bug, hypothesis, and fix.\n\n{}\n\n{verification}",
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
                 6. Final synthesis: `incremental_optimize(action=\"report\", description=\"Agent Task Complete: <name>\")`\n\n{verification}",
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
                 5. After implementation, run `incremental_optimize(action=\"report\", description=\"Architecture: <feature> Complete\")`\n\n{verification}",
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
                 {}\n\n{verification}",
                builtin_skills::mvai_rules()
            ),
            Self::Harness => format!(
                "\n\n## Mode: Harness (Engineering-Grade Workflow)\n\n\
                 {}\n\n{verification}",
                builtin_skills::harness_rules()
            ),
        }
    }

    /// Returns an allowlist of tool names. `None` means all tools are allowed.
    pub fn allowed_tools(&self) -> Option<HashSet<&'static str>> {
        match self {
            Self::Plan => Some(Self::read_only_tools()),
            Self::Ask => Some(Self::ask_only_tools()),
            Self::Architect => Some(Self::architect_tools()),
            Self::Spec | Self::Harness => Some(Self::spec_tools()),
            _ => None,
        }
    }

    /// How this mode affects the approval flow.
    pub fn approval_policy(&self) -> ModeApprovalPolicy {
        match self {
            Self::Agent | Self::Harness => ModeApprovalPolicy::AutoApprove,
            Self::Plan | Self::Ask => ModeApprovalPolicy::ReadOnly,
            _ => ModeApprovalPolicy::Default,
        }
    }

    /// Post-tool-call behavior hook for this mode.
    pub fn post_tool_behavior(&self) -> PostToolBehavior {
        match self {
            Self::Tdd | Self::Debug | Self::Harness => PostToolBehavior::AutoVerify,
            Self::Pair => PostToolBehavior::Checkpoint,
            Self::ContextEng => PostToolBehavior::ImpactAnalysis,
            _ => PostToolBehavior::None,
        }
    }

    /// Whether this mode should auto-run verification commands after file changes.
    pub fn auto_verify_on_edit(&self) -> bool {
        matches!(
            self,
            Self::Tdd | Self::Debug | Self::Agent | Self::Spec | Self::Mvai | Self::ContextEng | Self::Harness
        )
    }

    /// Whether this mode injects context budget information into the prompt.
    /// Enabled for all modes — context awareness is a universal capability.
    pub fn injects_context_budget(&self) -> bool {
        true
    }

    /// Maximum tool iterations for this mode (0 = use config default).
    pub fn max_iterations_override(&self) -> usize {
        match self {
            Self::Agent => 200,
            Self::Spec | Self::Mvai | Self::ContextEng | Self::Harness => 100,
            Self::Ask | Self::Plan => 20,
            _ => 0,
        }
    }

    /// Display name for the REPL prompt badge.
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

    /// Short description for `/mode` listing.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Vibe => "Full autonomy, minimal prompting — fast prototyping",
            Self::Spec => "Specification-driven — plan then execute with verification",
            Self::Plan => "Read-only planning — analysis without modifications",
            Self::Ask => "Read-only Q&A — explain code, no changes",
            Self::Tdd => "Test-driven development — Red-Green-Refactor enforced",
            Self::Debug => "Systematic debugging — four-stage root-cause analysis",
            Self::Agent => "Autonomous agent — decompose, execute, verify, synthesize",
            Self::Architect => "Architecture & design — high-level review with targeted edits",
            Self::Pair => "Pair programming — collaborative with checkpoints",
            Self::ContextEng => "Context engineering — explore-first, precision-strike for large codebases",
            Self::Mvai => "MVAI — interface-first, observable, testable architecture",
            Self::Harness => "Engineering-grade harness — spec → skill orchestration → session checkpoints → verification",
        }
    }

    /// All available modes.
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
        tools.remove("todo_write");
        tools.remove("update_plan");
        tools
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
        // Spec engineering tools: code analysis and incremental optimization
        tools.insert("code_to_spec");
        tools.insert("incremental_optimize");
        tools
    }

    /// Tools for Spec engineering mode — focused on spec generation, code analysis, and incremental optimization.
    fn spec_tools() -> HashSet<&'static str> {
        let mut tools = Self::read_only_tools();
        // Write / edit tools for spec files
        tools.insert("file_write");
        tools.insert("file_edit");
        tools.insert("multi_edit");
        tools.insert("notebook_edit");
        // Code analysis & diagnostics
        tools.insert("diagnostics");
        tools.insert("lsp");
        // Shell / build / test
        tools.insert("shell");
        tools.insert("git_operations");
        // Planning, state, skills
        tools.insert("todo_write");
        tools.insert("update_plan");
        tools.insert("structured_output");
        tools.insert("brief");
        // Memory persistence
        tools.insert("memory_store");
        tools.insert("memory_search");
        // Session management
        tools.insert("sessions_list");
        tools.insert("sessions_history");
        tools.insert("sessions_send");
        // Spec engineering tools (Layer 1: Spec + Layer 5: Optimization)
        tools.insert("code_to_spec");
        tools.insert("incremental_optimize");
        tools
    }

    /// Tools for Harness engineering mode — builds on spec_tools with additional engineering skills.
    fn harness_tools() -> HashSet<&'static str> {
        // Start from spec_tools (includes code_to_spec, incremental_optimize, etc.)
        let mut tools = Self::spec_tools();
        // Additional Harness-specific tools
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

impl Default for CodingMode {
    fn default() -> Self {
        Self::Vibe
    }
}

/// Create a new shared coding mode handle with the default mode.
pub fn new_coding_mode_handle() -> CodingModeHandle {
    Arc::new(RwLock::new(CodingMode::default()))
}

/// Create a shared handle starting in the given mode.
pub fn coding_mode_handle_with(mode: CodingMode) -> CodingModeHandle {
    Arc::new(RwLock::new(mode))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mode_names() {
        assert_eq!(CodingMode::from_str_loose("vibe"), Some(CodingMode::Vibe));
        assert_eq!(CodingMode::from_str_loose("PLAN"), Some(CodingMode::Plan));
        assert_eq!(CodingMode::from_str_loose("tdd"), Some(CodingMode::Tdd));
        assert_eq!(CodingMode::from_str_loose("d"), Some(CodingMode::Debug));
        assert_eq!(CodingMode::from_str_loose("a"), Some(CodingMode::Ask));
        assert_eq!(CodingMode::from_str_loose("s"), Some(CodingMode::Spec));
        assert_eq!(CodingMode::from_str_loose("agent"), Some(CodingMode::Agent));
        assert_eq!(CodingMode::from_str_loose("auto"), Some(CodingMode::Agent));
        // "agentic" and "ae" now resolve to Agent (merged)
        assert_eq!(
            CodingMode::from_str_loose("agentic"),
            Some(CodingMode::Agent)
        );
        assert_eq!(CodingMode::from_str_loose("ae"), Some(CodingMode::Agent));
        assert_eq!(
            CodingMode::from_str_loose("architect"),
            Some(CodingMode::Architect)
        );
        assert_eq!(CodingMode::from_str_loose("pair"), Some(CodingMode::Pair));
        assert_eq!(
            CodingMode::from_str_loose("context"),
            Some(CodingMode::ContextEng)
        );
        assert_eq!(
            CodingMode::from_str_loose("ce"),
            Some(CodingMode::ContextEng)
        );
        assert_eq!(CodingMode::from_str_loose("mvai"), Some(CodingMode::Mvai));
        assert_eq!(CodingMode::from_str_loose("harness"), Some(CodingMode::Harness));
        assert_eq!(CodingMode::from_str_loose("hn"), Some(CodingMode::Harness));
        assert_eq!(CodingMode::from_str_loose("unknown"), None);
    }

    #[test]
    fn tool_restrictions() {
        assert!(CodingMode::Plan.allowed_tools().is_some());
        assert!(CodingMode::Ask.allowed_tools().is_some());
        assert!(CodingMode::Architect.allowed_tools().is_some());
        assert!(CodingMode::Harness.allowed_tools().is_some());
        assert!(CodingMode::Spec.allowed_tools().is_some()); // Spec now has spec_tools (includes code_to_spec, incremental_optimize)
        assert!(CodingMode::Vibe.allowed_tools().is_none());
        assert!(CodingMode::Tdd.allowed_tools().is_none());
        assert!(CodingMode::Debug.allowed_tools().is_none());
        assert!(CodingMode::Agent.allowed_tools().is_none());
        assert!(CodingMode::Pair.allowed_tools().is_none());
        assert!(CodingMode::ContextEng.allowed_tools().is_none());
        assert!(CodingMode::Mvai.allowed_tools().is_none());
    }

    #[test]
    fn plan_tools_exclude_writes() {
        let tools = CodingMode::Plan.allowed_tools().unwrap();
        assert!(tools.contains("file_read"));
        assert!(tools.contains("web_search"));
        assert!(!tools.contains("file_write"));
        assert!(!tools.contains("shell"));
        assert!(!tools.contains("file_edit"));
    }

    #[test]
    fn ask_excludes_more_than_plan() {
        let plan = CodingMode::Plan.allowed_tools().unwrap();
        let ask = CodingMode::Ask.allowed_tools().unwrap();
        assert!(ask.len() < plan.len());
        assert!(ask.contains("file_read"));
        assert!(!ask.contains("todo_write"));
    }

    #[test]
    fn architect_includes_targeted_edits() {
        let tools = CodingMode::Architect.allowed_tools().unwrap();
        assert!(tools.contains("file_read"));
        assert!(tools.contains("file_write"));
        assert!(tools.contains("file_edit"));
        assert!(tools.contains("shell"));
        assert!(tools.contains("diagnostics"));
    }

    #[test]
    fn approval_policies() {
        assert_eq!(
            CodingMode::Agent.approval_policy(),
            ModeApprovalPolicy::AutoApprove
        );
        assert_eq!(
            CodingMode::Harness.approval_policy(),
            ModeApprovalPolicy::AutoApprove
        );
        assert_eq!(
            CodingMode::Plan.approval_policy(),
            ModeApprovalPolicy::ReadOnly
        );
        assert_eq!(
            CodingMode::Ask.approval_policy(),
            ModeApprovalPolicy::ReadOnly
        );
        assert_eq!(
            CodingMode::Vibe.approval_policy(),
            ModeApprovalPolicy::Default
        );
        assert_eq!(
            CodingMode::ContextEng.approval_policy(),
            ModeApprovalPolicy::Default
        );
        assert_eq!(
            CodingMode::Mvai.approval_policy(),
            ModeApprovalPolicy::Default
        );
    }

    #[test]
    fn auto_verify_modes() {
        assert!(CodingMode::Tdd.auto_verify_on_edit());
        assert!(CodingMode::Debug.auto_verify_on_edit());
        assert!(CodingMode::Agent.auto_verify_on_edit());
        assert!(CodingMode::Spec.auto_verify_on_edit());
        assert!(CodingMode::Mvai.auto_verify_on_edit());
        assert!(CodingMode::ContextEng.auto_verify_on_edit());
        assert!(CodingMode::Harness.auto_verify_on_edit());
        assert!(!CodingMode::Vibe.auto_verify_on_edit());
        assert!(!CodingMode::Ask.auto_verify_on_edit());
    }

    #[test]
    fn context_budget_injection_all_modes() {
        for mode in CodingMode::all() {
            assert!(
                mode.injects_context_budget(),
                "Mode {:?} should inject context budget",
                mode
            );
        }
    }

    #[test]
    fn post_tool_behaviors() {
        assert_eq!(
            CodingMode::Tdd.post_tool_behavior(),
            PostToolBehavior::AutoVerify
        );
        assert_eq!(
            CodingMode::Harness.post_tool_behavior(),
            PostToolBehavior::AutoVerify
        );
        assert_eq!(
            CodingMode::Pair.post_tool_behavior(),
            PostToolBehavior::Checkpoint
        );
        assert_eq!(
            CodingMode::ContextEng.post_tool_behavior(),
            PostToolBehavior::ImpactAnalysis
        );
        assert_eq!(
            CodingMode::Vibe.post_tool_behavior(),
            PostToolBehavior::None
        );
    }

    #[test]
    fn system_prompt_contains_mode_name() {
        for mode in CodingMode::all() {
            let prompt = mode.system_prompt_injection();
            assert!(
                prompt.contains("Mode:"),
                "Mode {:?} prompt missing header",
                mode
            );
        }
    }

    #[test]
    fn all_modes_have_descriptions() {
        for mode in CodingMode::all() {
            assert!(!mode.description().is_empty());
            assert!(!mode.display_name().is_empty());
        }
    }

    #[test]
    fn default_is_vibe() {
        assert_eq!(CodingMode::default(), CodingMode::Vibe);
    }

    #[test]
    fn iteration_limits() {
        assert_eq!(CodingMode::Agent.max_iterations_override(), 200);
        assert_eq!(CodingMode::Spec.max_iterations_override(), 100);
        assert_eq!(CodingMode::Mvai.max_iterations_override(), 100);
        assert_eq!(CodingMode::ContextEng.max_iterations_override(), 100);
        assert_eq!(CodingMode::Harness.max_iterations_override(), 100);
        assert_eq!(CodingMode::Vibe.max_iterations_override(), 0);
    }

    #[test]
    fn all_contains_twelve_modes() {
        assert_eq!(CodingMode::all().len(), 12);
    }

    #[test]
    fn agent_includes_orchestration() {
        let prompt = CodingMode::Agent.system_prompt_injection();
        assert!(prompt.contains("Decompose"));
        assert!(prompt.contains("Synthesize"));
        assert!(prompt.contains("Self-correct"));
        assert!(prompt.contains("Quality Gates"));
    }

    #[test]
    fn context_eng_is_real_engineering_mode() {
        let prompt = CodingMode::ContextEng.system_prompt_injection();
        assert!(prompt.contains("Phase 1: Explore"));
        assert!(prompt.contains("Phase 2: Map"));
        assert!(prompt.contains("Phase 3: Strike"));
        assert!(prompt.contains("Phase 4: Consolidate"));
        assert!(prompt.contains("Tool Discipline"));
        assert!(prompt.contains("precision-strike"));
        assert!(CodingMode::ContextEng.auto_verify_on_edit());
        assert_eq!(
            CodingMode::ContextEng.post_tool_behavior(),
            PostToolBehavior::ImpactAnalysis,
        );
        assert_eq!(CodingMode::ContextEng.max_iterations_override(), 100);
    }

    #[test]
    fn harness_is_engineering_grade_workflow() {
        let prompt = CodingMode::Harness.system_prompt_injection();
        // Core layers — use exact "Layer N: ..." headings from concat! output
        assert!(prompt.contains("Mode: Harness"));
        assert!(prompt.contains("Layer 2: Skill Orchestration Layer"));
        assert!(prompt.contains("Layer 3: Session Management Layer"));
        assert!(prompt.contains("Layer 4: Multi-Agent Orchestration Layer"));
        assert!(prompt.contains("Layer 5: Capability Enhancement Layer"));
        assert!(prompt.contains("Layer 6: Structure and Project Memory Layer"));
        // Key discipline keywords
        assert!(prompt.contains("OpenSpec") || prompt.contains("agree before you build"));
        assert!(prompt.contains("Superpowers") || prompt.contains("Engineering Discipline"));
        assert!(prompt.contains("Checkpoint") || prompt.contains("checkpoint"));
        assert!(prompt.contains("Verify") || prompt.contains("verify"));
        assert!(prompt.contains("memory_store"));
        // Behavioral guarantees
        assert!(CodingMode::Harness.auto_verify_on_edit());
        assert!(CodingMode::Harness.approval_policy() == ModeApprovalPolicy::AutoApprove);
        assert!(CodingMode::Harness.post_tool_behavior() == PostToolBehavior::AutoVerify);
        assert_eq!(CodingMode::Harness.max_iterations_override(), 100);
    }

    #[test]
    fn harness_tools_include_engineering_core() {
        let tools = CodingMode::Harness.allowed_tools().unwrap();
        // Spec & planning
        assert!(tools.contains("todo_write"));
        assert!(tools.contains("update_plan"));
        // Memory persistence
        assert!(tools.contains("memory_store"));
        assert!(tools.contains("memory_recall"));
        // Session management
        assert!(tools.contains("sessions_list"));
        assert!(tools.contains("sessions_history"));
        // Build & test
        assert!(tools.contains("shell"));
        assert!(tools.contains("git_operations"));
        // Skills
        assert!(tools.contains("read_skill"));
        // Write tools
        assert!(tools.contains("file_write"));
        assert!(tools.contains("file_edit"));
        // Harness engineering tools (Layer 1: Spec, Layer 5: Optimization)
        assert!(tools.contains("code_to_spec"));
        assert!(tools.contains("incremental_optimize"));
        // Core engineering surface is present
        assert!(tools.contains("todo_write"));
        assert!(tools.contains("update_plan"));
        assert!(tools.contains("memory_store"));
        assert!(tools.contains("diagnostics"));
    }

    #[test]
    fn spec_mode_has_code_to_spec_and_incremental_optimize() {
        let tools = CodingMode::Spec.allowed_tools().unwrap();
        // Core spec engineering tools
        assert!(tools.contains("code_to_spec"), "Spec mode must include code_to_spec");
        assert!(tools.contains("incremental_optimize"), "Spec mode must include incremental_optimize");
        // Basic read/write tools
        assert!(tools.contains("file_read"));
        assert!(tools.contains("file_write"));
        assert!(tools.contains("file_edit"));
        // Build & test
        assert!(tools.contains("shell"));
        assert!(tools.contains("git_operations"));
        // Planning
        assert!(tools.contains("todo_write"));
        assert!(tools.contains("update_plan"));
        assert!(tools.contains("diagnostics"));
    }

    #[test]
    fn spec_mode_system_prompt_mentions_tools() {
        let prompt = CodingMode::Spec.system_prompt_injection();
        assert!(prompt.contains("code_to_spec"), "Spec prompt must mention code_to_spec tool");
        assert!(prompt.contains("incremental_optimize"), "Spec prompt must mention incremental_optimize tool");
        assert!(prompt.contains("SPEC.md"), "Spec prompt must mention SPEC.md");
        assert!(prompt.contains("checkpoint"), "Spec prompt must mention checkpoint");
    }

    #[test]
    fn handle_creation() {
        let handle = new_coding_mode_handle();
        assert_eq!(*handle.read(), CodingMode::Vibe);

        let handle = coding_mode_handle_with(CodingMode::Tdd);
        assert_eq!(*handle.read(), CodingMode::Tdd);
    }
}
