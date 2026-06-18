// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::docx::render_docx;
use super::state::{
    CuratorActive, CuratorState, CuratorTemplateKind, PendingCurator, PendingCuratorPayload,
};
use super::templates::{list_summary, template_for};
use crate::security::SecurityPolicy;
use crate::tools::plan_mode::enter::PlanModeFlag;
use crate::tools::traits::{Tool, ToolResult};
use crate::tools::web::fetch::WebFetchTool;
use crate::tools::web::search::tool::WebSearchTool;
use async_trait::async_trait;
use futures_util::StreamExt;
use parking_lot::RwLock;
use regex::Regex;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

pub type CuratorModeFlag = Arc<CuratorModeRegistry>;

#[derive(Default)]
pub struct CuratorModeRegistry {
    active: RwLock<std::collections::HashSet<String>>,
}

impl CuratorModeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    fn session_key() -> String {
        crate::session::current_session_context()
            .map(|c| c.session_id)
            .unwrap_or_else(|| "default".to_string())
    }

    pub fn set_active(&self, active: bool) {
        let key = Self::session_key();
        let mut guard = self.active.write();
        if active {
            guard.insert(key);
        } else {
            guard.remove(&key);
        }
    }

    pub fn is_active(&self) -> bool {
        let key = Self::session_key();
        self.active.read().contains(&key)
    }
}

static SOURCE_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[S(\d+)\]").expect("source id regex"));

static REF_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([SGL])(\d+)\]").expect("ref id regex"));

static PATH_LINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)`?[A-Za-z0-9_./\\-]+\.(?:go|java|kt|py|rs|c|cpp|cc|h|hpp|cs|js|jsx|ts|tsx|swift|rb|php|scala|m|mm|dart|lua|hs|ex|exs|erl)`?\s*:\s*\d+(?:\s*[-–]\s*\d+)?",
    )
    .expect("path:line regex")
});

static OSS_BRAND_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:one[-\s]?api|new[-\s]?api|newswapi|litellm|openrouter|portkey|sen\s?api|vllm|fastchat|langchain|llama\.cpp|ollama|ray\s?serve|tritoninfere?nce|tgi|text-generation-inference)\b",
    )
    .expect("oss brand regex")
});

static FUNC_SIGNATURE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^\s*(?:func\s+\w+\s*\(|def\s+\w+\s*\(|fn\s+\w+\s*\(|public\s+(?:static\s+)?\w[\w<>\[\]]*\s+\w+\s*\(|class\s+\w+\s*[\(:{])",
    )
    .expect("function signature regex")
});

pub struct EnterCuratorModeTool {
    flag: CuratorModeFlag,
    state: CuratorState,
    workspace_root: Arc<RwLock<PathBuf>>,
}

impl EnterCuratorModeTool {
    pub fn new(
        flag: CuratorModeFlag,
        state: CuratorState,
        workspace_root: Arc<RwLock<PathBuf>>,
    ) -> Self {
        Self {
            flag,
            state,
            workspace_root,
        }
    }
}

#[async_trait]
impl Tool for EnterCuratorModeTool {
    fn name(&self) -> &str {
        "enter_curator_mode"
    }

    fn description(&self) -> &str {
        "Activate Curator mode and materialise the `<workspace>/.senweavercoding/curators/<slug>/` directory with placeholder \
         research_notes.md, sources.md, draft.md, final.md, impl_blueprint.md files. Must be \
         called before `curator_collect` / `curator_template_apply` / `exit_curator_mode`."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "intent": { "type": "string", "description": "Concise restatement of the user's goal (1–3 sentences)." },
                "template": {
                    "type": "string",
                    "enum": [
                        "paper_imrad","paper_apa","paper_mla","paper_chicago","paper_gb7714",
                        "solution_functional",
                        "solution_gb8567_2006","solution_gb8567_1988",
                        "solution_ieee830","solution_iso29148","solution_iso42010","solution_ieee1016","solution_iso12207",
                        "tech_report",
                        "paper","solution"
                    ],
                    "description": "Target document template. Aliases: `paper` → paper_imrad, `solution` → solution_functional."
                },
                "slug": { "type": "string", "description": "Optional explicit slug for the `.senweavercoding/curators/<slug>/` directory. Multiple parallel curator sessions in the same workspace are supported  -  each one lives under its own slug; if the chosen slug already exists, a numeric suffix is appended automatically (e.g. `my-doc`, `my-doc-2`, `my-doc-3`)." }
            },
            "required": ["intent"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let intent = args
            .get("intent")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("enter_curator_mode requires non-empty 'intent'"))?
            .to_string();
        let template = args
            .get("template")
            .and_then(|v| v.as_str())
            .map(CuratorTemplateKind::from_str_loose)
            .unwrap_or_default();
        let explicit_slug = args.get("slug").and_then(|v| v.as_str()).map(str::trim);
        let workspace = self.workspace_root.read().clone();
        let workspace = if workspace.as_os_str().is_empty() {
            std::env::current_dir()?
        } else {
            workspace
        };
        let base_slug = explicit_slug
            .filter(|s| !s.is_empty())
            .map(slugify)
            .unwrap_or_else(|| slugify(&intent));
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let init = {
            let workspace = workspace.clone();
            let intent_seed = intent.clone();
            let now_seed = now.clone();
            tokio::task::spawn_blocking(move || {
                enter_curator_init(workspace, base_slug, template, &intent_seed, &now_seed)
            })
            .await
            .map_err(|e| anyhow::anyhow!("enter_curator_mode internal task error: {e}"))??
        };
        let curator_root = init.curator_root;
        let slug = init.slug;

        for (p, bytes) in &init.created {
            crate::agent::file_edit_emitter::emit_file_create(p, bytes, None).await;
        }

        self.flag.set_active(true);
        self.state.set(CuratorActive {
            slug: slug.clone(),
            intent: intent.clone(),
            template,
            root_dir: curator_root.clone(),
            started_at: now.clone(),
        });

        let rel = pathdiff_or_self(&curator_root, &workspace);
        let output = format!(
            "Curator mode active. slug=`{slug}` template={} root=`{}` (relative `{}`).\n\n\
             Files initialised:\n\
             - {rel}/research_notes.md\n\
             - {rel}/sources.md\n\
             - {rel}/draft.md\n\
             - {rel}/final.md\n\
             - {rel}/impl_blueprint.md\n\n\
             Workflow: Intent → Web Collect (`web_search` / `web_fetch`) → \
             Local Collect (`workspace_deep_search`) → Organize → Draft → Polish → \
             `exit_curator_mode(slug=\"{slug}\", template=\"{}\")`.",
            init.kind_label,
            curator_root.display(),
            rel,
            init.kind_label
        );
        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}

pub struct CuratorCollectTool {
    state: CuratorState,
    security: Arc<SecurityPolicy>,
}

impl CuratorCollectTool {
    pub fn new(state: CuratorState, security: Arc<SecurityPolicy>) -> Self {
        Self { state, security }
    }
}

#[async_trait]
impl Tool for CuratorCollectTool {
    fn name(&self) -> &str {
        "curator_collect"
    }

    fn description(&self) -> &str {
        "Append a research note or external source to the active Curator session. \
         For external sources set kind=\"source\" with title/url/note. For workspace notes set \
         kind=\"note\" with path/lines/excerpt/commentary."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": ["source", "note"],
                    "description": "`source` appends to sources.md; `note` appends to research_notes.md."
                },
                "title": { "type": "string", "description": "Source title (for kind=source)." },
                "url": { "type": "string", "description": "Source URL (for kind=source)." },
                "author": { "type": "string" },
                "published_at": { "type": "string" },
                "note": { "type": "string", "description": "Free-form commentary." },
                "path": { "type": "string", "description": "Workspace path (for kind=note)." },
                "lines": { "type": "string", "description": "Line range, e.g. 12-45 (for kind=note)." },
                "excerpt": { "type": "string", "description": "Verbatim excerpt (for kind=note)." },
                "commentary": { "type": "string", "description": "Why this excerpt matters (for kind=note)." },
                "tags": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["kind"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let kind = args
            .get("kind")
            .and_then(|v| v.as_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        let active = self
            .state
            .get()
            .ok_or_else(|| anyhow::anyhow!("curator_collect requires an active Curator session (call enter_curator_mode first)."))?;
        ensure_inside_curator(&active.root_dir, &self.security)?;
        let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let mut payload = String::new();
        let target_file = match kind.as_str() {
            "source" => {
                let title = args
                    .get("title")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("curator_collect kind=source requires non-empty 'title'"))?;
                let url = args
                    .get("url")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("curator_collect kind=source requires non-empty 'url'"))?;
                let id = {
                    let root = active.root_dir.clone();
                    tokio::task::spawn_blocking(move || next_source_id(&root))
                        .await
                        .map_err(|e| anyhow::anyhow!("curator_collect internal task error: {e}"))??
                };
                payload.push_str(&format!(
                    "## {id}  -  {title}\n- URL: <{url}>\n",
                ));
                if let Some(author) = args.get("author").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()) {
                    payload.push_str(&format!("- Author: {author}\n"));
                }
                if let Some(pub_at) = args.get("published_at").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()) {
                    payload.push_str(&format!("- Published: {pub_at}\n"));
                }
                if let Some(tags) = collect_str_array(args.get("tags")) {
                    payload.push_str(&format!("- Tags: {}\n", tags.join(", ")));
                }
                if let Some(note) = args.get("note").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()) {
                    payload.push_str(&format!("- Note: {note}\n"));
                }
                payload.push_str(&format!("- Captured: {timestamp}\n\n"));
                active.root_dir.join("sources.md")
            }
            "note" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                let lines = args.get("lines").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty());
                let excerpt = args
                    .get("excerpt")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                let commentary = args.get("commentary").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty());
                if path.is_none() && excerpt.is_none() && commentary.is_none() && args.get("note").is_none() {
                    anyhow::bail!(
                        "curator_collect kind=note requires at least one of 'path', 'excerpt', or 'commentary'."
                    );
                }
                let header = match (path, lines) {
                    (Some(p), Some(l)) => format!("## `{p}:{l}`"),
                    (Some(p), None) => format!("## `{p}`"),
                    _ => format!("## Research note @ {timestamp}"),
                };
                payload.push_str(&header);
                payload.push('\n');
                if let Some(c) = commentary {
                    payload.push_str(&format!("- Commentary: {c}\n"));
                }
                if let Some(tags) = collect_str_array(args.get("tags")) {
                    payload.push_str(&format!("- Tags: {}\n", tags.join(", ")));
                }
                if let Some(ex) = excerpt {
                    payload.push_str("\n```text\n");
                    payload.push_str(ex);
                    if !ex.ends_with('\n') {
                        payload.push('\n');
                    }
                    payload.push_str("```\n");
                }
                if let Some(extra) = args.get("note").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()) {
                    payload.push_str(&format!("\n{extra}\n"));
                }
                payload.push('\n');
                active.root_dir.join("research_notes.md")
            }
            other => {
                anyhow::bail!(
                    "Unknown curator_collect kind '{other}'. Expected 'source' or 'note'."
                );
            }
        };
        let (before_bytes, after_bytes) = {
            let target = target_file.clone();
            let payload = payload.clone();
            tokio::task::spawn_blocking(move || -> anyhow::Result<(Option<Vec<u8>>, Option<Vec<u8>>)> {
                let before = std::fs::read(&target).ok();
                append_file(&target, &payload)?;
                let after = std::fs::read(&target).ok();
                Ok((before, after))
            })
            .await
            .map_err(|e| anyhow::anyhow!("curator_collect internal task error: {e}"))??
        };
        if let Some(after) = after_bytes.as_deref() {
            crate::agent::file_edit_emitter::emit_file_edit(
                &target_file,
                before_bytes.as_deref(),
                Some(after),
                None,
            )
            .await;
        }
        Ok(ToolResult {
            success: true,
            output: format!(
                "Appended {} bytes to `{}`.",
                payload.len(),
                target_file.display()
            ),
            error: None,
        })
    }
}

