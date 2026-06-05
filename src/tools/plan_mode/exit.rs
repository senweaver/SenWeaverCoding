// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::enter::PlanModeFlag;
use super::super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

pub type PendingPlan = Arc<RwLock<Option<String>>>;

pub fn new_pending_plan() -> PendingPlan {
    Arc::new(RwLock::new(None))
}

pub struct ExitPlanModeTool {
    flag: PlanModeFlag,
    pending_plan: PendingPlan,

    workspace_root: Arc<RwLock<PathBuf>>,
}

impl ExitPlanModeTool {
    pub fn new(flag: PlanModeFlag) -> Self {
        Self {
            pending_plan: new_pending_plan(),
            flag,
            workspace_root: Arc::new(RwLock::new(PathBuf::new())),
        }
    }

    pub fn with_workspace_root(mut self, workspace_root: Arc<RwLock<PathBuf>>) -> Self {
        self.workspace_root = workspace_root;
        self
    }

    fn resolve_workspace(&self) -> anyhow::Result<PathBuf> {
        let configured = self.workspace_root.read().clone();
        let initial = if !configured.as_os_str().is_empty() {
            configured
        } else {
            std::env::current_dir()?
        };
        Ok(reroute_if_tauri_dev(initial))
    }
}

fn reroute_if_tauri_dev(initial: PathBuf) -> PathBuf {
    use std::ffi::OsStr;

    let last_component = initial
        .file_name()
        .and_then(OsStr::to_str)
        .map(str::to_lowercase);
    let parent = initial.parent().map(std::path::Path::to_path_buf);

    if last_component.as_deref() == Some("src-tauri")
        && let Some(p) = parent.as_ref()
        && p.join("src-tauri").join("tauri.conf.json").exists()
    {
        tracing::warn!(
            target: "tools.exit_plan_mode",
            from = %initial.display(),
            to = %p.display(),
            "rerouting plan write away from Tauri-watched src-tauri/ to project root"
        );
        return p.clone();
    }

    initial
}

#[async_trait]
impl Tool for ExitPlanModeTool {
    fn name(&self) -> &str {
        "exit_plan_mode"
    }

    fn description(&self) -> &str {
        "Exit plan mode and provide the plan content. Returns to normal mode where all tools are available."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "plan_content": {
                    "type": "string",
                    "description": "The plan that was created during plan mode",
                },
            },
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let plan_text = args
            .get("plan_content")
            .and_then(|v| v.as_str())
            .map(|s| s.trim_end_matches('\u{0}'))
            .unwrap_or("");

        let trimmed = plan_text.trim();
        if let Err(reason) = quality_check(trimmed) {
            tracing::info!(
                target: "tools.exit_plan_mode",
                reason = %reason,
                "rejecting thin exit_plan_mode submission; asking model to plan for real"
            );
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(reason),
            });
        }

        *self.flag.write() = false;

        if let Some(svc) = crate::services::try_get_services() {
            svc.set_pending_plan(plan_text.to_string());
        } else {
            *self.pending_plan.write() = Some(plan_text.to_string());
        }

        let workspace = self.resolve_workspace();
        let plan_text_owned = plan_text.to_string();
        let write_outcome = tokio::task::spawn_blocking(move || {
            workspace.and_then(|w| write_plan_file_under(&w, &plan_text_owned))
        })
        .await
        .unwrap_or_else(|e| Err(anyhow::anyhow!("plan write task failed: {e}")));
        let (path_disp, wrapped_md, write_note) = match write_outcome {
            Ok((p, body)) => {
                let path_str = p.to_string_lossy().to_string();
                tracing::info!(
                    target: "tools.exit_plan_mode",
                    path = %path_str,
                    "Plan persisted to disk"
                );
                (path_str, body, String::new())
            }
            Err(e) => {
                tracing::warn!(
                    target: "tools.exit_plan_mode",
                    error = %e,
                    "Failed to persist plan to disk; falling back to in-memory only"
                );
                (
                    String::new(),
                    plan_text.to_string(),
                    format!(
                        "\n\n_Note: failed to persist plan file ({e}); plan kept in memory only._"
                    ),
                )
            }
        };

        let header = if path_disp.is_empty() {
            "Exited plan mode. Awaiting user's Build click.".to_string()
        } else {
            format!(
                "Exited plan mode. Plan saved to `{path_disp}`. \
                 Awaiting user's Build click  - DO NOT call any other tool now; \
                 the user will click Build / Switch to start execution in Agent mode."
            )
        };

        let output = format!(
            "{header}\n\n\
             ===PLAN_MARKDOWN_BEGIN===\n{wrapped_md}\n===PLAN_MARKDOWN_END==={write_note}"
        );

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}

