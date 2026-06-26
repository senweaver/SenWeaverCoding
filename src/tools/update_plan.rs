// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

fn reroute_if_tauri_dev_for_plans(initial: PathBuf) -> PathBuf {
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
            target: "tools.update_plan",
            from = %initial.display(),
            to = %p.display(),
            "rerouting plan write away from Tauri-watched src-tauri/ to project root"
        );
        return p.clone();
    }

    initial
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: String,
    pub title: String,
    pub status: PlanStepStatus,
    pub notes: Option<String>,
    #[serde(default)]
    pub verify: Option<String>,
    #[serde(default)]
    pub expect: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PlanStepStatus {
    Pending,
    InProgress,
    Completed,
    Skipped,
}

impl std::fmt::Display for PlanStepStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Completed => write!(f, "completed"),
            Self::Skipped => write!(f, "skipped"),
        }
    }
}

pub type PlanHandle = Arc<RwLock<Vec<PlanStep>>>;

pub struct UpdatePlanTool {
    plan: PlanHandle,
    workspace_root: Arc<RwLock<PathBuf>>,
    active_plan_name: Arc<RwLock<Option<String>>>,
}

impl UpdatePlanTool {
    pub fn new(plan: PlanHandle) -> Self {
        Self {
            plan,
            workspace_root: Arc::new(RwLock::new(PathBuf::new())),
            active_plan_name: Arc::new(RwLock::new(None)),
        }
    }

    pub fn with_workspace_root(plan: PlanHandle, workspace_root: Arc<RwLock<PathBuf>>) -> Self {
        Self {
            plan,
            workspace_root,
            active_plan_name: Arc::new(RwLock::new(None)),
        }
    }

    async fn persist_active_plan_progress(&self) {
        let Some(plan_name) = self.active_plan_name.read().clone() else {
            return;
        };
        let file_path = self
            .plans_dir_snapshot()
            .join(format!("{plan_name}.plan.md"));
        let Ok(content) = tokio::fs::read_to_string(&file_path).await else {
            return;
        };
        let updated = {
            let plan = self.plan.read();
            rewrite_frontmatter_statuses(&content, &plan)
        };
        if updated != content {
            if let Err(e) =
                crate::util::atomic_write_async(file_path.clone(), updated.into_bytes()).await
            {
                tracing::warn!(
                    target: "tools.update_plan",
                    error = %e,
                    path = %file_path.display(),
                    "failed to persist plan progress to disk"
                );
            }
        }
    }

    fn plans_dir_snapshot(&self) -> PathBuf {
        let root = self.workspace_root.read().clone();
        if root.as_os_str().is_empty() {
            return PathBuf::new();
        }

        let safe_root = reroute_if_tauri_dev_for_plans(root);
        safe_root.join(".senweavercoding").join("plans")
    }

    async fn ensure_plans_dir(&self) -> anyhow::Result<()> {
        let plans_dir = self.plans_dir_snapshot();
        if !plans_dir.as_os_str().is_empty() {
            tokio::fs::create_dir_all(&plans_dir).await?;
        }
        Ok(())
    }

    fn render_plan_md(&self, title: &str, description: &str) -> String {
        let plan = self.plan.read();
        render_plan_frontmatter_doc(title, description, &plan)
    }

    fn parse_plan_md(content: &str) -> Vec<PlanStep> {
        if let Some(steps) = parse_plan_frontmatter(content) {
            if !steps.is_empty() {
                return steps;
            }
        }
        parse_plan_checkbox_list(content)
    }

    fn current_mode_is_plan() -> bool {
        matches!(
            crate::agent::coding_mode::active_coding_mode(),
            crate::agent::coding_mode::CodingMode::Plan
        )
    }

    fn render_plan_progress_header(total: usize, completed: usize) -> String {
        if Self::current_mode_is_plan() {
            format!("Plan draft  -  {total} todo(s) outlined")
        } else {
            format!("{total} To-dos ({completed}/{total} completed)")
        }
    }