pub struct CuratorDeepCollectTool {
    state: CuratorState,
    security: Arc<SecurityPolicy>,
    web_search: Arc<WebSearchTool>,
    web_fetch: Arc<WebFetchTool>,
}

impl CuratorDeepCollectTool {
    pub fn new(
        state: CuratorState,
        security: Arc<SecurityPolicy>,
        web_search: Arc<WebSearchTool>,
        web_fetch: Arc<WebFetchTool>,
    ) -> Self {
        Self {
            state,
            security,
            web_search,
            web_fetch,
        }
    }
}

#[async_trait]
impl Tool for CuratorDeepCollectTool {
    fn name(&self) -> &str {
        "curator_deep_collect"
    }

    fn description(&self) -> &str {
        "Curator-only deep research pipeline: runs web_search with multi-engine fan-out, picks the \
         top N (default 5) URLs, fetches each page through web_fetch (with Jina/Firecrawl \
         fallback for JS-heavy sites), appends a long excerpt of each page to research_notes.md \
         and registers a [Sn] entry in sources.md. Use this as the primary collection action \
         instead of issuing search + fetch + curator_collect manually for every URL."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query (required)." },
                "category": {
                    "type": "string",
                    "enum": ["web", "academic", "code", "cn", "news", "social"],
                    "description": "Search category routed to web_search."
                },
                "time_range": {
                    "type": "string",
                    "enum": ["day", "week", "month", "year"],
                    "description": "Freshness filter for engines that support it."
                },
                "locale": { "type": "string", "description": "Optional locale hint." },
                "max_sources": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 12,
                    "description": "Top URLs to fetch and append (default 5)."
                },
                "max_results": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 30,
                    "description": "Search-side result cap (default 12)."
                },
                "snippet_chars": {
                    "type": "integer",
                    "minimum": 400,
                    "maximum": 12000,
                    "description": "How much body text to keep per page in research_notes.md (default 3500)."
                },
                "tags": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("curator_deep_collect requires non-empty 'query'"))?
            .to_string();
        let active = self
            .state
            .get()
            .ok_or_else(|| anyhow::anyhow!(
                "curator_deep_collect requires an active Curator session (call enter_curator_mode first)."
            ))?;
        ensure_inside_curator(&active.root_dir, &self.security)?;

        let max_sources = args
            .get("max_sources")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(5)
            .clamp(1, 12);
        let max_results = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(12)
            .clamp(max_sources, 30);
        let snippet_chars = args
            .get("snippet_chars")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(3500)
            .clamp(400, 12000);
        let tags = collect_str_array(args.get("tags"));

        let mut search_args = json!({
            "query": query,
            "multi": true,
            "max_results": max_results,
        });
        if let Some(category) = args.get("category").and_then(|v| v.as_str()) {
            search_args["category"] = json!(category);
        }
        if let Some(time_range) = args.get("time_range").and_then(|v| v.as_str()) {
            search_args["time_range"] = json!(time_range);
        }
        if let Some(locale) = args.get("locale").and_then(|v| v.as_str()) {
            search_args["locale"] = json!(locale);
        }

        let hits = self
            .web_search
            .search_hits(search_args)
            .await
            .map_err(|e| anyhow::anyhow!("curator_deep_collect search failed: {e}"))?;

        if hits.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "curator_deep_collect: web_search returned no hits for '{query}'"
                )),
            });
        }

        let mut seen_urls = std::collections::HashSet::new();
        let mut selected: Vec<_> = Vec::new();
        for hit in hits.iter() {
            if hit.url.trim().is_empty() {
                continue;
            }
            if !seen_urls.insert(hit.url.clone()) {
                continue;
            }
            selected.push(hit.clone());
            if selected.len() >= max_sources {
                break;
            }
        }
        if selected.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "curator_deep_collect: no usable URLs in search results for '{query}'"
                )),
            });
        }

        let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let mut summary_lines: Vec<String> = Vec::new();
        let mut total_appended: usize = 0;
        let mut failures: Vec<String> = Vec::new();

        let notes_path = active.root_dir.join("research_notes.md");
        let sources_path = active.root_dir.join("sources.md");
        let session_header = format!(
            "\n\n## Deep collect @ {timestamp}  -  query `{query}`\n- max_sources: {max_sources}\n- search hits: {}\n\n",
            hits.len()
        );
        let (notes_before, notes_after_header) = {
            let notes_path = notes_path.clone();
            let session_header = session_header.clone();
            tokio::task::spawn_blocking(
                move || -> anyhow::Result<(Option<Vec<u8>>, Option<Vec<u8>>)> {
                    let before = std::fs::read(&notes_path).ok();
                    append_file(&notes_path, &session_header)?;
                    let after = std::fs::read(&notes_path).ok();
                    Ok((before, after))
                },
            )
            .await
            .map_err(|e| anyhow::anyhow!("curator_deep_collect internal task error: {e}"))??
        };
        if let Some(after) = notes_after_header.as_deref() {
            crate::agent::file_edit_emitter::emit_file_edit(
                &notes_path,
                notes_before.as_deref(),
                Some(after),
                None,
            )
            .await;
        }
        total_appended += session_header.len();

        let tool_timeout_secs = crate::services::try_get_services()
            .and_then(|svc| svc.config().pacing.tool_timeout_secs)
            .filter(|s| *s > 0)
            .unwrap_or(600);
        let fetch_budget = std::time::Duration::from_secs(
            tool_timeout_secs
                .saturating_mul(3)
                .saturating_div(4)
                .clamp(60, 480),
        );
        let deadline = tokio::time::Instant::now() + fetch_budget;
        const PER_FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(75);
        const FETCH_CONCURRENCY: usize = 3;

        enum FetchOutcome {
            Body(String),
            Failed(String),
            Skipped,
        }

        let fetch_inputs: Vec<(usize, String, String)> = selected
            .iter()
            .enumerate()
            .map(|(idx, hit)| {
                let label = if hit.engine.is_empty() {
                    "search".to_string()
                } else {
                    hit.engine.clone()
                };
                (idx, hit.url.clone(), label)
            })
            .collect();
        let web_fetch = self.web_fetch.clone();
        let mut fetched: Vec<(usize, FetchOutcome)> = futures_util::stream::iter(
            fetch_inputs.into_iter().map(|(idx, url, label)| {
                let web_fetch = web_fetch.clone();
                let position = idx + 1;
                async move {
                    let now = tokio::time::Instant::now();
                    if now >= deadline {
                        tracing::warn!(
                            target: "tools.curator_deep_collect",
                            position,
                            url = %url,
                            "skipping fetch: deep-collect time budget exhausted"
                        );
                        return (idx, FetchOutcome::Skipped);
                    }
                    tracing::info!(
                        target: "tools.curator_deep_collect",
                        position,
                        engine = %label,
                        url = %url,
                        "fetching"
                    );
                    let remaining = deadline.saturating_duration_since(now);
                    let this_timeout = PER_FETCH_TIMEOUT.min(remaining);
                    let fetch_args = json!({ "url": url });
                    let exec = crate::agent::loop_::execute_tool_panic_safe(
                        web_fetch.as_ref(),
                        "web_fetch",
                        fetch_args,
                    );
                    let outcome = match tokio::time::timeout(this_timeout, exec).await {
                        Ok(Ok(r)) => {
                            if r.success {
                                let body = r.output.trim().to_string();
                                if body.is_empty() {
                                    FetchOutcome::Failed("empty body".to_string())
                                } else {
                                    FetchOutcome::Body(body)
                                }
                            } else {
                                FetchOutcome::Failed(
                                    r.error.unwrap_or_else(|| "unknown error".to_string()),
                                )
                            }
                        }
                        Ok(Err(e)) => FetchOutcome::Failed(format!("web_fetch threw: {e}")),
                        Err(_) => {
                            tracing::warn!(
                                target: "tools.curator_deep_collect",
                                position,
                                url = %url,
                                timeout_secs = this_timeout.as_secs(),
                                "fetch exceeded per-source timeout; moving on"
                            );
                            FetchOutcome::Failed(format!(
                                "fetch timed out after {}s",
                                this_timeout.as_secs()
                            ))
                        }
                    };
                    (idx, outcome)
                }
            }),
        )
        .buffer_unordered(FETCH_CONCURRENCY)
        .collect()
        .await;
        fetched.sort_by_key(|(idx, _)| *idx);

        let mut skipped = 0usize;
        for (idx, outcome) in fetched {
            let hit = &selected[idx];
            let position = idx + 1;
            let label = if hit.engine.is_empty() { "search" } else { hit.engine.as_str() };

            let body = match outcome {
                FetchOutcome::Body(b) => b,
                FetchOutcome::Skipped => {
                    skipped += 1;
                    summary_lines.push(format!(
                        "  {position}. ⏭ [{label}] {}  -  skipped (time budget reached)",
                        hit.title
                    ));
                    continue;
                }
                FetchOutcome::Failed(reason) => {
                    failures.push(format!("{} ({}): {reason}", hit.title, hit.url));
                    summary_lines.push(format!(
                        "  {position}. ✗ [{label}] {}  -  fetch failed: {reason}",
                        hit.title
                    ));
                    continue;
                }
            };

            let persist = {
                let root_dir = active.root_dir.clone();
                let sources_path = sources_path.clone();
                let notes_path = notes_path.clone();
                let title = hit.title.clone();
                let url = hit.url.clone();
                let label = label.to_string();
                let description = hit.description.clone();
                let tags = tags.clone();
                let query = query.clone();
                let timestamp = timestamp.clone();
                let body_owned = body.to_string();
                tokio::task::spawn_blocking(move || -> anyhow::Result<DeepHitPersist> {
                    let trimmed: String = body_owned.chars().take(snippet_chars).collect();
                    let truncated = body_owned.chars().count() > snippet_chars;

                    let source_id = next_source_id(&root_dir)?;
                    let mut source_entry = String::new();
                    source_entry.push_str(&format!("## {source_id}  -  {title}\n"));
                    source_entry.push_str(&format!("- URL: <{url}>\n"));
                    source_entry.push_str(&format!("- Engine: {label}\n"));
                    if !description.trim().is_empty() {
                        source_entry.push_str(&format!("- Search snippet: {}\n", description.trim()));
                    }
                    if let Some(tag_list) = tags.as_ref() {
                        source_entry.push_str(&format!("- Tags: {}\n", tag_list.join(", ")));
                    }
                    source_entry.push_str(&format!("- Captured: {timestamp}\n"));
                    source_entry.push_str(&format!(
                        "- Captured via: curator_deep_collect (query=`{query}`)\n\n"
                    ));
                    let sources_before = std::fs::read(&sources_path).ok();
                    append_file(&sources_path, &source_entry)?;
                    let sources_after = std::fs::read(&sources_path).ok();

                    let mut note_block = String::new();
                    note_block.push_str(&format!("### {source_id}  -  {title}\n"));
                    note_block.push_str(&format!("- URL: <{url}>\n"));
                    note_block.push_str(&format!("- Engine: {label}\n"));
                    if truncated {
                        note_block.push_str(&format!(
                            "- Excerpt ({} of {} chars):\n",
                            trimmed.chars().count(),
                            body_owned.chars().count()
                        ));
                    } else {
                        note_block.push_str(&format!(
                            "- Excerpt ({} chars):\n",
                            trimmed.chars().count()
                        ));
                    }
                    note_block.push_str("\n```text\n");
                    note_block.push_str(&trimmed);
                    if !trimmed.ends_with('\n') {
                        note_block.push('\n');
                    }
                    if truncated {
                        note_block.push_str(
                            "... [truncated; fetch the URL again to read the rest] ...\n",
                        );
                    }
                    note_block.push_str("```\n\n");
                    let notes_before = std::fs::read(&notes_path).ok();
                    append_file(&notes_path, &note_block)?;
                    let notes_after = std::fs::read(&notes_path).ok();

                    let summary_line = format!(
                        "  {position}. ✓ [{label}] {title}  -  {} chars (id={source_id})",
                        trimmed.chars().count()
                    );
                    Ok(DeepHitPersist {
                        sources_before,
                        sources_after,
                        notes_before,
                        notes_after,
                        appended: note_block.len() + source_entry.len(),
                        summary_line,
                    })
                })
                .await
                .map_err(|e| anyhow::anyhow!("curator_deep_collect internal task error: {e}"))??
            };

            if let Some(after) = persist.sources_after.as_deref() {
                crate::agent::file_edit_emitter::emit_file_edit(
                    &sources_path,
                    persist.sources_before.as_deref(),
                    Some(after),
                    None,
                )
                .await;
            }
            if let Some(after) = persist.notes_after.as_deref() {
                crate::agent::file_edit_emitter::emit_file_edit(
                    &notes_path,
                    persist.notes_before.as_deref(),
                    Some(after),
                    None,
                )
                .await;
            }
            total_appended += persist.appended;
            summary_lines.push(persist.summary_line);
        }

        let succeeded = summary_lines
            .iter()
            .filter(|l| l.contains("✓"))
            .count();
        let mut output = format!(
            "curator_deep_collect query=`{query}` fetched {succeeded}/{} sources ({} bytes appended).\n",
            selected.len(),
            total_appended
        );
        if skipped > 0 {
            output.push_str(&format!(
                "Note: {skipped} source(s) skipped to stay within the deep-collect time budget; re-run curator_deep_collect (or web_fetch) for the remaining URLs if needed.\n"
            ));
        }
        if !summary_lines.is_empty() {
            output.push_str("Sources:\n");
            output.push_str(&summary_lines.join("\n"));
            output.push('\n');
        }
        if !failures.is_empty() {
            output.push_str(&format!(
                "\nFailures ({}):\n  - {}\n",
                failures.len(),
                failures.join("\n  - ")
            ));
        }
        let success = succeeded > 0;
        Ok(ToolResult {
            success,
            output,
            error: if success {
                None
            } else {
                Some(format!(
                    "All {} candidate URLs failed to fetch usable content.",
                    selected.len()
                ))
            },
        })
    }
}

