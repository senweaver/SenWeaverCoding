// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::tools::traits::Tool;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinToolTier {
    Core,
    Enhanced,
    OnDemand,
}

impl BuiltinToolTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            BuiltinToolTier::Core => "Core",
            BuiltinToolTier::Enhanced => "Enhanced",
            BuiltinToolTier::OnDemand => "OnDemand",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSurfaceBaseline {
    Cli,
    Desktop,
    Both,
}

impl ToolSurfaceBaseline {
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolSurfaceBaseline::Cli => "Cli",
            ToolSurfaceBaseline::Desktop => "Desktop",
            ToolSurfaceBaseline::Both => "Both",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRiskLevel {
    Safe,
    Moderate,
    HighRisk,
}

impl ToolRiskLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolRiskLevel::Safe => "Safe",
            ToolRiskLevel::Moderate => "Moderate",
            ToolRiskLevel::HighRisk => "HighRisk",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ToolTierEntry {
    pub tier: BuiltinToolTier,
    pub surface_baseline: ToolSurfaceBaseline,
    pub risk: ToolRiskLevel,
    pub description: &'static str,
}

impl ToolTierEntry {
    pub const fn new(
        tier: BuiltinToolTier,
        surface_baseline: ToolSurfaceBaseline,
        risk: ToolRiskLevel,
        description: &'static str,
    ) -> Self {
        Self {
            tier,
            surface_baseline,
            risk,
            description,
        }
    }
}

const CORE: BuiltinToolTier = BuiltinToolTier::Core;
const ENHANCED: BuiltinToolTier = BuiltinToolTier::Enhanced;
const ON_DEMAND: BuiltinToolTier = BuiltinToolTier::OnDemand;

const SURFACE_BOTH: ToolSurfaceBaseline = ToolSurfaceBaseline::Both;
const SURFACE_DESKTOP: ToolSurfaceBaseline = ToolSurfaceBaseline::Desktop;
const SURFACE_CLI: ToolSurfaceBaseline = ToolSurfaceBaseline::Cli;

const SAFE: ToolRiskLevel = ToolRiskLevel::Safe;
const MODERATE: ToolRiskLevel = ToolRiskLevel::Moderate;
const HIGH_RISK: ToolRiskLevel = ToolRiskLevel::HighRisk;

pub static TOOL_TIERS: LazyLock<HashMap<&'static str, ToolTierEntry>> = LazyLock::new(|| {
    let mut m: HashMap<&'static str, ToolTierEntry> = HashMap::new();

    let core_entries: &[(&str, ToolRiskLevel, &str)] = &[
        ("shell", MODERATE, "Execute a shell command with sandboxing"),
        ("powershell", MODERATE, "Execute a PowerShell command on Windows"),
        ("file_read", SAFE, "Read file contents from the workspace"),
        ("file_write", MODERATE, "Write or create a file in the workspace"),
        ("file_edit", MODERATE, "Edit an existing file by patch"),
        ("apply_edit", MODERATE, "Merge a lazy edit snippet into a file via a fast apply model"),
        ("multi_edit", MODERATE, "Apply multiple file edits atomically"),
        ("notebook_edit", MODERATE, "Edit Jupyter notebook cells"),
        ("glob_search", SAFE, "Find files by glob pattern"),
        ("glob_edit", MODERATE, "Edit files matching a glob pattern"),
        ("content_search", SAFE, "Search file contents by regex"),
        ("patch_apply", MODERATE, "Apply a unified diff patch"),
        ("diff_apply", MODERATE, "Atomically apply unified diffs across multiple files"),
        ("lsp", SAFE, "LSP language service for code intelligence"),
        ("lsp_rename", MODERATE, "Rename a symbol across the workspace via LSP"),
        ("diagnostics", SAFE, "Fetch workspace diagnostics from LSPs"),
        ("git_operations", MODERATE, "Run common git operations"),
        ("code_to_spec", SAFE, "Generate or fetch code specifications"),
        ("code_graph_query", SAFE, "Query the code graph"),
        ("codebase_search", SAFE, "Semantic + lexical search over the indexed codebase"),
        ("code_outline", SAFE, "Return the structural outline of a source file"),
        ("code_xfile_refactor", MODERATE, "Cross-file refactor operation"),
        ("incremental_optimize", SAFE, "Incremental optimization helper"),
        ("dir_list", SAFE, "List directory entries"),
        ("present_files", SAFE, "Present a set of files to the user"),
        ("restore_file", MODERATE, "Restore a file to its previous version"),
        ("copy_path", MODERATE, "Copy a file or directory"),
        ("move_path", MODERATE, "Move a file or directory"),
        ("delete_path", MODERATE, "Delete a file or directory"),
        ("create_directory", SAFE, "Create a new directory"),
        ("worktree_enter", SAFE, "Enter an isolated git worktree"),
        ("worktree_exit", SAFE, "Exit the current git worktree"),
        ("inline_complete", SAFE, "Inline completion suggestion provider"),
        ("pdf_read", SAFE, "Read text content from a PDF document"),
        ("now", SAFE, "Return the current timestamp"),
        ("sleep", SAFE, "Sleep for a given duration"),
        ("read_user_rule", SAFE, "Read a user-defined rule"),
        ("read_skill", SAFE, "Read a skill definition"),
        ("tool_search", SAFE, "Activate deferred tools on demand"),
    ];
    for (name, risk, desc) in core_entries {
        m.insert(name, ToolTierEntry::new(CORE, SURFACE_BOTH, *risk, desc));
    }