    fn render_plan_load_header(file_path_display: String, count: usize, completed: usize) -> String {
        if Self::current_mode_is_plan() {
            format!("Loaded plan from `{file_path_display}` ({count} todo(s) outlined)")
        } else {
            format!(
                "Loaded plan from `{file_path_display}` ({count} steps, {completed} completed)"
            )
        }
    }
}

fn normalize_plan_key(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        }
    }
    out
}

fn parse_plan_frontmatter(content: &str) -> Option<Vec<PlanStep>> {
    let trimmed = content.trim_start_matches('\u{feff}');
    let mut lines = trimmed.lines();
    let first = lines.next()?;
    if first.trim() != "---" {
        return None;
    }
    let mut body = String::new();
    for line in lines {
        if line.trim() == "---" {
            return Some(parse_todos_section(&body));
        }
        body.push_str(line);
        body.push('\n');
    }
    None
}

fn parse_todos_section(body: &str) -> Vec<PlanStep> {
    let mut in_todos = false;
    let mut current: Option<PlanStep> = None;
    let mut out: Vec<PlanStep> = Vec::new();

    for line in body.lines() {
        let trimmed_line = line.trim_end();
        let trimmed = trimmed_line.trim();

        if !in_todos {
            if trimmed_line.trim_start() == trimmed_line && trimmed.starts_with("todos:") {
                in_todos = true;
            }
            continue;
        }

        if !line.starts_with(' ') && !line.starts_with('\t') && !trimmed.is_empty() {
            if let Some(s) = current.take() {
                out.push(s);
            }
            in_todos = false;
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("- id:") {
            if let Some(s) = current.take() {
                out.push(s);
            }
            current = Some(PlanStep {
                id: rest.trim().trim_matches('"').to_string(),
                title: String::new(),
                status: PlanStepStatus::Pending,
                notes: None,
                verify: None,
                expect: None,
            });
        } else if let Some(rest) = trimmed.strip_prefix("content:") {
            if let Some(s) = current.as_mut() {
                s.title = rest.trim().trim_matches('"').to_string();
            }
        } else if let Some(rest) = trimmed.strip_prefix("status:") {
            if let Some(s) = current.as_mut() {
                s.status = match rest.trim().trim_matches('"') {
                    "completed" => PlanStepStatus::Completed,
                    "in_progress" => PlanStepStatus::InProgress,
                    "cancelled" | "skipped" => PlanStepStatus::Skipped,
                    _ => PlanStepStatus::Pending,
                };
            }
        } else if let Some(rest) = trimmed.strip_prefix("verify:") {
            if let Some(s) = current.as_mut() {
                let v = rest.trim().trim_matches('"').to_string();
                s.verify = if v.is_empty() { None } else { Some(v) };
            }
        } else if let Some(rest) = trimmed.strip_prefix("expect:") {
            if let Some(s) = current.as_mut() {
                let v = rest.trim().trim_matches('"').to_string();
                s.expect = if v.is_empty() { None } else { Some(v) };
            }
        }
    }
    if let Some(s) = current {
        out.push(s);
    }
    out
}

fn parse_plan_checkbox_list(content: &str) -> Vec<PlanStep> {
    let mut steps: Vec<PlanStep> = Vec::new();
    let mut step_id = 1u32;

    for line in content.lines() {
        let trimmed = line.trim();
        let (status, title) = if let Some(rest) = trimmed.strip_prefix("- [x] ") {
            (PlanStepStatus::Completed, rest)
        } else if let Some(rest) = trimmed.strip_prefix("- [~] ") {
            (PlanStepStatus::InProgress, rest)
        } else if let Some(rest) = trimmed.strip_prefix("- [-] ") {
            (PlanStepStatus::Skipped, rest)
        } else if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
            (PlanStepStatus::Pending, rest)
        } else {
            if trimmed.starts_with("> ") && !steps.is_empty() {
                let note_text = trimmed.strip_prefix("> ").unwrap_or(trimmed);
                if let Some(last) = steps.last_mut() {
                    last.notes = Some(note_text.to_string());
                }
            }
            continue;
        };

        steps.push(PlanStep {
            id: format!("s{step_id}"),
            title: title.to_string(),
            status,
            notes: None,
            verify: None,
            expect: None,
        });
        step_id += 1;
    }
    steps
}