pub struct CuratorTemplateListTool;

impl Default for CuratorTemplateListTool {
    fn default() -> Self {
        Self
    }
}

impl CuratorTemplateListTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for CuratorTemplateListTool {
    fn name(&self) -> &str {
        "curator_template_list"
    }

    fn description(&self) -> &str {
        "List all bundled Curator document templates  -  5 academic paper styles (IMRaD / APA 7 / MLA 9 / Chicago 17-18 / GB/T 7714), 8 software solution standards (Functional / GB/T 8567-2006 / 1988 / IEEE 830 / ISO 29148 / ISO 42010 / IEEE 1016 / ISO 12207), and 1 technical report."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({"type": "object", "properties": {}, "required": []})
    }

    async fn execute(&self, _args: serde_json::Value) -> anyhow::Result<ToolResult> {
        Ok(ToolResult {
            success: true,
            output: list_summary(),
            error: None,
        })
    }
}

pub struct CuratorTemplateApplyTool {
    state: CuratorState,
    security: Arc<SecurityPolicy>,
}

impl CuratorTemplateApplyTool {
    pub fn new(state: CuratorState, security: Arc<SecurityPolicy>) -> Self {
        Self { state, security }
    }
}

#[async_trait]
impl Tool for CuratorTemplateApplyTool {
    fn name(&self) -> &str {
        "curator_template_apply"
    }