    let enhanced_entries: &[(&str, ToolRiskLevel, &str)] = &[
        ("todo_write", SAFE, "Manage the agent TODO list"),
        ("update_plan", SAFE, "Update the active plan"),
        ("write_plan", MODERATE, "Author or update a plan markdown file"),
        ("enter_plan_mode", SAFE, "Enter plan mode"),
        ("exit_plan_mode", SAFE, "Exit plan mode"),
        ("code_review", SAFE, "Run an automated review over workspace changes"),
        ("sessions_outline", SAFE, "Summarize the structure of a session"),
        ("media_generate", HIGH_RISK, "Generate media via a paid provider API and write it to the workspace"),
        ("structured_output", SAFE, "Produce a structured output payload"),
        ("flow_run", MODERATE, "Run a saved flow"),
        ("flow_rollback", MODERATE, "Rollback a flow run"),
        ("setup_agent", MODERATE, "Bootstrap a delegate agent"),
        ("llm_task", MODERATE, "Run a one-shot LLM task"),
        ("multi_persona_review", MODERATE, "Five-persona expert review (default or adversarial set)"),
        ("scenario_matrix", MODERATE, "Enumerate edge-cases across 12 dimensions"),
        ("security_audit", MODERATE, "STRIDE + OWASP red-team code audit"),
        ("delegate", MODERATE, "Delegate a task to a sub-agent"),
        ("delegate_parallel", MODERATE, "Run multiple delegates in parallel"),
        ("spawn_workers", MODERATE, "Spawn parallel worker sub-agents"),
        ("swarm", MODERATE, "Run a multi-agent swarm"),
        ("execute_pipeline", MODERATE, "Execute a tool pipeline"),
        ("model_routing_config", SAFE, "Inspect or update model routing configuration"),
        ("model_switch", SAFE, "Switch the active model"),
        ("proxy_config", SAFE, "Inspect or update proxy configuration"),
        ("memory_store", MODERATE, "Store a long-term memory"),
        ("memory_recall", SAFE, "Recall stored memories"),
        ("memory_forget", MODERATE, "Forget a stored memory"),
        ("memory_export", SAFE, "Export memories to disk"),
        ("memory_purge", MODERATE, "Purge stored memories"),
        ("knowledge", SAFE, "Query the knowledge graph"),
        ("mcp_resources_list", SAFE, "List resources from MCP servers"),
        ("mcp_resources_read", SAFE, "Read a resource from an MCP server"),
        ("task_create", SAFE, "Create a background task"),
        ("task_get", SAFE, "Fetch the status of a background task"),
        ("task_update", SAFE, "Update a background task"),
        ("task_list", SAFE, "List background tasks"),
        ("task_output", SAFE, "Read the output of a background task"),
        ("task_stop", MODERATE, "Stop a background task"),
        ("sessions_list", SAFE, "List recent sessions"),
        ("sessions_history", SAFE, "Read session history"),
        ("sessions_search", SAFE, "Search a session's history for a keyword"),
        ("sessions_send", MODERATE, "Send a message to another session"),
        #[cfg(feature = "office-docs")]
        (
            "document_convert",
            MODERATE,
            "Generate a real xlsx/csv/docx/md/html/pdf file from structured content",
        ),
        #[cfg(feature = "office-docs")]
        (
            "pdf_ops",
            MODERATE,
            "Native PDF page operations: merge/split/extract/delete/rotate/info",
        ),
        #[cfg(feature = "office-docs")]
        (
            "presentation_create",
            MODERATE,
            "Create a real .pptx presentation from a slide outline",
        ),
        ("web_search_tool", SAFE, "Search the web with the default provider"),
        ("web_fetch", SAFE, "Fetch content from a URL"),
        ("multi_search", SAFE, "Search the web across multiple providers"),
        ("tavily_search", SAFE, "Search the web via Tavily"),
        ("exa_search", SAFE, "Search the web via Exa"),
        ("github_search", SAFE, "Search GitHub for code or issues"),
        (
            "github_advanced_search",
            SAFE,
            "Advanced GitHub search with the full advanced-search qualifier surface (owners/repos/language/stars/forks/license/path/extension/topic/state/labels/comments/author/reviewed-by …)",
        ),
        #[cfg(feature = "tool-workspace-deep")]
        (
            "workspace_deep_search",
            SAFE,
            "Local workspace DeepSearch with planner + lexical/structural/fuzzy recall and paragraph-level traced output",
        ),
        #[cfg(feature = "tool-curator")]
        (
            "enter_curator_mode",
            SAFE,
            "Enter Curator mode (extensive research + DOCX-grade document drafting)",
        ),
        #[cfg(feature = "tool-curator")]
        (
            "exit_curator_mode",
            SAFE,
            "Exit Curator mode and persist final.md/impl_blueprint.md/final.docx",
        ),
        #[cfg(feature = "tool-curator")]
        (
            "curator_collect",
            SAFE,
            "Persist research notes / sources for the active Curator session",
        ),
        #[cfg(feature = "tool-curator")]
        (
            "curator_template_list",
            SAFE,
            "List bundled Curator document templates (paper / solution / tech_report)",
        ),
        #[cfg(feature = "tool-curator")]
        (
            "curator_template_apply",
            SAFE,
            "Apply a bundled Curator template to a draft document",
        ),
        #[cfg(feature = "tool-curator")]
        (
            "curator_git_reference",
            MODERATE,
            "Shallow-clone remote git repositories into the active Curator session as reference projects (writes [Gn] entries to sources.md and structured excerpts to research_notes.md)",
        ),
        #[cfg(feature = "tool-curator")]
        (
            "curator_local_reference",
            SAFE,
            "Register local in-workspace project directories as Curator references (writes [Ln] entries to sources.md and structured excerpts to research_notes.md)",
        ),
        ("http_request", MODERATE, "Make an arbitrary HTTP request"),
        ("text_browser", SAFE, "Open a URL in the text browser"),
        ("browser", MODERATE, "Drive the visual browser"),
        ("debug_test_report", SAFE, "Author a structured QA/debug Markdown report from start → cases → findings → screenshots → finalize"),
        ("ask_question", SAFE, "Ask the user a question"),
        ("ask_user", SAFE, "Ask the user a question and wait for the reply"),
        ("escalate_to_human", MODERATE, "Escalate the conversation to a human"),
        ("send_user_message", SAFE, "Send a message to the user"),
        ("claude_code", MODERATE, "Drive the Claude Code CLI"),
        ("claude_code_runner", MODERATE, "Run a sub-task with Claude Code"),
        ("codex_cli", MODERATE, "Drive the Codex CLI"),
        ("gemini_cli", MODERATE, "Drive the Gemini CLI"),
        ("opencode_cli", MODERATE, "Drive the configured external agentic coding CLI"),
        ("backup", MODERATE, "Take a workspace backup"),
        ("workspace", SAFE, "Manage workspaces"),
        ("vi_verify", SAFE, "Run the verifiable-intent check"),
        #[cfg(feature = "tool-image")]
        ("view_image", SAFE, "View an image file"),
        ("send_message", SAFE, "Send a message to another channel"),
    ];
    for (name, risk, desc) in enhanced_entries {
        m.insert(name, ToolTierEntry::new(ENHANCED, SURFACE_BOTH, *risk, desc));
    }