fn rewrite_frontmatter_statuses(content: &str, plan: &[PlanStep]) -> String {
    let status_for = |id_raw: &str| -> Option<&'static str> {
        let key = normalize_plan_key(id_raw);
        plan.iter()
            .find(|s| s.id == id_raw || (!key.is_empty() && normalize_plan_key(&s.id) == key))
            .map(|s| frontmatter_status(&s.status))
    };

    let mut out = String::with_capacity(content.len() + 32);
    let mut frontmatter_open = false;
    let mut frontmatter_done = false;
    let mut in_todos = false;
    let mut current_id: Option<String> = None;

    for (line_idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if !frontmatter_done {
            if trimmed == "---" {
                if frontmatter_open {
                    frontmatter_done = true;
                    in_todos = false;
                    current_id = None;
                } else if line_idx == 0 {
                    frontmatter_open = true;
                }
                out.push_str(line);
                out.push('\n');
                continue;
            }

            if frontmatter_open {
                let indented = line.starts_with(' ') || line.starts_with('\t');
                if !in_todos {
                    if !indented && trimmed.starts_with("todos:") {
                        in_todos = true;
                    }
                } else {
                    if !indented && !trimmed.is_empty() {
                        in_todos = false;
                        current_id = None;
                    } else if let Some(rest) = trimmed.strip_prefix("- id:") {
                        current_id = Some(rest.trim().trim_matches('"').to_string());
                    } else if trimmed.strip_prefix("status:").is_some() {
                        if let Some(id) = current_id.as_deref() {
                            if let Some(new_status) = status_for(id) {
                                let indent: String = line
                                    .chars()
                                    .take_while(|c| *c == ' ' || *c == '\t')
                                    .collect();
                                out.push_str(&indent);
                                out.push_str("status: ");
                                out.push_str(new_status);
                                out.push('\n');
                                continue;
                            }
                        }
                    }
                }
            }
        }

        out.push_str(line);
        out.push('\n');
    }

    if !content.ends_with('\n') {
        out.pop();
    }
    out
}

fn frontmatter_status(status: &PlanStepStatus) -> &'static str {
    match status {
        PlanStepStatus::Pending => "pending",
        PlanStepStatus::InProgress => "in_progress",
        PlanStepStatus::Completed => "completed",
        PlanStepStatus::Skipped => "cancelled",
    }
}

fn merge_steps_preserving_progress(existing: &[PlanStep], incoming: Vec<PlanStep>) -> Vec<PlanStep> {
    incoming
        .into_iter()
        .map(|mut step| {
            if step.status == PlanStepStatus::Pending {
                if let Some(prev) =
                    find_step_index(existing, &step.id, &step.title).map(|i| &existing[i])
                {
                    if prev.status != PlanStepStatus::Pending {
                        step.status = prev.status.clone();
                        if step.notes.is_none() {
                            step.notes = prev.notes.clone();
                        }
                    }
                }
            }
            step
        })
        .collect()
}

fn find_step_index(plan: &[PlanStep], step_id: &str, title_hint: &str) -> Option<usize> {
    if let Some(i) = plan.iter().position(|s| s.id == step_id) {
        return Some(i);
    }
    let key = normalize_plan_key(step_id);
    if !key.is_empty() {
        if let Some(i) = plan.iter().position(|s| normalize_plan_key(&s.id) == key) {
            return Some(i);
        }
    }
    if !title_hint.is_empty() {
        let title_key = normalize_plan_key(title_hint);
        if !title_key.is_empty() {
            if let Some(i) = plan
                .iter()
                .position(|s| normalize_plan_key(&s.title) == title_key)
            {
                return Some(i);
            }
            return plan.iter().position(|s| {
                let tk = normalize_plan_key(&s.title);
                !tk.is_empty() && (tk.contains(&title_key) || title_key.contains(&tk))
            });
        }
    }
    None
}

