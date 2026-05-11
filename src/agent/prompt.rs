// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::agent::personality;
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
            "## Task Planning Protocol\n\n\
             For any user request that needs more than one tool call or step, your FIRST action \
             should be to call the `todo_write` tool to register the steps. Mark exactly one todo \
             as `in_progress`, complete it, then update the list before moving on.\n\
             \n\
             - This applies in **every coding mode**, including Ask. In Ask mode the steps describe \
             the analysis you'll perform (read X, summarise Y, compare Z), not edits.\n\
             - Skip planning only for trivial single-step replies (one comment, one quick lookup, \
             a one-line answer).\n\
             - **Update incrementally — never batch silently.** Every time you finish a step, \
             call `todo_write` again to flip that step to `completed` BEFORE starting the next one. \
             The user's task bar reflects the latest call; if you do five steps and only update at \
             the end, the bar appears stuck at 0/5 the whole time.\n\
             - **If a single turn finishes multiple items**, update **all** of them in one \
             `todo_write` call (e.g. flip step 1 → `completed`, step 2 → `completed`, step 3 → \
             `in_progress` together). Don't skip the update because \"more than one moved\".\n\
             - **Pass `merge: true`** when you're only changing a few statuses so the rest of the \
             list isn't lost; pass `merge: false` only when the plan itself has fundamentally \
             changed shape.\n\
             - Mark steps `cancelled` when they turn out to be unnecessary, not silently dropped."
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
            "## CRITICAL: Tool Honesty\n\n\
             - NEVER fabricate, invent, or guess tool results. If a tool returns empty results, say \"No results found.\"\n\
             - If a tool call fails, the result is prefixed with \"Error: \" — this is a failure signal, not content to repeat.\n\
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
                "- Execute tools and actions directly — no extra approval needed.\n\
                 - You have full access to all configured tools. Use them confidently to accomplish tasks.\n\
                 - Only refuse an action if the runtime explicitly rejects it — do not preemptively decline."
            }
            AutonomyLevel::ReadOnly => {
                "- This runtime is read-only. Write operations will be rejected by the runtime if attempted.\n\
                 - Use read-only tools freely and confidently."
            }
            AutonomyLevel::Supervised => {
                "- Ask for approval when the runtime policy requires it for the specific action.\n\
                 - Do not preemptively refuse actions — attempt them and let the runtime enforce restrictions.\n\
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
            - `[Voice] <text>` — The user sent a voice/audio message that has already been transcribed to text. Respond to the transcribed content directly.\n\
            - `[IMAGE:<path>]` — An image attachment, processed by the vision pipeline.\n\
            - `[Document: <name>] <path>` — A file attachment saved to the workspace."
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