    fn description(&self) -> &str {
        "Reset the active Curator session's draft.md (and optionally impl_blueprint.md) to a bundled template. Use this when switching the document type during research."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "template": {
                    "type": "string",
                    "enum": [
                        "paper_imrad","paper_apa","paper_mla","paper_chicago","paper_gb7714",
                        "solution_functional",
                        "solution_gb8567_2006","solution_gb8567_1988",
                        "solution_ieee830","solution_iso29148","solution_iso42010","solution_ieee1016","solution_iso12207",
                        "tech_report",
                        "paper","solution"
                    ],
                    "description": "Template id to apply. Aliases: `paper` → paper_imrad, `solution` → solution_functional."
                },
                "include_blueprint": { "type": "boolean", "description": "Also reset impl_blueprint.md (default true)." },
                "force": { "type": "boolean", "description": "Overwrite even if draft.md was already edited (default false)." }
            },
            "required": ["template"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let template = args
            .get("template")
            .and_then(|v| v.as_str())
            .map(CuratorTemplateKind::from_str_loose)
            .ok_or_else(|| anyhow::anyhow!("curator_template_apply requires 'template'"))?;
        let include_blueprint = args
            .get("include_blueprint")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let force = args.get("force").and_then(|v| v.as_bool()).unwrap_or(false);
        let active = self
            .state
            .get()
            .ok_or_else(|| anyhow::anyhow!("curator_template_apply requires an active Curator session."))?;
        ensure_inside_curator(&active.root_dir, &self.security)?;
        let info = template_for(template);
        let kind_label = info.kind.label().to_string();
        let draft_bytes = compose_draft_with_banner(info.draft_markdown).into_bytes();
        let blueprint_bytes = info.blueprint_markdown.as_bytes().to_vec();
        let prep = {
            let root = active.root_dir.clone();
            tokio::task::spawn_blocking(move || -> anyhow::Result<TemplateApplyPrep> {
                let draft_path = root.join("draft.md");
                let existing = std::fs::read_to_string(&draft_path).unwrap_or_default();
                let already_edited = existing
                    .lines()
                    .filter(|l| !l.trim_start().is_empty())
                    .count()
                    > 15;
                if already_edited && !force {
                    return Ok(TemplateApplyPrep::AlreadyEdited);
                }
                let draft_before = std::fs::read(&draft_path).ok();
                std::fs::write(&draft_path, &draft_bytes)?;
                let blueprint = if include_blueprint {
                    let blueprint_path = root.join("impl_blueprint.md");
                    let blueprint_before = std::fs::read(&blueprint_path).ok();
                    std::fs::write(&blueprint_path, &blueprint_bytes)?;
                    Some((blueprint_path, blueprint_before, blueprint_bytes))
                } else {
                    None
                };
                Ok(TemplateApplyPrep::Applied {
                    draft_path,
                    draft_before,
                    draft_bytes,
                    blueprint,
                })
            })
            .await
            .map_err(|e| anyhow::anyhow!("curator_template_apply internal task error: {e}"))??
        };

        let applied = match prep {
            TemplateApplyPrep::AlreadyEdited => {
                anyhow::bail!(
                    "draft.md already contains substantive content. Pass force=true to overwrite, \
                     or merge manually."
                );
            }
            TemplateApplyPrep::Applied {
                draft_path,
                draft_before,
                draft_bytes,
                blueprint,
            } => {
                crate::agent::file_edit_emitter::emit_file_edit(
                    &draft_path,
                    draft_before.as_deref(),
                    Some(&draft_bytes),
                    None,
                )
                .await;
                let mut applied = vec!["draft.md".to_string()];
                if let Some((blueprint_path, blueprint_before, blueprint_bytes)) = blueprint {
                    crate::agent::file_edit_emitter::emit_file_edit(
                        &blueprint_path,
                        blueprint_before.as_deref(),
                        Some(&blueprint_bytes),
                        None,
                    )
                    .await;
                    applied.push("impl_blueprint.md".to_string());
                }
                applied
            }
        };

        self.state.set_template(template);
        Ok(ToolResult {
            success: true,
            output: format!("Applied template `{}` to: {}", kind_label, applied.join(", ")),
            error: None,
        })
    }
}