    let on_demand_entries: &[(&str, ToolRiskLevel, &str)] = &[
        #[cfg(feature = "tool-cron")]
        ("cron_add", HIGH_RISK, "Schedule a new cron job"),
        #[cfg(feature = "tool-cron")]
        ("cron_list", HIGH_RISK, "List existing cron jobs"),
        #[cfg(feature = "tool-cron")]
        ("cron_remove", HIGH_RISK, "Remove a cron job"),
        #[cfg(feature = "tool-cron")]
        ("cron_update", HIGH_RISK, "Update a cron job"),
        #[cfg(feature = "tool-cron")]
        ("cron_run", HIGH_RISK, "Run a cron job immediately"),
        #[cfg(feature = "tool-cron")]
        ("cron_runs", HIGH_RISK, "List cron job runs"),
        #[cfg(feature = "tool-cron")]
        ("schedule", HIGH_RISK, "Schedule a one-shot job"),
        #[cfg(feature = "tool-sop")]
        ("sop_list", HIGH_RISK, "List standard operating procedures"),
        #[cfg(feature = "tool-sop")]
        ("sop_execute", HIGH_RISK, "Execute a standard operating procedure"),
        #[cfg(feature = "tool-sop")]
        ("sop_advance", HIGH_RISK, "Advance a SOP step"),
        #[cfg(feature = "tool-sop")]
        ("sop_approve", HIGH_RISK, "Approve a SOP step"),
        #[cfg(feature = "tool-sop")]
        ("sop_status", HIGH_RISK, "Check the status of a SOP run"),
        #[cfg(feature = "tool-team")]
        ("team_create", HIGH_RISK, "Create a team registry entry"),
        #[cfg(feature = "tool-team")]
        ("team_delete", HIGH_RISK, "Delete a team registry entry"),
        #[cfg(feature = "tool-reports")]
        ("report_template", HIGH_RISK, "Apply a report template"),
        ("project_intel", HIGH_RISK, "Run a project intelligence sweep"),
        #[cfg(feature = "tool-cloud-ops")]
        ("cloud_ops", HIGH_RISK, "Perform a cloud operation"),
        #[cfg(feature = "tool-cloud-ops")]
        ("cloud_patterns", HIGH_RISK, "Look up cloud patterns"),
        #[cfg(feature = "tool-cloud-ops")]
        ("data_management", HIGH_RISK, "Run a data management operation"),
        #[cfg(feature = "tool-cloud-ops")]
        ("security_ops", HIGH_RISK, "Run a security operations action"),
        #[cfg(feature = "tool-linkedin")]
        ("linkedin", HIGH_RISK, "Post or interact via LinkedIn"),
        #[cfg(feature = "tool-pushover")]
        ("pushover", HIGH_RISK, "Send a Pushover push notification"),
        #[cfg(feature = "tool-search-social")]
        ("discord_search", MODERATE, "Search Discord history"),
        #[cfg(feature = "tool-search-social")]
        ("reddit_search", MODERATE, "Search Reddit"),
        #[cfg(feature = "tool-search-social")]
        ("youtube_search", MODERATE, "Search YouTube"),
        #[cfg(feature = "tool-productivity")]
        ("composio", HIGH_RISK, "Run a Composio integration action"),
        #[cfg(feature = "tool-productivity")]
        ("google_workspace", HIGH_RISK, "Interact with Google Workspace"),
        ("microsoft365", HIGH_RISK, "Interact with Microsoft 365"),
        #[cfg(feature = "tool-productivity")]
        ("jira", HIGH_RISK, "Interact with Jira"),
        #[cfg(feature = "tool-productivity")]
        ("notion", HIGH_RISK, "Interact with Notion"),
        #[cfg(feature = "tool-utility-misc")]
        ("calculator", SAFE, "Evaluate a math expression"),
        #[cfg(feature = "tool-utility-misc")]
        ("weather", SAFE, "Look up the weather"),
        #[cfg(feature = "tool-image")]
        ("image_search", MODERATE, "Search the web for images"),
        #[cfg(feature = "tool-image")]
        ("image_info", SAFE, "Inspect image metadata"),
        ("reaction", HIGH_RISK, "Send a reaction in a channel"),
        ("poll", HIGH_RISK, "Run a poll in a channel"),
    ];
    for (name, risk, desc) in on_demand_entries {
        m.insert(name, ToolTierEntry::new(ON_DEMAND, SURFACE_BOTH, *risk, desc));
    }