fn write_plan_file_under(
    workspace: &std::path::Path,
    plan_text: &str,
) -> anyhow::Result<(std::path::PathBuf, String)> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let plans_dir = workspace.join(".senweavercoding").join("plans");
    if !plans_dir.exists() {
        std::fs::create_dir_all(&plans_dir)?;
    }

    let title = extract_plan_title(plan_text);
    let slug = slugify_for_filename(&title);
    let suffix_seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let suffix = format!("{:08x}", suffix_seed as u32);
    let filename = format!("{slug}_{suffix}.plan.md");
    let path = plans_dir.join(filename);

    let body = if plan_text.trim_start().starts_with("---") {
        plan_text.to_string()
    } else {
        wrap_with_frontmatter(&title, plan_text)
    };

    crate::util::atomic_write(&path, body.as_bytes())?;
    Ok((path, body))
}

fn extract_plan_title(plan_text: &str) -> String {
    for line in plan_text.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let stripped = t.trim_start_matches('#').trim();
        if !stripped.is_empty() {
            return stripped.chars().take(80).collect();
        }
    }
    "Plan".to_string()
}

fn slugify_for_filename(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut prev_dash = true;
    for ch in title.chars() {
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
        "plan".to_string()
    } else {
        out.chars().take(60).collect()
    }
}

fn wrap_with_frontmatter(title: &str, plan_text: &str) -> String {
    let overview = derive_overview(title, plan_text);

    let mut todos = detect_todos(plan_text);
    if todos.is_empty() {
        todos.push(format!("Execute: {title}"));
    }

    let safe_overview = yaml_quote(&overview);
    let safe_title = yaml_quote(title);

    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("name: {}\n", yaml_quote(&slugify_for_filename(title))));
    out.push_str(&format!("overview: {safe_overview}\n"));
    out.push_str("todos:\n");
    for (i, t) in todos.iter().enumerate() {
        out.push_str(&format!("  - id: t{}\n", i + 1));
        out.push_str(&format!("    content: {}\n", yaml_quote(t)));
        out.push_str("    status: pending\n");
    }
    out.push_str("isProject: false\n");
    out.push_str("---\n\n");
    if !plan_text.trim_start().starts_with('#') {
        out.push_str("# ");
        out.push_str(&safe_title.trim_matches('"').replace("\\\"", "\""));
        out.push_str("\n\n");
    }
    out.push_str(plan_text);
    if !plan_text.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn derive_overview(title: &str, plan_text: &str) -> String {
    let candidate = plan_text
        .lines()
        .map(str::trim)
        .find(|l| {
            !l.is_empty()
                && !l.starts_with('#')
                && !l.starts_with("---")
                && !l.starts_with("- ")
        })
        .unwrap_or("")
        .chars()
        .take(160)
        .collect::<String>();
    if candidate.trim().is_empty() {
        format!("Plan: {title}")
            .chars()
            .take(160)
            .collect::<String>()
    } else {
        candidate
    }
}

fn detect_todos(plan_text: &str) -> Vec<String> {
    let mut todos: Vec<String> = Vec::new();
    let mut in_yaml_todos_block = false;

    for raw in plan_text.lines() {
        let l = raw.trim();

        if l.starts_with("todos:") {
            in_yaml_todos_block = true;
            continue;
        }
        if in_yaml_todos_block {
            if let Some(rest) = l.strip_prefix("content:") {
                let cleaned = rest.trim().trim_matches('"').trim_matches('\'').to_string();
                if !cleaned.is_empty() && cleaned.chars().count() <= 200 {
                    todos.push(cleaned);
                    if todos.len() >= 25 {
                        return todos;
                    }
                }
                continue;
            }
            if !l.starts_with('-') && !l.starts_with("id:") && !l.starts_with("status:")
                && !l.starts_with("notes:") && !raw.starts_with(' ')
            {
                in_yaml_todos_block = false;
            } else {
                continue;
            }
        }

        let stripped = if let Some(rest) = l.strip_prefix("- [ ]") {
            Some(rest.trim().to_string())
        } else if let Some(rest) = l.strip_prefix("- [x]") {
            Some(rest.trim().to_string())
        } else if let Some(rest) = l.strip_prefix("- [X]") {
            Some(rest.trim().to_string())
        } else if let Some(rest) = l.strip_prefix("- ") {
            Some(rest.trim().to_string())
        } else if let Some(rest) = l.strip_prefix("* ") {
            Some(rest.trim().to_string())
        } else if l.len() >= 3
            && l.chars().next().is_some_and(|c| c.is_ascii_digit())
            && l.chars().nth(1).is_some_and(|c| c == '.' || c == ')')
        {
            Some(l[2..].trim().to_string())
        } else {
            None
        };
        if let Some(item) = stripped {
            if !item.is_empty() && item.chars().count() <= 200 {
                todos.push(item);
                if todos.len() >= 25 {
                    break;
                }
            }
        }
    }
    todos
}