enum TemplateApplyPrep {
    AlreadyEdited,
    Applied {
        draft_path: std::path::PathBuf,
        draft_before: Option<Vec<u8>>,
        draft_bytes: Vec<u8>,
        blueprint: Option<(std::path::PathBuf, Option<Vec<u8>>, Vec<u8>)>,
    },
}

pub struct ExitCuratorModeTool {
    flag: CuratorModeFlag,
    plan_flag: PlanModeFlag,
    state: CuratorState,
    pending: PendingCurator,
    security: Arc<SecurityPolicy>,
}

impl ExitCuratorModeTool {
    pub fn new(
        flag: CuratorModeFlag,
        plan_flag: PlanModeFlag,
        state: CuratorState,
        pending: PendingCurator,
        security: Arc<SecurityPolicy>,
    ) -> Self {
        Self {
            flag,
            plan_flag,
            state,
            pending,
            security,
        }
    }
}

#[async_trait]
impl Tool for ExitCuratorModeTool {
    fn name(&self) -> &str {
        "exit_curator_mode"
    }

    fn description(&self) -> &str {
        "Finalize the active Curator session: persist final.md / impl_blueprint.md, render final.docx with the chosen template, and surface the document for the IDE's Build → Switch-to-Agent flow."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "final_content": {
                    "type": "string",
                    "description": "Polished Markdown body that will be persisted as final.md AND rendered into final.docx with the active template's typography. Must satisfy the curator quality gate (≥2400 chars, ≥4 top-level `## ` sections, and a citations/References section that maps to the `[Sn]/[Gn]/[Ln]` entries in sources.md)."
                },
                "impl_blueprint": {
                    "type": "string",
                    "description": "Polished Markdown body for impl_blueprint.md. Must satisfy the blueprint quality gate (≥600 chars, ≥3 `##` sections, at least one fenced ```bash/```sh code block)."
                },
                "summary": {
                    "type": "string",
                    "description": "Optional one-paragraph executive summary surfaced in the curator card."
                },
                "allow_docx_skip": {
                    "type": "boolean",
                    "description": "Reserved escape hatch. DOCX rendering is REQUIRED by default; only set to true when the user has explicitly accepted a Markdown-only deliverable. Setting it true marks the deliverable as degraded."
                }
            },
            "required": ["final_content", "impl_blueprint"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {

        let active = self
            .state
            .get()
            .ok_or_else(|| anyhow::anyhow!("exit_curator_mode requires an active Curator session."))?;
        ensure_inside_curator(&active.root_dir, &self.security)?;

        let final_content = args
            .get("final_content")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("exit_curator_mode requires non-empty 'final_content'"))?;
        let impl_blueprint = args
            .get("impl_blueprint")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("exit_curator_mode requires non-empty 'impl_blueprint'"))?;
        let summary = args
            .get("summary")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let allow_docx_skip = args
            .get("allow_docx_skip")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if let Err(reason) = quality_check(final_content, impl_blueprint) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(reason),
            });
        }

        if let Err(reason) = curator_content_style_check(final_content) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(reason),
            });
        }

        let evidence = {
            let root = active.root_dir.clone();
            tokio::task::spawn_blocking(move || curator_evidence_check(&root))
                .await
                .map_err(|e| anyhow::anyhow!("exit_curator_mode internal task error: {e}"))?
        };
        if let Err(reason) = evidence {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(reason),
            });
        }

        let final_path = active.root_dir.join("final.md");
        let blueprint_path = active.root_dir.join("impl_blueprint.md");
        let docx_path = active.root_dir.join("final.docx");

        let prep = {
            let final_path = final_path.clone();
            let blueprint_path = blueprint_path.clone();
            let docx_path = docx_path.clone();
            let final_owned = final_content.to_string();
            let blueprint_owned = impl_blueprint.to_string();
            let render_template = active.template;
            tokio::task::spawn_blocking(move || -> ExitPrep {
                let final_before = std::fs::read(&final_path).ok();
                let blueprint_before = std::fs::read(&blueprint_path).ok();
                if let Err(e) = std::fs::write(&final_path, &final_owned) {
                    return ExitPrep::WriteFailed(format!(
                        "exit_curator_mode REJECTED  -  failed to persist final.md at `{}`: {e}\n\nFix the IO error (permissions / disk space) and retry.",
                        final_path.display()
                    ));
                }
                if let Err(e) = std::fs::write(&blueprint_path, &blueprint_owned) {
                    let _ = std::fs::remove_file(&final_path);
                    return ExitPrep::WriteFailed(format!(
                        "exit_curator_mode REJECTED  -  failed to persist impl_blueprint.md at `{}`: {e}\n\nFix the IO error and retry.",
                        blueprint_path.display()
                    ));
                }

                let docx_before = std::fs::read(&docx_path).ok();
                let mut docx_ready = false;
                let mut docx_error: Option<String> = None;
                let mut docx_bytes: Option<Vec<u8>> = None;
                match render_docx(&final_owned, render_template, &docx_path) {
                    Ok(()) => match verify_docx_artifact(&docx_path) {
                        Ok(()) => {
                            docx_ready = true;
                            docx_bytes = Some(std::fs::read(&docx_path).unwrap_or_default());
                        }
                        Err(e) => {
                            docx_error = Some(format!(
                                "DOCX file `{}` failed post-write verification: {e}",
                                docx_path.display()
                            ));
                            let _ = std::fs::remove_file(&docx_path);
                        }
                    },
                    Err(e) => {
                        docx_error = Some(format!("DOCX renderer returned an error: {e}"));
                        let _ = std::fs::remove_file(&docx_path);
                    }
                }

                ExitPrep::Done {
                    final_before,
                    blueprint_before,
                    docx_before,
                    docx_ready,
                    docx_error,
                    docx_bytes,
                }
            })
            .await
            .map_err(|e| anyhow::anyhow!("exit_curator_mode internal task error: {e}"))?
        };

        let (final_before, blueprint_before, docx_before, docx_ready, docx_error, docx_bytes) =
            match prep {
                ExitPrep::WriteFailed(msg) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(msg),
                    });
                }
                ExitPrep::Done {
                    final_before,
                    blueprint_before,
                    docx_before,
                    docx_ready,
                    docx_error,
                    docx_bytes,
                } => (
                    final_before,
                    blueprint_before,
                    docx_before,
                    docx_ready,
                    docx_error,
                    docx_bytes,
                ),
            };

        if !docx_ready && !allow_docx_skip {
            tracing::warn!(
                target: "agent.curator_mode",
                detail = docx_error.as_deref().unwrap_or("unknown DOCX render failure"),
                "exit_curator_mode: DOCX export failed; finalizing the Markdown deliverable in degraded mode so the documents still complete instead of trapping the turn in an unwinnable retry loop"
            );
        }

        crate::agent::file_edit_emitter::emit_file_edit(
            &final_path,
            final_before.as_deref(),
            Some(final_content.as_bytes()),
            None,
        )
        .await;
        crate::agent::file_edit_emitter::emit_file_edit(
            &blueprint_path,
            blueprint_before.as_deref(),
            Some(impl_blueprint.as_bytes()),
            None,
        )
        .await;
        let final_docx_path_opt: Option<std::path::PathBuf> = if docx_ready {
            let docx_bytes_after = docx_bytes.unwrap_or_default();
            crate::agent::file_edit_emitter::emit_file_edit(
                &docx_path,
                docx_before.as_deref(),
                Some(&docx_bytes_after),
                None,
            )
            .await;
            Some(docx_path.clone())
        } else {
            None
        };

        self.flag.set_active(false);
        self.plan_flag.set(false);

        let payload = PendingCuratorPayload {
            slug: active.slug.clone(),
            template: active.template,
            final_md_path: display_path(&final_path),
            impl_blueprint_path: display_path(&blueprint_path),
            docx_path: final_docx_path_opt.as_ref().map(|p| display_path(p)),
            root_dir: display_path(&active.root_dir),
            final_md_body: final_content.to_string(),
            impl_blueprint_body: impl_blueprint.to_string(),
        };
        self.pending.set(payload);
        self.state.clear();

        let docx_line = if let Some(ref p) = final_docx_path_opt {
            format!("\nfinal.docx: `{}`", display_path(p))
        } else if allow_docx_skip {
            "\nfinal.docx: (skipped by request  -  Markdown-only deliverable)".to_string()
        } else {
            format!(
                "\nfinal.docx: (DOCX export unavailable  -  renderer error: {}; final.md and impl_blueprint.md were saved successfully)",
                docx_error.as_deref().unwrap_or("unknown render error")
            )
        };
        let summary_line = summary
            .as_ref()
            .map(|s| format!("\n\nExecutive summary: {s}"))
            .unwrap_or_default();
        let lead_line = if final_docx_path_opt.is_some() {
            "Exited Curator mode. All deliverables generated and verified."
        } else {
            "Exited Curator mode. Markdown deliverables generated and verified; the DOCX export was skipped (see note below) but the documents are complete."
        };
        let header = format!(
            "{lead_line}\n\
             final.md: `{}`\n\
             impl_blueprint.md: `{}`{docx_line}\n\
             Slug: `{}`  |  Template: `{}`{summary_line}\n\n\
             Awaiting user's Build click  -  DO NOT call any other tool now; the user will click \
             Build → Switch to start the engineering implementation in Agent mode, and that \
             implementation MUST mirror impl_blueprint.md verbatim.",
            display_path(&final_path),
            display_path(&blueprint_path),
            active.slug,
            active.template.label()
        );
        let envelope = format!(
            "{header}\n\n\
             ===CURATOR_MARKDOWN_BEGIN===\nslug: {}\ntemplate: {}\nfinal_md_path: {}\nimpl_blueprint_path: {}\n{}---\n{final_content}\n===CURATOR_MARKDOWN_END===",
            active.slug,
            active.template.label(),
            display_path(&final_path),
            display_path(&blueprint_path),
            final_docx_path_opt
                .as_ref()
                .map(|p| format!("docx_path: {}\n", display_path(p)))
                .unwrap_or_default()
        );
        Ok(ToolResult {
            success: true,
            output: envelope,
            error: None,
        })
    }
}

