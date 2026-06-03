// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::agent::profile::personality;
use crate::config::IdentityConfig;
use crate::config::schema::GlobalDirective;
use crate::i18n::ToolDescriptions;
use crate::identity;
use crate::security::AutonomyLevel;
use crate::skills::Skill;
use crate::tools::Tool;
use anyhow::Result;
use chrono::{Datelike, Local, Timelike};
use std::fmt::Write;
use std::path::Path;

pub struct PromptContext<'a> {
    pub workspace_dir: &'a Path,
    pub model_name: &'a str,
    pub tools: &'a [Box<dyn Tool>],
    pub allowed_tool_names: Option<std::collections::HashSet<&'static str>>,
    pub skills: &'a [Skill],
    pub skills_prompt_mode: crate::config::SkillsPromptInjectionMode,
    pub identity_config: Option<&'a IdentityConfig>,
    pub dispatcher_instructions: &'a str,

    pub tool_descriptions: Option<&'a ToolDescriptions>,

    pub security_summary: Option<String>,

    pub autonomy_level: AutonomyLevel,

    pub global_directives: &'a [GlobalDirective],

    pub coding_mode_label: Option<&'a str>,
}

pub trait PromptSection: Send + Sync {
    fn name(&self) -> &str;
    fn build(&self, ctx: &PromptContext<'_>) -> Result<String>;
}

#[derive(Default)]
pub struct SystemPromptBuilder {
    sections: Vec<Box<dyn PromptSection>>,
}

impl SystemPromptBuilder {
    pub fn with_defaults() -> Self {
        Self {
            sections: vec![
                Box::new(DateTimeSection),
                Box::new(IdentitySection),
                Box::new(GlobalDirectivesSection),
                Box::new(EvolutionLessonsSection),
                Box::new(ExperienceRecyclingSection),
                Box::new(ToolHonestySection),
                Box::new(ToolsSection),
                Box::new(TaskPlanningSection),
                Box::new(SafetySection),
                Box::new(SkillsSection),
                Box::new(UserRulesSection),
                Box::new(WorkspaceSection),
                Box::new(RuntimeSection),
                Box::new(ChannelMediaSection),
            ],
        }
    }

    pub fn add_section(mut self, section: Box<dyn PromptSection>) -> Self {
        self.sections.push(section);
        self
    }

    pub fn build(&self, ctx: &PromptContext<'_>) -> Result<String> {
        let mut output = String::new();
        for section in &self.sections {
            let part = section.build(ctx)?;
            if part.trim().is_empty() {
                continue;
            }
            output.push_str(part.trim_end());
            output.push_str("\n\n");
        }
        Ok(output)
    }
}

pub struct IdentitySection;
pub struct ToolHonestySection;
pub struct ToolsSection;
pub struct TaskPlanningSection;
pub struct SafetySection;
pub struct SkillsSection;
pub struct UserRulesSection;
pub struct WorkspaceSection;
pub struct RuntimeSection;
pub struct DateTimeSection;
pub struct ChannelMediaSection;
pub struct GlobalDirectivesSection;
pub struct EvolutionLessonsSection;
pub struct ExperienceRecyclingSection;

impl PromptSection for GlobalDirectivesSection {
    fn name(&self) -> &str {
        "global_directives"
    }

    fn build(&self, ctx: &PromptContext<'_>) -> Result<String> {
        if ctx.global_directives.is_empty() {
            return Ok(String::new());
        }

        let active_mode = ctx.coding_mode_label.map(str::to_ascii_lowercase);
        let mut applicable: Vec<&str> = Vec::new();
        for d in ctx.global_directives {
            let content = d.content.trim();
            if content.is_empty() {
                continue;
            }
            if let Some(ref m) = d.mode {
                let m_norm = m.trim();
                if m_norm.is_empty() {
                    applicable.push(content);
                    continue;
                }
                match active_mode.as_deref() {
                    Some(active) if active == m_norm.to_ascii_lowercase() => {
                        applicable.push(content);
                    }
                    _ => {}
                }
            } else {
                applicable.push(content);
            }
        }

        if applicable.is_empty() {
            return Ok(String::new());
        }

        let mut out = String::from(
            "## Global Directives\n\n\
             The following user-configured directives apply to every \
             response in this session.  Treat them as binding constraints \
             on top of the rest of this system prompt:\n\n",
        );
        for d in applicable {
            for line in d.lines() {
                let trimmed = line.trim_end();
                if trimmed.is_empty() {
                    out.push('\n');
                } else {
                    out.push_str("- ");
                    out.push_str(trimmed);
                    out.push('\n');
                }
            }
            out.push('\n');
        }
        Ok(out)
    }
}

impl PromptSection for TaskPlanningSection {
    fn name(&self) -> &str {
        "task_planning"
    }

