// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::path::Path;

use super::values::{render_values, unresolved_tokens, FixedValue};

pub const SKILL_PLAN_FILE: &str = "skill-plan.json";
pub const BUILT_SKILL_FILE: &str = "built-skill.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub text: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub tool: String,
}

fn default_kind() -> String {
    "action".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPlan {
    pub architecture: String,
    pub name: String,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub generalization: String,
    #[serde(default)]
    pub values: Vec<FixedValue>,
    #[serde(default)]
    pub steps: Vec<PlanStep>,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltSkill {
    pub version: u32,
    pub session_id: String,
    pub architecture: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    pub body: String,
    #[serde(default)]
    pub values: Vec<FixedValue>,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exported_path: Option<String>,
}

pub fn slugify_skill_name(raw: &str) -> String {
    let slug: String = raw
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let collapsed = slug
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let trimmed: String = collapsed.chars().take(60).collect();
    let trimmed = trimmed.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "recorded-skill".to_string()
    } else {
        trimmed
    }
}

pub fn parse_plan(architecture: &str, args: &serde_json::Value) -> Result<SkillPlan> {
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(slugify_skill_name)
        .ok_or_else(|| anyhow!("propose_plan requires a 'name'"))?;
    let description = args
        .get("description")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| anyhow!("propose_plan requires a 'description'"))?
        .trim()
        .to_string();
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or(&name)
        .trim()
        .to_string();
    let steps = args
        .get("steps")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|raw| {
                    let text = raw
                        .get("text")
                        .and_then(|v| v.as_str())
                        .or_else(|| raw.as_str())?;
                    Some(PlanStep {
                        title: raw
                            .get("title")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        text: text.to_string(),
                        kind: raw
                            .get("kind")
                            .and_then(|v| v.as_str())
                            .filter(|k| *k == "calculation" || *k == "action")
                            .unwrap_or("action")
                            .to_string(),
                        tool: raw
                            .get("tool")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(SkillPlan {
        architecture: architecture.to_string(),
        name,
        title,
        description,
        summary: args
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        generalization: args
            .get("generalization")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        values: super::values::parse_values(args.get("values")),
        steps,
        allowed_tools: string_array(args.get("allowedTools")),
    })
}

pub fn string_array(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub fn render_skill_markdown(skill: &BuiltSkill) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    let _ = writeln!(out, "name: {}", slugify_skill_name(&skill.name));
    let description = skill.description.trim().replace('\\', "\\\\").replace('"', "\\\"");
    let _ = writeln!(out, "description: \"{description}\"");
    let tools: Vec<&String> = skill
        .allowed_tools
        .iter()
        .filter(|t| !t.trim().is_empty())
        .collect();
    if !tools.is_empty() {
        out.push_str("allowed-tools:\n");
        for tool in tools {
            let _ = writeln!(out, "  - {}", tool.trim());
        }
    }
    out.push_str("---\n\n");
    out.push_str(render_values(&skill.body, &skill.values).trim());
    out.push('\n');
    out
}

pub fn render_plan_for_prompt(plan: &SkillPlan) -> String {
    let mut lines = vec![
        format!("Title: {}", plan.title),
        format!("Name: {}", plan.name),
        format!("Description: {}", plan.description),
    ];
    if !plan.generalization.is_empty() {
        lines.push(format!("Generalization: {}", plan.generalization));
    }
    if !plan.values.is_empty() {
        lines.push(String::new());
        lines.push(
            "Values (reference each by its {{id}} token in the body — never write the literal):"
                .to_string(),
        );
        for v in &plan.values {
            lines.push(format!(
                "- {{{{{}}}}}{}",
                v.id,
                if v.name.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", v.name)
                }
            ));
        }
    }
    if !plan.steps.is_empty() {
        lines.push(String::new());
        lines.push("Steps (in order):".to_string());
        for (idx, step) in plan.steps.iter().enumerate() {
            let head = [step.title.as_str(), step.text.as_str()]
                .iter()
                .filter(|s| !s.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join(" — ");
            let mut line = format!("{}. ({}) {}", idx + 1, step.kind, head);
            if !step.tool.is_empty() {
                line.push_str(&format!(" [tool: {}]", step.tool));
            }
            lines.push(line);
        }
    }
    if !plan.allowed_tools.is_empty() {
        lines.push(String::new());
        lines.push(format!("allowed-tools: {}", plan.allowed_tools.join(", ")));
    }
    lines.join("\n")
}

pub fn built_skill_from_submission(
    session_id: &str,
    plan: &SkillPlan,
    args: &serde_json::Value,
) -> Result<BuiltSkill> {
    let body = args
        .get("body")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| anyhow!("submit_skill requires a non-empty 'body'"))?
        .to_string();
    let allowed_tools = {
        let submitted = string_array(args.get("allowedTools"));
        if submitted.is_empty() {
            plan.allowed_tools.clone()
        } else {
            submitted
        }
    };
    let unknown = unresolved_tokens(&body, &plan.values);
    if !unknown.is_empty() {
        tracing::warn!(
            "skill body references unknown value tokens: {}",
            unknown
                .iter()
                .map(|t| format!("{{{{{t}}}}}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(BuiltSkill {
        version: 1,
        session_id: session_id.to_string(),
        architecture: plan.architecture.clone(),
        name: slugify_skill_name(&plan.name),
        description: plan.description.clone(),
        allowed_tools,
        body,
        values: plan.values.clone(),
        created_at: chrono::Utc::now().timestamp_millis(),
        exported_path: None,
    })
}

pub fn save_plan(dir: &Path, plan: &SkillPlan) {
    if let Ok(bytes) = serde_json::to_vec_pretty(plan) {
        let _ = std::fs::write(dir.join(SKILL_PLAN_FILE), bytes);
    }
}

pub fn load_plan(dir: &Path) -> Option<SkillPlan> {
    let content = std::fs::read_to_string(dir.join(SKILL_PLAN_FILE)).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn persist_built(dir: &Path, skill: &BuiltSkill) {
    if let Ok(bytes) = serde_json::to_vec_pretty(skill) {
        let _ = std::fs::write(dir.join(BUILT_SKILL_FILE), bytes);
    }
}