struct DeepHitPersist {
    sources_before: Option<Vec<u8>>,
    sources_after: Option<Vec<u8>>,
    notes_before: Option<Vec<u8>>,
    notes_after: Option<Vec<u8>>,
    appended: usize,
    summary_line: String,
}

enum ExitPrep {
    WriteFailed(String),
    Done {
        final_before: Option<Vec<u8>>,
        blueprint_before: Option<Vec<u8>>,
        docx_before: Option<Vec<u8>>,
        docx_ready: bool,
        docx_error: Option<String>,
        docx_bytes: Option<Vec<u8>>,
    },
}

fn verify_docx_artifact(path: &Path) -> Result<(), String> {
    let metadata = std::fs::metadata(path)
        .map_err(|e| format!("metadata read failed: {e}"))?;
    if !metadata.is_file() {
        return Err("not a regular file".to_string());
    }
    let size = metadata.len();
    if size < 256 {
        return Err(format!(
            "file size {size} bytes is below the minimum DOCX threshold (256). The renderer likely produced an empty or truncated archive."
        ));
    }
    let mut header = [0u8; 4];
    use std::io::Read;
    let mut f = std::fs::File::open(path)
        .map_err(|e| format!("open for header check failed: {e}"))?;
    f.read_exact(&mut header)
        .map_err(|e| format!("could not read first 4 bytes: {e}"))?;
    if &header != b"PK\x03\x04" {
        return Err(format!(
            "header is {:02X?}; expected `PK\\x03\\x04` (ZIP/DOCX magic). The renderer wrote a non-DOCX payload.",
            header
        ));
    }
    Ok(())
}

struct EnterInit {
    slug: String,
    curator_root: std::path::PathBuf,
    created: Vec<(std::path::PathBuf, Vec<u8>)>,
    kind_label: String,
}

fn enter_curator_init(
    workspace: std::path::PathBuf,
    base_slug: String,
    template: CuratorTemplateKind,
    intent: &str,
    now: &str,
) -> anyhow::Result<EnterInit> {
    let slug = uniquify_slug(&workspace, base_slug);
    let curators_base = curators_base_dir(&workspace);
    let curator_root = curators_base.join(&slug);
    std::fs::create_dir_all(&curator_root)?;
    write_placeholder(&curator_root, "research_notes.md", &research_seed(intent, now))?;
    write_placeholder(&curator_root, "sources.md", &sources_seed(intent, now))?;
    let tpl = template_for(template);
    let draft_with_banner = compose_draft_with_banner(tpl.draft_markdown);
    write_placeholder(&curator_root, "draft.md", &draft_with_banner)?;
    write_placeholder(
        &curator_root,
        "final.md",
        "# (final.md  -  populated by exit_curator_mode)\n",
    )?;
    write_placeholder(&curator_root, "impl_blueprint.md", tpl.blueprint_markdown)?;

    let placeholder_paths = [
        "research_notes.md",
        "sources.md",
        "draft.md",
        "final.md",
        "impl_blueprint.md",
    ];
    let mut created: Vec<(std::path::PathBuf, Vec<u8>)> = Vec::new();
    for name in placeholder_paths {
        let p = curator_root.join(name);
        if let Ok(bytes) = std::fs::read(&p) {
            created.push((p, bytes));
        }
    }

    Ok(EnterInit {
        slug,
        curator_root,
        created,
        kind_label: tpl.kind.label().to_string(),
    })
}

fn write_placeholder(root: &Path, name: &str, body: &str) -> anyhow::Result<()> {
    let path = root.join(name);
    if path.exists() {
        return Ok(());
    }
    std::fs::write(&path, body)?;
    Ok(())
}

const CURATOR_CONTENT_BANNER: &str = "\
> **Curator 内容硬约束（对 final.md 全文生效）**  \n\
> 1. 全文以**散文 + 表格 + 图示**为主，每个 `###` 子节正文 ≥2 段实质性描述（不允许只用 bullet 点拼凑）。  \n\
> 2. **禁止**任何 ```go```/```java```/```python```/```rust```/```c```/```cpp```/```csharp```/```js```/```ts```/```jsx```/```tsx```/```swift```/```ruby```/```php```/```scala```/```perl```/```dart```/```lua```/```haskell```/```elixir```/```erlang``` 等实现语言的源码块；如需逻辑示意，**至多 10 行 `text` 伪代码 或 Mermaid 图**。  \n\
> 3. **禁止** `path/file.ext:行号` 形式的源码引用；**禁止**裸贴 `func Foo(`/`def bar(`/`fn baz(`/`class Quux:` 等函数签名。  \n\
> 4. **禁止**在正文里直接点名具体开源项目（One-API / LiteLLM / OpenRouter / Portkey / vLLM / LangChain / llama.cpp / Ollama …），改用「某 Go 语言的 LLM 网关开源项目」「某 Python 多供应商 LLM 代理库」等中性描述；如确需对比，集中放在唯一一张「替代方案对比」表里，正文外提及 ≤3 次。  \n\
> 5. **允许**的代码块：```bash```/```sh```（部署/验收命令）、```yaml```/```toml```/```json```/```ini```/```nginx```/```dockerfile```（配置样本）、```mermaid```（图示）、```text```（≤10 行伪代码/Schema/EBNF）。  \n\
> 6. 写作重心：**功能描述（输入/输出/边界） · 技术原理（算法/协议/数据结构/关键参数） · 量化关键指标（带测试方法） · 数据与 API Schema · 实现要点（依赖类别/失败模式/重试/限流/可观测埋点） · 部署拓扑与运维**。\n\n\
---\n\n";

fn compose_draft_with_banner(draft_body: &str) -> String {
    let mut out = String::with_capacity(CURATOR_CONTENT_BANNER.len() + draft_body.len());
    out.push_str(CURATOR_CONTENT_BANNER);
    out.push_str(draft_body);
    out
}