fn quality_check(trimmed: &str) -> Result<(), String> {
    if trimmed.is_empty() {
        return Err(
            "exit_plan_mode requires a non-empty `plan_content` argument. Draft the full \
             canonical plan markdown (frontmatter + ## sections + >=3 todos with file refs) \
             and call exit_plan_mode again.".to_string(),
        );
    }

    const MIN_CHARS: usize = 600;
    const MIN_TODOS: usize = 3;

    let char_count = trimmed.chars().count();
    let todos = detect_todos(trimmed);
    let todo_count = todos.len();
    let section_count = trimmed
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("## ") || t.starts_with("### ")
        })
        .count();
    let has_file_refs = trimmed.contains("](")
        || trimmed.contains(".rs)")
        || trimmed.contains(".ts)")
        || trimmed.contains(".tsx)")
        || trimmed.contains(".go)")
        || trimmed.contains(".py)")
        || trimmed.contains(".js)")
        || trimmed.contains("file_path:")
        || trimmed.contains("path/to/");
    let has_code_fence = trimmed.contains("```");

    let mut missing: Vec<String> = Vec::new();
    if char_count < MIN_CHARS {
        missing.push(format!(
            "too short ({char_count} chars; need >={MIN_CHARS})"
        ));
    }
    if todo_count < MIN_TODOS {
        missing.push(format!(
            "too few concrete todos ({todo_count} detected; need >={MIN_TODOS})  - \
             a single \"Execute: <title>\" placeholder does NOT count, decompose \
             the work into per-file or per-track steps"
        ));
    }
    if section_count < 2 {
        missing.push(format!(
            "missing structural sections ({section_count} `## ` headings; \
             need at least `## Scope`, `## Track 1 - <title>`, and `## Verification`)"
        ));
    }
    if !has_file_refs {
        missing.push(
            "no file path references (use `[path/to/file.rs](path/to/file.rs)` \
             markdown links so the executor knows EXACTLY which files to touch)"
                .to_string(),
        );
    }
    if !has_code_fence {
        missing.push(
            "no fenced code block (the `## Verification` section MUST contain at least \
             one ```bash``` block listing the verification commands the \
             executor should run)"
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
        "exit_plan_mode REJECTED  - the supplied `plan_content` is too thin to be \
         a real plan.  Plan mode demands a canonical plan document  - \
         what's missing:\n{bullets}\n\n\
         Concrete next steps before retrying:\n\
         1. EXPLORE first using read-only tools  - `dir_list`, `glob_search`, \
            `content_search`/`grep`, `file_read`  - to enumerate every file the \
            task actually touches and read the entry points (e.g. `go.mod`, \
            `README.md`, `Dockerfile`, top-level configs).  For a project \
            rename: count the import sites with `content_search`, list every \
            `*.go` file, locate every `Dockerfile` / `docker-compose*.yml` / \
            `README*`.\n\
         2. DECOMPOSE the task into 5-10 ordered todos by file group, NOT a \
            single Execute line.  Each todo should be one verifiable \
            action (e.g. `Edit go.mod: replace module path`, \
            `Glob-replace all .go imports old to new (149 files)`, \
            `Update Dockerfile: ldflags path + binary name`).\n\
         3. WRITE the full canonical plan markdown body with a `## Scope` \
            section (linking each affected file via `[path](path)`), one \
            or more `## Track N - <title>` sections, and a `## Verification` \
            section with commands inside ```bash``` fences.\n\
         4. RE-CALL exit_plan_mode with the FULL markdown.  Do NOT submit a \
            stub again  - it will be refused until the bar above is met.  Do \
            NOT call any other mutating tool  - the user has not yet clicked \
            Build."
    ))
}

fn yaml_quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', " ");
    format!("\"{escaped}\"")
}