async fn run_acceptance_check(
    verify_cmd: &str,
    expect: Option<&str>,
    cwd: &std::path::Path,
) -> Result<(), String> {
    use std::process::Stdio;

    let (shell, flag) = if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };
    let mut cmd = crate::util::hidden_async_command(shell);
    cmd.arg(flag).arg(verify_cmd);
    if !cwd.as_os_str().is_empty() {
        cmd.current_dir(cwd);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("failed to spawn verify command `{verify_cmd}`: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        let combined = if stderr.trim().is_empty() {
            stdout.as_ref()
        } else {
            stderr.as_ref()
        };
        return Err(format!(
            "acceptance verify `{verify_cmd}` exited with code {code}: {}",
            tail_text(combined, 1500)
        ));
    }

    if let Some(exp) = expect.map(str::trim).filter(|e| !e.is_empty()) {
        if !stdout.contains(exp) && !stderr.contains(exp) {
            return Err(format!(
                "acceptance verify `{verify_cmd}` succeeded but output did not contain expected text {exp:?}"
            ));
        }
    }

    Ok(())
}

fn tail_text(s: &str, max_chars: usize) -> String {
    let total = s.chars().count();
    if total <= max_chars {
        return s.trim().to_string();
    }
    let start = total - max_chars;
    let tail: String = s.chars().skip(start).collect();
    format!("…{}", tail.trim())
}

fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_dash = false;
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
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
        out
    }
}

fn yaml_escape_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push(' '),
            _ => out.push(ch),
        }
    }
    out
}

pub(crate) fn render_plan_frontmatter(name: &str, overview: &str, todos: &[PlanStep]) -> String {
    let mut md = String::new();
    md.push_str("---\n");
    md.push_str(&format!("name: {}\n", slugify(name)));
    md.push_str(&format!("overview: \"{}\"\n", yaml_escape_quoted(overview)));
    md.push_str("todos:\n");
    if todos.is_empty() {
        md.push_str("  []\n");
    } else {
        for step in todos {
            let id = if step.id.trim().is_empty() {
                slugify(&step.title)
            } else {
                slugify(&step.id)
            };
            md.push_str(&format!("  - id: {}\n", id));
            md.push_str(&format!(
                "    content: \"{}\"\n",
                yaml_escape_quoted(&step.title)
            ));
            md.push_str(&format!("    status: {}\n", frontmatter_status(&step.status)));
            if let Some(verify) = step.verify.as_deref().filter(|v| !v.trim().is_empty()) {
                md.push_str(&format!("    verify: \"{}\"\n", yaml_escape_quoted(verify)));
            }
            if let Some(expect) = step.expect.as_deref().filter(|v| !v.trim().is_empty()) {
                md.push_str(&format!("    expect: \"{}\"\n", yaml_escape_quoted(expect)));
            }
        }
    }
    md.push_str("isProject: false\n");
    md.push_str("---\n");
    md
}