    let desktop_pref: &[(&str, ToolRiskLevel, &str)] = &[
        #[cfg(feature = "tool-image")]
        ("screenshot", MODERATE, "Capture a screenshot"),
        #[cfg(feature = "tool-utility-misc")]
        ("canvas", MODERATE, "Generate canvas-rendered output"),
        #[cfg(feature = "tool-image")]
        ("image_gen", HIGH_RISK, "Generate an image from text"),
    ];
    for (name, risk, desc) in desktop_pref {
        m.insert(name, ToolTierEntry::new(ENHANCED, SURFACE_DESKTOP, *risk, desc));
    }

    let cli_pref: &[(&str, ToolRiskLevel, &str)] = &[
        (
            "browser_open",
            MODERATE,
            "Open a URL in the external system browser (Chrome/Edge/Safari); on desktop prefer the built-in `browser` tool",
        ),
        (
            "browser_delegate",
            MODERATE,
            "Delegate a browser task to an external browser-capable CLI; on desktop prefer the built-in `browser` tool",
        ),
    ];
    for (name, risk, desc) in cli_pref {
        m.insert(name, ToolTierEntry::new(ENHANCED, SURFACE_CLI, *risk, desc));
    }

    m
});

pub fn classify(name: &str, surface: ToolSurfaceBaseline) -> ToolTierEntry {
    if let Some(entry) = TOOL_TIERS.get(name) {
        if matches!(surface, ToolSurfaceBaseline::Cli)
            && matches!(entry.surface_baseline, ToolSurfaceBaseline::Desktop)
        {
            return ToolTierEntry::new(
                BuiltinToolTier::OnDemand,
                entry.surface_baseline,
                entry.risk,
                entry.description,
            );
        }
        if matches!(surface, ToolSurfaceBaseline::Desktop)
            && matches!(entry.surface_baseline, ToolSurfaceBaseline::Cli)
        {
            return ToolTierEntry::new(
                BuiltinToolTier::OnDemand,
                entry.surface_baseline,
                entry.risk,
                entry.description,
            );
        }
        return *entry;
    }
    ToolTierEntry::new(
        BuiltinToolTier::Enhanced,
        ToolSurfaceBaseline::Both,
        ToolRiskLevel::Safe,
        "",
    )
}