fn append_file(path: &Path, payload: &str) -> anyhow::Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(payload.as_bytes())?;
    Ok(())
}

fn slugify(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut prev_dash = true;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "curator".to_string()
    } else {
        out.chars().take(60).collect()
    }
}

pub fn curators_base_dir(workspace: &Path) -> std::path::PathBuf {
    workspace.join(".senweavercoding").join("curators")
}

fn uniquify_slug(workspace: &Path, slug: String) -> String {
    let base = curators_base_dir(workspace);
    if !base.join(&slug).exists() {
        return slug;
    }
    for i in 2..200 {
        let candidate = format!("{slug}-{i}");
        if !base.join(&candidate).exists() {
            return candidate;
        }
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0);
    format!("{slug}-{stamp:x}")
}

pub(super) fn display_path(path: &Path) -> String {
    strip_extended_length_prefix(&path.to_string_lossy())
}

fn strip_extended_length_prefix(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = raw.strip_prefix(r"\\?\") {
        return rest.to_string();
    }
    raw.to_string()
}

fn pathdiff_or_self(target: &Path, base: &Path) -> String {
    target
        .strip_prefix(base)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| target.to_string_lossy().into_owned())
}

pub(super) fn ensure_inside_curator(root_dir: &Path, security: &SecurityPolicy) -> anyhow::Result<()> {
    let workspace = security.workspace_dir();
    let abs = std::fs::canonicalize(root_dir).unwrap_or_else(|_| root_dir.to_path_buf());
    let workspace_abs = std::fs::canonicalize(&workspace).unwrap_or(workspace);
    if !abs.starts_with(&workspace_abs) {
        anyhow::bail!(
            "Curator root '{}' is outside the workspace '{}'",
            abs.display(),
            workspace_abs.display()
        );
    }
    if !path_is_under_curators_dir(&abs) {
        anyhow::bail!(
            "Curator operations must target a path inside `.senweavercoding/curators/<slug>/` (got `{}`)",
            abs.display()
        );
    }
    Ok(())
}

pub fn path_is_under_curators_dir(path: &Path) -> bool {
    let comps: Vec<_> = path.components().collect();
    let target_root = std::ffi::OsStr::new(".senweavercoding");
    let target_dir = std::ffi::OsStr::new("curators");
    for window in comps.windows(2) {
        if let [std::path::Component::Normal(a), std::path::Component::Normal(b)] = window {
            if a.eq_ignore_ascii_case(target_root) && b.eq_ignore_ascii_case(target_dir) {
                return true;
            }
        }
    }
    false
}

fn research_seed(intent: &str, ts: &str) -> String {
    format!(
        "# Research Notes\n\n> Intent: {intent}\n> Started: {ts}\n\n## Open Questions\n- <…>\n\n## Working Hypotheses\n- <…>\n\n## Findings\n",
    )
}

fn sources_seed(intent: &str, ts: &str) -> String {
    format!(
        "# Sources\n\n> Intent: {intent}\n> Started: {ts}\n\n_Curator will append sources here. Each entry uses an `[Sn]` id._\n\n",
    )
}

fn next_source_id(root: &Path) -> anyhow::Result<String> {
    let path = root.join("sources.md");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut max_id = 0usize;
    for cap in SOURCE_ID_RE.captures_iter(&text) {
        if let Some(num) = cap.get(1).and_then(|m| m.as_str().parse::<usize>().ok()) {
            if num > max_id {
                max_id = num;
            }
        }
    }
    Ok(format!("[S{}]", max_id + 1))
}