pub(crate) fn render_plan_frontmatter_doc(
    title: &str,
    overview: &str,
    todos: &[PlanStep],
) -> String {
    let total = todos.len();
    let completed = todos
        .iter()
        .filter(|s| s.status == PlanStepStatus::Completed)
        .count();
    let in_progress = todos
        .iter()
        .filter(|s| s.status == PlanStepStatus::InProgress)
        .count();

    let mut md = String::new();
    md.push_str(&render_plan_frontmatter(title, overview, todos));
    md.push('\n');
    md.push_str(&format!("# {}\n\n", title));
    if !overview.is_empty() {
        md.push_str(overview);
        md.push_str("\n\n");
    }

    md.push_str("## 工作量摸底\n\n");
    md.push_str(&format!(
        "- **任务范围**: 共 {total} 项待办（{completed} 已完成 / {in_progress} 进行中）。\n"
    ));
    md.push_str(
        "- **影响文件**: 待办条目内引用的所有路径（请用 `[path](path)` markdown 链接形式）。\n",
    );
    md.push_str("- **验收门**: `cargo check` / `cargo test` / 自定义脚本（见 `## 验收` 段）。\n\n");

    md.push_str("## Track 1  -  任务清单\n\n");
    if todos.is_empty() {
        md.push_str("> 计划尚未生成具体步骤。\n\n");
    } else {
        for (idx, step) in todos.iter().enumerate() {
            let marker = match step.status {
                PlanStepStatus::Completed => "[x]",
                PlanStepStatus::InProgress => "[~]",
                PlanStepStatus::Skipped => "[-]",
                PlanStepStatus::Pending => "[ ]",
            };
            md.push_str(&format!(
                "- {} **Step {}**: {}\n",
                marker,
                idx + 1,
                step.title
            ));
            if let Some(ref notes) = step.notes {
                md.push_str(&format!("  > {}\n", notes));
            }
        }
        md.push('\n');
    }

    md.push_str("## 验收\n\n");
    md.push_str("```bash\ncargo check --features gui\ncargo test --features gui\n```\n\n");

    md.push_str("## 流程图\n\n");
    md.push_str("```mermaid\nflowchart LR\n  A[Start] --> B[Implement Steps] --> C[Verify] --> D[Done]\n```\n");

    md
}

#[async_trait]
impl Tool for UpdatePlanTool {
    fn name(&self) -> &str {
        "update_plan"
    }