pub fn is_eager_tier(tier: BuiltinToolTier) -> bool {
    matches!(tier, BuiltinToolTier::Core | BuiltinToolTier::Enhanced)
}

pub fn partition_for_llm<'a>(
    tools: &'a [Box<dyn Tool>],
    surface: ToolSurfaceBaseline,
) -> (Vec<&'a Box<dyn Tool>>, Vec<&'a Box<dyn Tool>>) {
    let mut kept: Vec<&'a Box<dyn Tool>> = Vec::with_capacity(tools.len());
    let mut deferred: Vec<&'a Box<dyn Tool>> = Vec::new();
    for tool in tools {
        let name = tool.name();
        if name == "tool_search" || name.contains("__") || name.starts_with("custom_") {
            kept.push(tool);
            continue;
        }
        let entry = classify(name, surface);
        if is_eager_tier(entry.tier) {
            kept.push(tool);
        } else {
            deferred.push(tool);
        }
    }
    (kept, deferred)
}

pub fn risk_for(name: &str, surface: ToolSurfaceBaseline) -> ToolRiskLevel {
    classify(name, surface).risk
}

pub fn tier_for(name: &str, surface: ToolSurfaceBaseline) -> BuiltinToolTier {
    classify(name, surface).tier
}

pub fn known_tool_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = TOOL_TIERS.keys().copied().collect();
    names.sort_unstable();
    names
}

