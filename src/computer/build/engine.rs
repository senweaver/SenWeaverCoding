// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{anyhow, Result};

use super::catalogue::Architecture;
use super::instructions::{
    AUTOMATION_BUILDER_INSTRUCTIONS, AUTOMATION_KICKOFF_PROMPT, BUILD_NUDGE_PROMPT,
    SKILL_BUILDER_INSTRUCTIONS, SKILL_KICKOFF_PROMPT,
};
use super::{automation, skill};
use crate::computer::action::extract_json_object;
use crate::computer::describe::Analysis;
use crate::computer::vision::VisionClient;
use crate::providers::traits::ChatMessage;

const MAX_TURNS: usize = 12;

fn analysis_context(analysis: &Analysis) -> String {
    let mut lines = vec![
        "## Approved analysis".to_string(),
        format!("Title: {}", analysis.title),
        format!("Intent: {}", analysis.intent),
    ];
    if !analysis.intent_rationale.is_empty() {
        lines.push(format!("Rationale: {}", analysis.intent_rationale));
    }
    lines.push("Steps:".to_string());
    for (idx, step) in analysis.steps.iter().enumerate() {
        let apps = if step.apps.is_empty() {
            String::new()
        } else {
            format!(" [apps: {}]", step.apps.join(", "))
        };
        lines.push(format!("{}. {} — {}{apps}", idx + 1, step.title, step.detail));
    }
    lines.join("\n")
}

async fn run_propose_loop(
    client: &VisionClient,
    system: &str,
    kickoff: &str,
    analysis: &Analysis,
    tool_name: &str,
    feedback: Option<&str>,
) -> Result<serde_json::Value> {
    let mut prompt = format!("{}\n\n{}", kickoff, analysis_context(analysis));
    if let Some(feedback) = feedback.map(str::trim).filter(|f| !f.is_empty()) {
        prompt.push_str(&format!(
            "\n\nThe user reviewed your previous plan and asked for these changes; revise the plan accordingly and emit {tool_name} again:\n{feedback}"
        ));
    }
    let mut messages = vec![
        ChatMessage::system(system),
        ChatMessage::user(prompt),
    ];
    let mut nudged = false;
    let mut turns = 0usize;
    loop {
        turns += 1;
        if turns > MAX_TURNS {
            return Err(anyhow!("planning exceeded the maximum number of turns"));
        }
        let raw = client
            .complete_messages(&messages)
            .await
            .map_err(|e| anyhow!("model request failed: {e}"))?;
        messages.push(ChatMessage::assistant(raw.clone()));
        let Some(json) = extract_json_object(&raw) else {
            if !nudged {
                nudged = true;
                messages.push(ChatMessage::user(BUILD_NUDGE_PROMPT.to_string()));
                continue;
            }
            return Err(anyhow!("model did not return a tool call"));
        };
        let call: serde_json::Value = serde_json::from_str(&json)
            .map_err(|e| anyhow!("failed to parse the model's tool call: {e}"))?;
        let tool = call
            .get("tool")
            .and_then(|v| v.as_str())
            .or_else(|| call.get("name").and_then(|v| v.as_str()))
            .unwrap_or("");
        if tool == tool_name {
            return Ok(call
                .get("args")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({})));
        }
        if !nudged {
            nudged = true;
            messages.push(ChatMessage::user(format!(
                "Emit a single {tool_name} JSON tool call now."
            )));
            continue;
        }
        return Err(anyhow!("model did not emit {tool_name}"));
    }
}

pub async fn propose_skill_plan(
    client: &VisionClient,
    architecture: Architecture,
    analysis: &Analysis,
    feedback: Option<&str>,
) -> Result<skill::SkillPlan> {
    let system = format!(
        "{}\n\n{}",
        SKILL_BUILDER_INSTRUCTIONS,
        architecture.skill_catalogue()
    );
    let args = run_propose_loop(
        client,
        &system,
        SKILL_KICKOFF_PROMPT,
        analysis,
        "propose_plan",
        feedback,
    )
    .await?;
    skill::parse_plan(architecture.id(), &args)
}

pub async fn create_skill_body(
    client: &VisionClient,
    architecture: Architecture,
    plan: &skill::SkillPlan,
) -> Result<serde_json::Value> {
    let system = format!(
        "{}\n\n{}",
        SKILL_BUILDER_INSTRUCTIONS,
        architecture.skill_catalogue()
    );
    let mut messages = vec![
        ChatMessage::system(system),
        ChatMessage::user(format!(
            "{}\n\n{}",
            super::instructions::SKILL_CREATE_PROMPT,
            skill::render_plan_for_prompt(plan)
        )),
    ];
    let mut nudged = false;
    let mut turns = 0usize;
    loop {
        turns += 1;
        if turns > MAX_TURNS {
            return Err(anyhow!("skill authoring exceeded the maximum number of turns"));
        }
        let raw = client
            .complete_messages(&messages)
            .await
            .map_err(|e| anyhow!("model request failed: {e}"))?;
        messages.push(ChatMessage::assistant(raw.clone()));
        let Some(json) = extract_json_object(&raw) else {
            if !nudged {
                nudged = true;
                messages.push(ChatMessage::user(BUILD_NUDGE_PROMPT.to_string()));
                continue;
            }
            return Err(anyhow!("model did not return a tool call"));
        };
        let call: serde_json::Value = serde_json::from_str(&json)
            .map_err(|e| anyhow!("failed to parse the model's tool call: {e}"))?;
        let tool = call
            .get("tool")
            .and_then(|v| v.as_str())
            .or_else(|| call.get("name").and_then(|v| v.as_str()))
            .unwrap_or("");
        if tool == "submit_skill" {
            return Ok(call
                .get("args")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({})));
        }
        if !nudged {
            nudged = true;
            messages.push(ChatMessage::user(
                "Emit a single submit_skill JSON tool call now.".to_string(),
            ));
            continue;
        }
        return Err(anyhow!("model did not emit submit_skill"));
    }
}

pub async fn propose_automation_plan(
    client: &VisionClient,
    architecture: Architecture,
    analysis: &Analysis,
    feedback: Option<&str>,
) -> Result<automation::AutomationPlan> {
    let system = format!(
        "{}\n\n{}",
        AUTOMATION_BUILDER_INSTRUCTIONS,
        architecture.automation_catalogue()
    );
    let args = run_propose_loop(
        client,
        &system,
        AUTOMATION_KICKOFF_PROMPT,
        analysis,
        "propose_automation_plan",
        feedback,
    )
    .await?;
    automation::parse_plan(architecture.id(), &args)
}