    fn description(&self) -> &str {
        "Create, update, query, save, and load structured task plans. \
Actions: 'set' (replace the whole plan), 'update' (flip ONE step's status/notes), 'get' (view current plan), \
'save' (persist as .plan.md file), 'load' (read .plan.md from disk), 'list' (list saved plans).\n\
\n\
LIFECYCLE RULES  -  read carefully, the user UI mirrors EVERY call:\n\
- Call 'set' exactly once at the very start of plan execution to seed the in-memory tracker. \
After that, prefer 'update' for status flips so completion is preserved.\n\
- For every step, the canonical execution sequence is THREE separate update_plan calls in this order:\n\
    1. update_plan(action=\"update\", step_id=<id>, status=\"in_progress\")  ← BEFORE you start the step\n\
    2. … perform the actual edits / shell commands for that one step …\n\
    3. update_plan(action=\"update\", step_id=<id>, status=\"completed\")     ← THE INSTANT it's done\n\
  Then move on to the next step's `in_progress` mark, and so on.\n\
- **NEVER batch status flips at the end of the run.** Do NOT do all the real work first \
and then fire a long sequence of `update_plan(action=\"update\", …)` calls back-to-back. \
The user is watching a live progress bar fed by these calls; batching makes the bar look \
frozen at 0/N until everything jumps to N/N at once, which is exactly what they DON'T want.\n\
- If a single piece of work genuinely closes multiple steps simultaneously, you MAY emit \
several `action=\"update\"` calls in a row  -  but ONLY because each step truly just finished, \
not as a deferred end-of-turn cleanup.\n\
- Use 'skipped' (with a `notes` reason) for steps that turn out unnecessary; do not silently \
leave them `pending`.\n\
- If you intend to add a brand-new todo that wasn't in the original plan, you MUST call \
'set' with the full updated list. Calling 'update' with a non-existent step_id will fail with a \
list of valid ids  -  fix the call, do not retry the same id.\n\
\n\
ID MATCHING\n\
- 'update' resolves step_id with case- and punctuation-insensitive matching, and falls back \
to matching against the step's `title` / `content` if you pass it. So minor formatting drift \
won't break updates, but inventing a new id (e.g. asking to update t18 when only t1..t16 exist) \
will."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "Action: 'set' (replace plan), 'update' (modify step), 'get' (view plan), 'save' (persist .plan.md), 'load' (read .plan.md), 'list' (list saved plans)",
                    "enum": ["set", "update", "get", "save", "load", "list"]
                },
                "steps": {
                    "type": "array",
                    "description": "Full plan steps (for 'set' action)",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string" },
                            "title": { "type": "string" },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed", "skipped"]
                            },
                            "notes": { "type": "string" },
                            "verify": {
                                "type": "string",
                                "description": "Optional shell command that objectively verifies this step is done (e.g. 'cargo check --lib'). When set, the step cannot be marked 'completed' unless this command exits 0."
                            },
                            "expect": {
                                "type": "string",
                                "description": "Optional substring that must appear in the verify command's output for the step to count as done."
                            }
                        },
                        "required": ["id", "title"]
                    }
                },
                "step_id": {
                    "type": "string",
                    "description": "Step ID to update (for 'update' action)"
                },
                "status": {
                    "type": "string",
                    "description": "New status for the step (for 'update' action)",
                    "enum": ["pending", "in_progress", "completed", "skipped"]
                },
                "notes": {
                    "type": "string",
                    "description": "Optional notes to attach to the step"
                },
                "verify": {
                    "type": "string",
                    "description": "Optional shell command that objectively verifies the step (used with action='update'). The step cannot be marked 'completed' unless this command exits 0."
                },
                "expect": {
                    "type": "string",
                    "description": "Optional substring required in the verify command's output (used with action='update')."
                },
                "plan_name": {
                    "type": "string",
                    "description": "Name for the plan file (for 'save'/'load'). Will be saved as <name>.plan.md"
                },
                "title": {
                    "type": "string",
                    "description": "Title heading for the plan document (for 'save')"
                },
                "description": {
                    "type": "string",
                    "description": "Description/summary for the plan document (for 'save')"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'action' parameter"))?;

        match action {
            "set" => {
                let steps_val = args
                    .get("steps")
                    .ok_or_else(|| anyhow::anyhow!("'set' requires 'steps' array"))?;
                let incoming: Vec<PlanStep> = serde_json::from_value(steps_val.clone())?;
                let merged = {
                    let existing = self.plan.read();
                    merge_steps_preserving_progress(&existing, incoming)
                };
                let count = merged.len();
                let retained = merged
                    .iter()
                    .filter(|s| s.status != PlanStepStatus::Pending)
                    .count();
                *self.plan.write() = merged;
                let output = if retained > 0 {
                    format!(
                        "Plan set with {count} steps ({retained} retained prior progress; \
                         use action=\"update\" from here, do not re-set)"
                    )
                } else {
                    format!("Plan set with {count} steps")
                };
                Ok(ToolResult {
                    success: true,
                    output,
                    error: None,
                })
            }
            "update" => {
                let step_id = args
                    .get("step_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("'update' requires 'step_id'"))?;
                let title_hint = args
                    .get("title")
                    .and_then(|v| v.as_str())
                    .or_else(|| args.get("content").and_then(|v| v.as_str()))
                    .unwrap_or("");

                let requested_status = args.get("status").and_then(|v| v.as_str());
                if requested_status == Some("completed") {
                    let verify_info = {
                        let plan = self.plan.read();
                        find_step_index(&plan, step_id, title_hint).and_then(|i| {
                            plan[i]
                                .verify
                                .as_ref()
                                .filter(|v| !v.trim().is_empty())
                                .map(|v| (v.clone(), plan[i].expect.clone(), plan[i].title.clone()))
                        })
                    };
                    if let Some((verify_cmd, expect, title)) = verify_info {
                        let cwd = self.workspace_root.read().clone();
                        if let Err(err) =
                            run_acceptance_check(&verify_cmd, expect.as_deref(), &cwd).await
                        {
                            return Ok(ToolResult {
                                success: false,
                                output: String::new(),
                                error: Some(format!(
                                    "Cannot mark step '{title}' completed: its acceptance check did not pass. {err}\n\
                                     Fix the underlying issue and retry, or mark the step 'skipped' with a `notes` reason if the check is no longer applicable."
                                )),
                            });
                        }
                    }
                }

                let update_outcome: Result<(String, String), String> = {
                    let mut plan = self.plan.write();

                    let mut idx: Option<usize> = plan.iter().position(|s| s.id == step_id);

                    if idx.is_none() {
                        let key = normalize_plan_key(step_id);
                        if !key.is_empty() {
                            idx = plan.iter().position(|s| normalize_plan_key(&s.id) == key);
                        }
                    }

                    if idx.is_none() && !title_hint.is_empty() {
                        let title_key = normalize_plan_key(title_hint);
                        if !title_key.is_empty() {
                            idx = plan
                                .iter()
                                .position(|s| normalize_plan_key(&s.title) == title_key);
                            if idx.is_none() {
                                idx = plan.iter().position(|s| {
                                    let tk = normalize_plan_key(&s.title);
                                    !tk.is_empty()
                                        && (tk.contains(&title_key) || title_key.contains(&tk))
                                });
                            }
                        }
                    }

                    match idx {
                        Some(i) => {
                            let s = &mut plan[i];
                            if let Some(status_str) = args.get("status").and_then(|v| v.as_str()) {
                                s.status = match status_str {
                                    "in_progress" => PlanStepStatus::InProgress,
                                    "completed" => PlanStepStatus::Completed,
                                    "skipped" => PlanStepStatus::Skipped,
                                    _ => PlanStepStatus::Pending,
                                };
                            }
                            if let Some(notes) = args.get("notes").and_then(|v| v.as_str()) {
                                s.notes = Some(notes.to_string());
                            }
                            if let Some(verify) = args.get("verify").and_then(|v| v.as_str()) {
                                s.verify = if verify.trim().is_empty() {
                                    None
                                } else {
                                    Some(verify.to_string())
                                };
                            }
                            if let Some(expect) = args.get("expect").and_then(|v| v.as_str()) {
                                s.expect = if expect.trim().is_empty() {
                                    None
                                } else {
                                    Some(expect.to_string())
                                };
                            }
                            Ok((s.title.clone(), s.status.to_string()))
                        }
                        None => {
                            let available = plan
                                .iter()
                                .map(|s| format!("'{}' (\"{}\")", s.id, s.title))
                                .collect::<Vec<_>>()
                                .join(", ");
                            let hint = if available.is_empty() {
                                "Plan is empty  -  call action='set' first to seed the todo list.".to_string()
                            } else {
                                format!(
                                    "Available steps: {available}. If this is a brand new todo, call action='set' with the FULL updated list (do not 'update' an id that does not exist)."
                                )
                            };
                            Err(format!("Step '{step_id}' not found in plan. {hint}"))
                        }
                    }
                };

                match update_outcome {
                    Ok((title, status)) => {
                        self.persist_active_plan_progress().await;
                        let output = if Self::current_mode_is_plan() {
                            format!("Plan todo '{title}' annotated (status={status}).")
                        } else {
                            format!("Updated step '{title}': status={status}")
                        };
                        Ok(ToolResult {
                            success: true,
                            output,
                            error: None,
                        })
                    }
                    Err(error_message) => Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(error_message),
                    }),
                }
            }
            "get" => {
                let plan = self.plan.read();
                if plan.is_empty() {
                    return Ok(ToolResult {
                        success: true,
                        output: "No plan set. Use action='set' to create one.".to_string(),
                        error: None,
                    });
                }

                let lines: Vec<String> = plan
                    .iter()
                    .map(|s| {
                        let checkbox = match s.status {
                            PlanStepStatus::Pending => "- [ ]",
                            PlanStepStatus::InProgress => "- [~]",
                            PlanStepStatus::Completed => "- [x]",
                            PlanStepStatus::Skipped => "- [-]",
                        };
                        let notes = s
                            .notes
                            .as_deref()
                            .map(|n| format!(" -- {n}"))
                            .unwrap_or_default();
                        format!("{checkbox} {}{}", s.title, notes)
                    })
                    .collect();

                let completed = plan
                    .iter()
                    .filter(|s| s.status == PlanStepStatus::Completed)
                    .count();
                let total = plan.len();

                Ok(ToolResult {
                    success: true,
                    output: format!(
                        "{}\n\n{}",
                        Self::render_plan_progress_header(total, completed),
                        lines.join("\n")
                    ),
                    error: None,
                })
            }
            "save" => {
                let plan_is_empty = { self.plan.read().is_empty() };
                if plan_is_empty {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some("No plan to save. Use action='set' first.".to_string()),
                    });
                }

                let plan_name = args
                    .get("plan_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("plan");
                let title = args
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or(plan_name);
                let description = args
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                self.ensure_plans_dir().await?;

                let filename = format!("{plan_name}.plan.md");
                let file_path = self.plans_dir_snapshot().join(&filename);
                let md = self.render_plan_md(title, description);
                crate::util::atomic_write_async(&file_path, md.clone().into_bytes()).await?;
                *self.active_plan_name.write() = Some(plan_name.to_string());

                let header = if Self::current_mode_is_plan() {
                    format!("Plan draft saved to `{}`", file_path.display())
                } else {
                    format!("Plan saved to `{}`", file_path.display())
                };
                Ok(ToolResult {
                    success: true,
                    output: format!("{header}\n\n{md}"),
                    error: None,
                })
            }

            "load" => {
                let plan_name = args
                    .get("plan_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("plan");

                let filename = format!("{plan_name}.plan.md");
                let file_path = self.plans_dir_snapshot().join(&filename);

                if !file_path.exists() {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Plan file not found: {}", file_path.display())),
                    });
                }

                let content = tokio::fs::read_to_string(&file_path).await?;
                let steps = Self::parse_plan_md(&content);
                let count = steps.len();
                let completed = steps
                    .iter()
                    .filter(|s| s.status == PlanStepStatus::Completed)
                    .count();
                *self.plan.write() = steps;
                *self.active_plan_name.write() = Some(plan_name.to_string());

                Ok(ToolResult {
                    success: true,
                    output: format!(
                        "{}\n\n{content}",
                        Self::render_plan_load_header(
                            file_path.display().to_string(),
                            count,
                            completed,
                        )
                    ),
                    error: None,
                })
            }

            "list" => {
                self.ensure_plans_dir().await?;
                let plans_dir = self.plans_dir_snapshot();
                let plans_dir_for_scan = plans_dir.clone();
                let plans: Vec<String> = tokio::task::spawn_blocking(move || {
                    let mut plans = Vec::new();
                    if plans_dir_for_scan.exists() {
                        if let Ok(entries) = std::fs::read_dir(&plans_dir_for_scan) {
                            for entry in entries.flatten() {
                                let path = entry.path();
                                if path.extension().and_then(|e| e.to_str()) == Some("md") {
                                    if let Some(name) = path.file_stem().and_then(|n| n.to_str()) {
                                        if name.ends_with(".plan")
                                            || path.to_string_lossy().contains(".plan.md")
                                        {
                                            plans.push(name.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                    plans
                })
                .await?;

                if plans.is_empty() {
                    Ok(ToolResult {
                        success: true,
                        output: format!("No saved plans in `{}`", plans_dir.display()),
                        error: None,
                    })
                } else {
                    let list = plans
                        .iter()
                        .map(|p| format!("- {p}"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    Ok(ToolResult {
                        success: true,
                        output: format!("Saved plans in `{}`:\n{list}", plans_dir.display()),
                        error: None,
                    })
                }
            }

            other => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Unknown action '{other}'. Use 'set', 'update', 'get', 'save', 'load', or 'list'."
                )),
            }),
        }
    }
}