    fn build(&self, _ctx: &PromptContext<'_>) -> Result<String> {

        Ok(
            "## Task Planning Protocol  -  MANDATORY for multi-step work\n\n\
             For **any** user request whose completion requires 3+ tool calls, touches multiple \
             files, or has more than one distinct sub-goal, your **FIRST tool call MUST be \
             `todo_write`** to register the breakdown. This is not optional; the user's task UI \
             relies on this stream of calls to render a live progress bar  -  if you skip it, the \
             user sees nothing happening even while you work.\n\
             \n\
             ### When you MUST call `todo_write` first\n\
             - Any \"refactor X\", \"migrate Y\", \"add feature Z\" request.\n\
             - Investigations that span more than one file or component.\n\
             - Requests phrased as \"do A then B then C\" (numbered or comma-separated steps).\n\
             - Any task that will plausibly produce 3 or more `## Summary` bullets.\n\
             - Cross-cutting cleanups (lint sweeps, dead-code removal across modules).\n\
             \n\
             ### When you MUST NOT call `todo_write`\n\
             - **You are inside an active plan-execution turn** (i.e. the user clicked **Build** on \
             a `.plan.md` card, or you see a system message like `[Plan execution  - Agent mode]` \
             in this turn). In that case `update_plan` is the canonical tracker; calling \
             `todo_write` would create a duplicate, unsynchronised list and confuse the UI. Use \
             `update_plan(action=\"update\", ...)` for every step instead.\n\
             - Trivial single-shot replies (one comment, one quick lookup, a one-line answer).\n\
             - Pure conversational responses that do not invoke any tool.\n\
             \n\
             ### Lifecycle rules (when `todo_write` IS in play)\n\
             1. Register every planned step up front with the first call. Mark exactly ONE item as \
             `in_progress`; the rest are `pending`.\n\
             2. Do the work for that one step.\n\
             3. Call `todo_write` again (with `merge: true`) to flip it to `completed` AND flip the \
             next step to `in_progress`  -  in the **same** call, before you start the next step.\n\
             4. Repeat until all steps are terminal (`completed` or `cancelled`).\n\
             5. **Never** batch status flips at the very end of the turn  -  the progress bar will \
             appear stuck at 0/N and then jump to N/N, which is exactly the failure mode this \
             rule prevents.\n\
             6. Use `cancelled` (not silent drops) for items that turn out unnecessary, and \
             include a short reason in the `content` field if you can.\n\
             7. `merge: true` is the default for status updates; only use `merge: false` when the \
             overall plan structure changes fundamentally and the old list is no longer valid.\n\
             8. **Plan first, then execute.** Register the full breakdown before you start doing \
             the work, then walk the steps in order, flipping statuses as you go. Do not interleave \
             planning a brand-new list with execution of the current one.\n\
             9. **Never recreate a list that still has open items.** While ANY item in the current \
             list is `pending` or `in_progress`, you MUST NOT call `todo_write` with `merge: false` \
             to start a fresh list  -  that throws away unfinished work and confuses the UI \
             (the user sees a half-done list suddenly replaced by a new 0/N list). To add, remove, \
             or re-scope steps mid-flight, call `todo_write(merge: true, ...)` and update or append \
             items on the existing list; mark dropped steps `cancelled` rather than deleting them. \
             Only after every item is `completed` or `cancelled` may you start a brand-new list \
             with `merge: false`, and only if a genuinely new multi-step effort is needed. \
             (The runtime enforces this: a `merge: false` call while open items exist is \
             automatically merged into the existing list instead of replacing it.)\n\
             \n\
             ### Choosing the right tracker: task vs plan vs worker\n\
             - **Short task** (a handful of steps you can finish in this session): use `todo_write` \
             (the task list described here).\n\
             - **Medium task** (needs a written, reviewable plan before/while executing): use \
             `update_plan` with a `.plan.md` document  -  that is the canonical tracker for plan \
             work. When a plan list exists, do NOT also keep a `todo_write` task list; the plan \
             already IS the to-do list, and two parallel lists desync the UI.\n\
             - **Long task** (large or independent sub-jobs, especially parallelisable ones): \
             decompose with `spawn_workers` (parallel worker sub-agents) or `delegate_parallel`, \
             rather than cramming everything into one flat task list.\n\
             \n\
             ### Concrete example\n\
             User: \"Refactor user auth to use JWT.\"  -> first tool call MUST be:\n\
             `todo_write(todos=[\n\
                 {id:\"1\", content:\"Audit current auth flow\", status:\"in_progress\"},\n\
                 {id:\"2\", content:\"Implement JWT issue/verify\", status:\"pending\"},\n\
                 {id:\"3\", content:\"Update tests / docs\", status:\"pending\"}\n\
             ])`. Then do step 1, then call `todo_write(merge:true, todos=[{id:\"1\", \
             status:\"completed\"}, {id:\"2\", status:\"in_progress\"}])` before starting step 2."
                .into(),
        )
    }
}

impl PromptSection for IdentitySection {
    fn name(&self) -> &str {
        "identity"
    }