fn collect_str_array(value: Option<&serde_json::Value>) -> Option<Vec<String>> {
    let arr = value?.as_array()?;
    let cleaned: Vec<String> = arr
        .iter()
        .filter_map(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
        .collect();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn curator_evidence_check(root: &Path) -> Result<(), String> {
    let sources_path = root.join("sources.md");
    let notes_path = root.join("research_notes.md");
    let sources_text = std::fs::read_to_string(&sources_path).unwrap_or_default();
    let notes_text = std::fs::read_to_string(&notes_path).unwrap_or_default();

    let mut web_source_ids: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut git_ref_ids: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut local_ref_ids: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for cap in REF_ID_RE.captures_iter(&sources_text) {
        let prefix = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        if let Some(full) = cap.get(0).map(|m| m.as_str().to_string()) {
            match prefix {
                "S" => {
                    web_source_ids.insert(full);
                }
                "G" => {
                    git_ref_ids.insert(full);
                }
                "L" => {
                    local_ref_ids.insert(full);
                }
                _ => {}
            }
        }
    }
    let web_source_count = web_source_ids.len();
    let git_ref_count = git_ref_ids.len();
    let local_ref_count = local_ref_ids.len();
    let total_ref_count = web_source_count + git_ref_count + local_ref_count;
    let notes_chars = notes_text.chars().count();

    let min_total_refs = 5usize;
    let min_notes_chars = 4000usize;
    let mut missing: Vec<String> = Vec::new();
    if total_ref_count < min_total_refs {
        missing.push(format!(
            "sources.md only registers {total_ref_count} references (≥ {min_total_refs} required). \
             Breakdown: {web_source_count} web `[Sn]`, {git_ref_count} git `[Gn]`, {local_ref_count} local `[Ln]`. \
             Grow it by running `curator_deep_collect`, `curator_collect(kind=\"source\")`, \
             `curator_git_reference` (remote repos), or `curator_local_reference` (in-workspace projects). \
             For local paragraph-level evidence run `workspace_deep_search(query=…)` and persist \
             useful chunks via `curator_collect(kind=\"note\", path=…, lines=…, excerpt=…, commentary=…)`."
        ));
    }
    if notes_chars < min_notes_chars {
        missing.push(format!(
            "research_notes.md is too thin ({notes_chars} chars; ≥ {min_notes_chars} required). \
             Capture more long excerpts via `curator_deep_collect`, `curator_git_reference`, \
             `curator_local_reference`, or `workspace_deep_search` + `curator_collect(kind=\"note\")`."
        ));
    }
    if missing.is_empty() {
        return Ok(());
    }
    let bullets = missing
        .iter()
        .map(|m| format!("  - {m}"))
        .collect::<Vec<_>>()
        .join("\n");
    Err(format!(
        "exit_curator_mode REJECTED  -  research evidence is insufficient:\n{bullets}\n\n\
         Concrete next steps:\n\
         1. Web evidence: run `curator_deep_collect` with 2–3 different query angles, then \
            follow up with targeted `web_fetch` + `curator_collect(kind=\"source\")` on any \
            high-value URLs the deep collect missed.\n\
         2. Git references: call `curator_git_reference(repos=[…])` to shallow-clone any \
            open-source projects the user supplied (or that you discovered) into the curator \
            workspace; each clone yields a `[Gn]` entry and a README/skeleton excerpt.\n\
         3. Local references: call `curator_local_reference(projects=[…])` for any reference \
            projects already sitting inside the current workspace; each entry yields a `[Ln]` \
            with metadata + key-source skeleton.\n\
         4. Local deep search: call `workspace_deep_search(query=…)` to mine paragraph-level \
            evidence from anywhere in the workspace, then persist the useful chunks via \
            `curator_collect(kind=\"note\", path=…, lines=…, excerpt=…, commentary=…)`.\n\
         5. Re-call `exit_curator_mode` once the gates are green."
    ))
}

fn curator_content_style_check(final_md: &str) -> Result<(), String> {
    const FORBIDDEN_CODE_LANGS: &[&str] = &[
        "go", "golang",
        "java", "kotlin", "kt",
        "python", "py",
        "rust", "rs",
        "c", "cpp", "c++", "cxx",
        "csharp", "cs", "c#",
        "javascript", "js", "jsx",
        "typescript", "ts", "tsx",
        "swift",
        "ruby", "rb",
        "php",
        "scala",
        "perl",
        "objective-c", "objc",
        "dart",
        "lua",
        "haskell", "hs",
        "elixir", "ex", "exs",
        "erlang", "erl",
    ];

    let mut forbidden_code_blocks: usize = 0;
    let mut total_code_lines: usize = 0;
    let mut oversized_text_blocks: usize = 0;
    let mut in_block = false;
    let mut current_lang = String::new();
    let mut current_lines: usize = 0;
    for line in final_md.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("```") {
            if !in_block {
                in_block = true;
                current_lang = rest.trim().to_ascii_lowercase();
                current_lines = 0;
            } else {
                let lang_key = current_lang.split_whitespace().next().unwrap_or("").to_string();
                if !lang_key.is_empty() && FORBIDDEN_CODE_LANGS.iter().any(|x| *x == lang_key) {
                    forbidden_code_blocks += 1;
                    total_code_lines += current_lines;
                } else if (lang_key.is_empty() || lang_key == "text" || lang_key == "pseudocode")
                    && current_lines > 14
                {
                    oversized_text_blocks += 1;
                }
                in_block = false;
                current_lang.clear();
                current_lines = 0;
            }
            continue;
        }
        if in_block {
            current_lines += 1;
        }
    }

    let path_line_hits = PATH_LINE_RE.find_iter(final_md).count();

    let oss_brand_hits = OSS_BRAND_RE.find_iter(final_md).count();

    let func_signature_re = &*FUNC_SIGNATURE_RE;
    let func_hits_outside_blocks = {
        let mut count = 0usize;
        let mut inside = false;
        for line in final_md.lines() {
            if line.trim_start().starts_with("```") {
                inside = !inside;
                continue;
            }
            if inside {
                continue;
            }
            if func_signature_re.is_match(line) {
                count += 1;
            }
        }
        count
    };

    let mut violations: Vec<String> = Vec::new();
    if forbidden_code_blocks > 0 {
        violations.push(format!(
            "final.md contains {forbidden_code_blocks} language-tagged source code block(s) totalling {total_code_lines} lines (e.g. ```go / ```python / ```rust ...). \
             Curator deliverables must describe **design, mechanism, and decisions in prose**, not paste implementation source. \
             Replace each block with: (a) ≥2 paragraphs of prose explaining the technical principle, \
             (b) a Mermaid flow / sequence diagram, or (c) ≤10 lines of pseudocode in a plain ```text``` block."
        ));
    }
    if oversized_text_blocks > 0 {
        violations.push(format!(
            "final.md has {oversized_text_blocks} oversized untagged / ```text``` code block(s) (>14 lines). \
             Pseudocode and schema sketches should be ≤10 lines; otherwise it is a verbatim source dump in disguise."
        ));
    }
    if path_line_hits > 0 {
        violations.push(format!(
            "final.md cites real source files {path_line_hits} time(s) in `path/file.ext:Lstart-Lend` form. \
             Curator documents must not point at external repositories at the file / line level  -  remove these citations and describe the behavior in prose instead."
        ));
    }
    if func_hits_outside_blocks > 1 {
        violations.push(format!(
            "final.md contains {func_hits_outside_blocks} function / class signature lines outside fenced blocks (e.g. `func Foo(`, `def bar(`, `fn baz(`, `class Quux:`). \
             These are leaked source  -  describe behavior in prose, not in language signatures."
        ));
    }
    if oss_brand_hits > 3 {
        violations.push(format!(
            "final.md mentions specific open-source product names {oss_brand_hits} times (e.g. One-API / LiteLLM / OpenRouter / vLLM / LangChain ...). \
             Keep them inside a single «Alternatives / Comparison» table only; elsewhere use generic descriptions (e.g. \"a Go-based LLM gateway open-source project\")."
        ));
    }

    if violations.is_empty() {
        return Ok(());
    }
    let bullets = violations
        .iter()
        .map(|m| format!("  - {m}"))
        .collect::<Vec<_>>()
        .join("\n");
    Err(format!(
        "exit_curator_mode REJECTED  -  Curator deliverables (paper / solution / tech_report) must \
         focus on functional design, mechanism, and decisions in prose, not implementation source:\n{bullets}\n\n\
         How to fix in one rewrite pass:\n\
         1. Delete every ```language``` source block; replace with ≥2 paragraphs of prose explaining the \
            **what / why / measurement** of that behavior.\n\
         2. Express algorithms / control flow via Mermaid diagrams or ≤10-line ```text``` pseudocode.\n\
         3. Remove all `path/file.ext:line` citations; describe the behavior in plain language.\n\
         4. Replace specific OSS project names with generic descriptions; only keep them inside a single \
            «Alternatives» comparison table.\n\
         5. Call `exit_curator_mode` again with the cleaned `final_content`."
    ))
}

fn quality_check(final_md: &str, blueprint_md: &str) -> Result<(), String> {
    let final_chars = final_md.chars().count();
    let blueprint_chars = blueprint_md.chars().count();
    let mut missing: Vec<String> = Vec::new();
    if final_chars < 2400 {
        missing.push(format!(
            "final.md too short ({final_chars} chars; expect ≥ 2400). The final document is the user-facing deliverable  -  a serious paper / solution / report needs depth, not a stub. Flesh out each section with substantive prose."
        ));
    }
    if blueprint_chars < 600 {
        missing.push(format!(
            "impl_blueprint.md too short ({blueprint_chars} chars; expect ≥ 600). The blueprint is the contract Agent mode will follow  -  include modules, build/run commands, and acceptance criteria."
        ));
    }
    let final_top_sections = final_md
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("## ") && !t.starts_with("### ")
        })
        .count();
    if final_top_sections < 4 {
        missing.push(format!(
            "final.md lacks top-level structure ({final_top_sections} `## ` sections; ≥4 required). A professional deliverable is organised into at least four top-level sections (e.g. Background / Approach / Design / Evaluation / References)."
        ));
    }
    let has_reference_section = final_md.lines().any(|l| {
        let t = l.trim_start().trim_start_matches('#').trim().to_ascii_lowercase();
        let is_heading = l.trim_start().starts_with('#');
        is_heading
            && (t.contains("references")
                || t.contains("bibliography")
                || t.contains("works cited")
                || t.contains("参考文献")
                || t.contains("参考资料"))
    });
    if !has_reference_section {
        missing.push(
            "final.md has no citations/References section. A professional, evidence-backed deliverable must close with a References / 参考文献 / Bibliography / Works Cited heading that lists the `[Sn]/[Gn]/[Ln]` sources gathered in sources.md."
                .to_string(),
        );
    }
    let blueprint_sections = blueprint_md
        .lines()
        .filter(|l| l.trim_start().starts_with("## "))
        .count();
    if blueprint_sections < 3 {
        missing.push(format!(
            "impl_blueprint.md lacks structural sections ({blueprint_sections} `## ` headings; ≥3 required: Scope / Decomposition / Verification)"
        ));
    }
    let has_blueprint_command = blueprint_md.contains("```bash") || blueprint_md.contains("```sh") || blueprint_md.contains("```cargo");
    if !has_blueprint_command {
        missing.push(
            "impl_blueprint.md is missing a fenced ```bash``` (or ```sh```) command block  -  Agent mode needs the verification commands written down."
                .to_string(),
        );
    }
    if missing.is_empty() {
        return Ok(());
    }
    let bullets = missing
        .iter()
        .map(|m| format!("  - {m}"))
        .collect::<Vec<_>>()
        .join("\n");
    Err(format!(
        "exit_curator_mode REJECTED  -  the submitted document is too thin to count as a finished Curator deliverable:\n{bullets}\n\n\
         Concrete next steps:\n\
         1. Continue collecting evidence with `web_search` / `workspace_deep_search` / \
            `curator_collect` until the gaps are filled.\n\
         2. Expand `draft.md` and then `final.md` with cited claims.\n\
         3. Enrich `impl_blueprint.md` so Agent mode can execute it verbatim (include Scope, \
            Decomposition, Data, Build & Run, Verification, Risks).\n\
         4. Re-call `exit_curator_mode` with the full content."
    ))
}