pub fn deferred_loading_effective(config: &crate::config::Config) -> (bool, bool) {
    let override_on = config.agent.tool_eager_override;
    let builtin_enabled = config.agent.builtin_tool_deferred_loading && !override_on;
    let mcp_enabled = config.mcp.deferred_loading && !override_on;
    (builtin_enabled, mcp_enabled)
}

pub fn build_deferred_builtin_set_for_surface(
    tools_registry: &[Box<dyn Tool>],
    surface: ToolSurfaceBaseline,
) -> crate::tools::mcp::deferred::DeferredBuiltinToolSet {
    let mut set = crate::tools::mcp::deferred::DeferredBuiltinToolSet::new();
    let (_, deferred) = partition_for_llm(tools_registry, surface);
    for tool_box in deferred {
        set.add_spec(tool_box.spec());
    }
    set
}

pub struct BuiltinDeferredRegistrationOptions<'a> {
    pub workspace_key: String,
    pub allowlist: Vec<String>,
    pub gate: Option<crate::security::permissions::ToolActivationGateHandle>,
    pub config: Option<&'a crate::config::Config>,
}

impl Default for BuiltinDeferredRegistrationOptions<'_> {
    fn default() -> Self {
        Self {
            workspace_key: String::new(),
            allowlist: Vec::new(),
            gate: None,
            config: None,
        }
    }
}

pub fn apply_builtin_deferred_registration(
    tools_registry: &mut Vec<Box<dyn Tool>>,
    deferred_section: &mut String,
    surface: ToolSurfaceBaseline,
    activated_handle: &mut Option<
        std::sync::Arc<parking_lot::Mutex<crate::tools::ActivatedToolSet>>,
    >,
) -> crate::tools::mcp::deferred::DeferredBuiltinToolSet {
    apply_builtin_deferred_registration_with_options(
        tools_registry,
        deferred_section,
        surface,
        activated_handle,
        BuiltinDeferredRegistrationOptions::default(),
    )
}

pub fn apply_builtin_deferred_registration_with_options(
    tools_registry: &mut Vec<Box<dyn Tool>>,
    deferred_section: &mut String,
    surface: ToolSurfaceBaseline,
    activated_handle: &mut Option<
        std::sync::Arc<parking_lot::Mutex<crate::tools::ActivatedToolSet>>,
    >,
    options: BuiltinDeferredRegistrationOptions<'_>,
) -> crate::tools::mcp::deferred::DeferredBuiltinToolSet {
    let deferred_builtin_set = build_deferred_builtin_set_for_surface(tools_registry, surface);
    if deferred_builtin_set.is_empty() {
        return deferred_builtin_set;
    }
    tracing::info!(
        "Builtin deferred ({}): {} tool stub(s)",
        surface.as_str(),
        deferred_builtin_set.len()
    );
    let builtin_section = crate::tools::mcp::deferred::build_deferred_builtin_section_with_surface(
        &deferred_builtin_set,
        surface,
    );
    if !deferred_section.is_empty() {
        deferred_section.push('\n');
    }
    deferred_section.push_str(&builtin_section);

    let handle = match activated_handle {
        Some(handle) => std::sync::Arc::clone(handle),
        None => {
            let new_handle = std::sync::Arc::new(parking_lot::Mutex::new(
                crate::tools::ActivatedToolSet::new(),
            ));
            *activated_handle = Some(std::sync::Arc::clone(&new_handle));
            new_handle
        }
    };

    let preserved_mcp_set = tools_registry
        .iter()
        .find(|t| t.name() == "tool_search")
        .and_then(|t| t.as_any())
        .and_then(|any| any.downcast_ref::<crate::tools::ToolSearchTool>())
        .map(|existing| existing.deferred_mcp_set())
        .unwrap_or_else(|| crate::tools::DeferredMcpToolSet {
            stubs: Vec::new(),
            registry: std::sync::Arc::new(crate::tools::McpRegistry::empty()),
        });

    tools_registry.retain(|t| t.name() != "tool_search");
    let tool = crate::tools::ToolSearchTool::new(preserved_mcp_set, handle)
        .with_builtin(deferred_builtin_set.clone())
    .with_surface(surface)
    .with_workspace_key(options.workspace_key)
    .with_allowlist(options.allowlist)
    .with_gate(options.gate);
    tools_registry.push(Box::new(tool));

    deferred_builtin_set
}