    fn build(&self, ctx: &PromptContext<'_>) -> Result<String> {
        let mut prompt = String::from("## Project Context\n\n");
        let mut has_aieos = false;
        if let Some(config) = ctx.identity_config {
            if identity::is_aieos_configured(config) {
                if let Ok(Some(aieos)) = identity::load_aieos_identity(config, ctx.workspace_dir) {
                    let rendered = identity::aieos_to_system_prompt(&aieos);
                    if !rendered.is_empty() {
                        prompt.push_str(&rendered);
                        prompt.push_str("\n\n");
                        has_aieos = true;
                    }
                }
            }
        }

        if !has_aieos {
            prompt.push_str(
                "The following workspace files define your identity, behavior, and context.\n\n",
            );
        }

        let profile = personality::load_personality(ctx.workspace_dir);
        prompt.push_str(&profile.render());

        Ok(prompt)
    }
}

impl PromptSection for ToolHonestySection {
    fn name(&self) -> &str {
        "tool_honesty"
    }

    fn build(&self, _ctx: &PromptContext<'_>) -> Result<String> {
        Ok(
            "## CRITICAL: Tool Honesty & Anti-Loop Policy\n\n\
             - NEVER fabricate, invent, or guess tool results. If a tool returns empty results, say \"No results found.\"\n\
             - If a tool call fails, the result is prefixed with \"Error: \"  - this is a failure signal, not content to repeat.\n\
             - If a shell command exits non-zero, inspect stdout/stderr and try a DIFFERENT approach. \
               Do NOT invoke the same shell command with identical arguments more than twice in a row \u{2014} \
               the loop guard will refuse the third identical retry and may abort the turn.\n\
             - If a shell command times out and is killed, NEVER repeat the same command verbatim. \
               Options: pass a larger `timeout_ms`; use `background: true` plus `background_status`/`background_logs`; \
               split the work into smaller steps; or use the ask tool to consult the user. \
               Long-running installs (pip, npm, cargo, apt) and full builds should always go via `background: true`.\n\
             - If two consecutive attempts fail in the same way (same error, same args), STOP retrying and \
               either (a) escalate to a different strategy or (b) ask the user for guidance via the ask tool.\n\
             - When unsure whether a tool call succeeded, ask the user rather than guessing."
                .into(),
        )
    }
}

impl PromptSection for ToolsSection {
    fn name(&self) -> &str {
        "tools"
    }

    fn build(&self, ctx: &PromptContext<'_>) -> Result<String> {
        let mut out = String::from("## Tools\n\n");
        for tool in ctx.tools {
            if let Some(ref allow) = ctx.allowed_tool_names
                && !allow.contains(tool.name())
            {
                continue;
            }
            let desc = ctx
                .tool_descriptions
                .and_then(|td: &ToolDescriptions| td.get(tool.name()))
                .unwrap_or_else(|| tool.description());
            let _ = writeln!(
                out,
                "- **{}**: {}\n  Parameters: `{}`",
                tool.name(),
                desc,
                tool.parameters_schema()
            );
        }
        if !ctx.dispatcher_instructions.is_empty() {
            out.push('\n');
            out.push_str(ctx.dispatcher_instructions);
        }
        Ok(out)
    }
}

impl PromptSection for SafetySection {
    fn name(&self) -> &str {
        "safety"
    }

    fn build(&self, ctx: &PromptContext<'_>) -> Result<String> {
        let mut out = String::from("## Safety\n\n- Do not exfiltrate private data.\n");

        if ctx.autonomy_level != AutonomyLevel::Full {
            out.push_str(
                "- Do not run destructive commands without asking.\n\
                 - Do not bypass oversight or approval mechanisms.\n",
            );
        }

        out.push_str("- Prefer `trash` over `rm`.\n");
        out.push_str(match ctx.autonomy_level {
            AutonomyLevel::Full => {
                "- Execute tools and actions directly  - no extra approval needed.\n\
                 - You have full access to all configured tools. Use them confidently to accomplish tasks.\n\
                 - Only refuse an action if the runtime explicitly rejects it  - do not preemptively decline."
            }
            AutonomyLevel::ReadOnly => {
                "- This runtime is read-only. Write operations will be rejected by the runtime if attempted.\n\
                 - Use read-only tools freely and confidently."
            }
            AutonomyLevel::Supervised => {
                "- Ask for approval when the runtime policy requires it for the specific action.\n\
                 - Do not preemptively refuse actions  - attempt them and let the runtime enforce restrictions.\n\
                 - Use available tools confidently; the security policy will enforce boundaries."
            }
        });

        if let Some(ref summary) = ctx.security_summary {
            out.push_str("\n\n### Active Security Policy\n\n");
            out.push_str(summary);
        }

        Ok(out)
    }
}

impl PromptSection for SkillsSection {
    fn name(&self) -> &str {
        "skills"
    }

    fn build(&self, ctx: &PromptContext<'_>) -> Result<String> {
        Ok(crate::skills::skills_to_prompt_with_mode(
            ctx.skills,
            ctx.workspace_dir,
            ctx.skills_prompt_mode,
        ))
    }
}

impl PromptSection for UserRulesSection {
    fn name(&self) -> &str {
        "user_rules"
    }

    fn build(&self, _ctx: &PromptContext<'_>) -> Result<String> {
        let rules = crate::user_rules::list_user_rules();
        Ok(crate::user_rules::user_rules_to_prompt(&rules))
    }
}

impl PromptSection for WorkspaceSection {
    fn name(&self) -> &str {
        "workspace"
    }

    fn build(&self, ctx: &PromptContext<'_>) -> Result<String> {
        Ok(format!(
            "## Workspace\n\nWorking directory: `{}`",
            ctx.workspace_dir.display()
        ))
    }
}

impl PromptSection for RuntimeSection {
    fn name(&self) -> &str {
        "runtime"
    }

    fn build(&self, ctx: &PromptContext<'_>) -> Result<String> {
        let host =
            hostname::get().map_or_else(|_| "unknown".into(), |h| h.to_string_lossy().to_string());
        Ok(format!(
            "## Runtime\n\nHost: {host} | OS: {} | Model: {}",
            std::env::consts::OS,
            ctx.model_name
        ))
    }
}

impl PromptSection for DateTimeSection {
    fn name(&self) -> &str {
        "datetime"
    }

    fn build(&self, _ctx: &PromptContext<'_>) -> Result<String> {
        let now = Local::now();

        let (year, month, day) = (now.year(), now.month(), now.day());
        let (hour, minute, second) = (now.hour(), now.minute(), now.second());
        let tz = now.format("%Z");

        Ok(format!(
            "## CRITICAL CONTEXT: CURRENT DATE & TIME\n\n\
             The following is the ABSOLUTE TRUTH regarding the current date and time. \
             Use this for all relative time calculations (e.g. \"last 7 days\").\n\n\
             Date: {year:04}-{month:02}-{day:02}\n\
             Time: {hour:02}:{minute:02}:{second:02} ({tz})\n\
             ISO 8601: {year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}{}",
            now.format("%:z")
        ))
    }
}

impl PromptSection for ChannelMediaSection {
    fn name(&self) -> &str {
        "channel_media"
    }

    fn build(&self, _ctx: &PromptContext<'_>) -> Result<String> {
        Ok("## Channel Media Markers\n\n\
            Messages from channels may contain media markers:\n\
            - `[Voice] <text>`  - The user sent a voice/audio message that has already been transcribed to text. Respond to the transcribed content directly.\n\
            - `[IMAGE:<path>]`  - An image attachment, processed by the vision pipeline.\n\
            - `[Document: <name>] <path>`  - A file attachment saved to the workspace."
            .into())
    }
}

impl PromptSection for EvolutionLessonsSection {
    fn name(&self) -> &str {
        "evolution_lessons"
    }

    fn build(&self, ctx: &PromptContext<'_>) -> Result<String> {
        let Some(engine) = crate::evolution::try_global() else {
            return Ok(String::new());
        };
        if !engine.enabled() {
            return Ok(String::new());
        }
        Ok(crate::evolution::build_lesson_block(&engine, ctx.coding_mode_label).unwrap_or_default())
    }
}

impl PromptSection for ExperienceRecyclingSection {
    fn name(&self) -> &str {
        "experience_recycling"
    }

    fn build(&self, ctx: &PromptContext<'_>) -> Result<String> {
        let Some(engine) = crate::evolution::try_global() else {
            return Ok(String::new());
        };
        if !engine.enabled() {
            return Ok(String::new());
        }
        Ok(
            crate::evolution::build_recycled_block(&engine, ctx.coding_mode_label)
                .unwrap_or_default(),
        )
    }
}
