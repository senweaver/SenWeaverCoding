// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Result;
use chrono::Utc;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use super::store::RecyclingStore;
use super::types::{RecycledExperience, RecycledExperienceOutcome};
use crate::config::domain::evolution::ExperienceRecyclingConfig;
use crate::evolution::types::{ToolOutcome, TurnRecord};

const HEADLINE_CHARS: usize = 110;
const CONTEXT_CHARS: usize = 480;
const RESPONSE_CHARS: usize = 720;
const TOOL_SUMMARY_CHARS: usize = 240;
const SECRET_PATTERNS: &[&str] = &[
    "api_key",
    "api-key",
    "apikey",
    "secret",
    "password",
    "passwd",
    "token",
    "bearer ",
    "ssh-rsa",
    "begin private key",
];

#[derive(Debug, Default, Clone)]
pub struct RecyclingHarvestReport {
    pub considered: u32,
    pub stored: u32,
    pub skipped_low_reward: u32,
    pub skipped_dedup: u32,
    pub skipped_filter: u32,
}

pub fn harvest_turn(
    store: &RecyclingStore,
    turn: &TurnRecord,
    config: &ExperienceRecyclingConfig,
    workspace_dir: Option<&Path>,
) -> Result<RecyclingHarvestReport> {
    let mut report = RecyclingHarvestReport {
        considered: 1,
        ..RecyclingHarvestReport::default()
    };
    if !config.enabled {
        return Ok(report);
    }
    let sample_rate = config.sample_rate.clamp(0.0, 1.0);
    if sample_rate <= 0.0 {
        report.skipped_filter = 1;
        return Ok(report);
    }
    if sample_rate < 0.999 {
        let mut hasher = DefaultHasher::new();
        turn.id.hash(&mut hasher);
        let bucket = (hasher.finish() % 1000) as f32 / 1000.0;
        if bucket >= sample_rate {
            report.skipped_filter = 1;
            return Ok(report);
        }
    }
    let reward = turn.reward.final_score;
    if reward < config.min_reward {
        report.skipped_low_reward = 1;
        return Ok(report);
    }
    let outcome = classify_outcome(turn);
    let include_success = config.include_successes;
    let include_failure = config.include_failures;
    let pass = match outcome {
        RecycledExperienceOutcome::Success => include_success,
        RecycledExperienceOutcome::Failure => include_failure,
        RecycledExperienceOutcome::Neutral => include_success || include_failure,
    };
    if !pass {
        report.skipped_filter = 1;
        return Ok(report);
    }
    let workspace = workspace_dir
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let response_text = turn.response.content.as_deref().unwrap_or("").trim();
    let context_text = build_context_excerpt(turn);
    let tools_text = build_tools_summary(&turn.tool_outcomes);
    let headline = derive_headline(response_text, &context_text);
    let mut redacted_response = redact(
        response_text,
        config.redact_workspace_paths,
        config.redact_secrets,
        &workspace,
    );
    let mut redacted_context = redact(
        &context_text,
        config.redact_workspace_paths,
        config.redact_secrets,
        &workspace,
    );
    let mut redacted_tools = redact(
        &tools_text,
        config.redact_workspace_paths,
        config.redact_secrets,
        &workspace,
    );
    if config.redact_user_text {
        redacted_context = "[user content redacted]".to_string();
    }
    redacted_response = truncate(&redacted_response, RESPONSE_CHARS);
    redacted_context = truncate(&redacted_context, CONTEXT_CHARS);
    redacted_tools = truncate(&redacted_tools, TOOL_SUMMARY_CHARS);
    let signature = compute_signature(turn, &headline);
    if store.exists_for_signature(&signature)? {
        report.skipped_dedup = 1;
        return Ok(report);
    }
    let exp = RecycledExperience {
        id: format!("rec_{}", uuid::Uuid::new_v4().simple()),
        session_id: turn.session_id.clone(),
        turn_id: turn.id.clone(),
        coding_mode: turn.coding_mode.clone(),
        outcome,
        reward,
        headline: truncate(&headline, HEADLINE_CHARS),
        context_excerpt: redacted_context,
        response_excerpt: redacted_response,
        tools_summary: redacted_tools,
        tags: derive_tags(turn),
        shape_signature: signature,
        hits: 0,
        created_at: Utc::now(),
    };
    store.upsert(&exp)?;
    store.prune_to_capacity(config.max_retained)?;
    report.stored = 1;
    Ok(report)
}

fn classify_outcome(turn: &TurnRecord) -> RecycledExperienceOutcome {
    if turn.reward.final_score >= 0.5 {
        return RecycledExperienceOutcome::Success;
    }
    if turn.reward.final_score <= -0.5 {
        return RecycledExperienceOutcome::Failure;
    }
    let any_fail = turn.tool_outcomes.iter().any(|o| !o.ok);
    if any_fail && turn.reward.final_score < 0.0 {
        return RecycledExperienceOutcome::Failure;
    }
    RecycledExperienceOutcome::Neutral
}

fn build_context_excerpt(turn: &TurnRecord) -> String {
    let mut buf = String::new();
    let openai_user = turn
        .openai_messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .and_then(|m| m.content.clone());
    if let Some(text) = openai_user {
        buf.push_str(&text);
        return buf;
    }
    if let Some(message) = turn.anthropic_messages.iter().rev().find(|m| m.role == "user") {
        for block in &message.content {
            if let Some(text) = &block.text {
                if !text.is_empty() {
                    if !buf.is_empty() {
                        buf.push('\n');
                    }
                    buf.push_str(text);
                }
            }
        }
    }
    buf
}

fn build_tools_summary(outcomes: &[ToolOutcome]) -> String {
    if outcomes.is_empty() {
        return String::new();
    }
    let mut parts: Vec<String> = Vec::with_capacity(outcomes.len());
    for outcome in outcomes.iter().take(8) {
        let status = if outcome.ok { "ok" } else { "fail" };
        parts.push(format!("{}:{}", outcome.name, status));
    }
    if outcomes.len() > 8 {
        parts.push(format!("+{} more", outcomes.len() - 8));
    }
    parts.join(", ")
}

fn derive_headline(response: &str, context: &str) -> String {
    let pick = response.lines().find(|line| !line.trim().is_empty());
    if let Some(line) = pick {
        return line.trim().to_string();
    }
    let context_pick = context.lines().find(|line| !line.trim().is_empty());
    if let Some(line) = context_pick {
        return format!("re: {}", line.trim());
    }
    "(no headline available)".to_string()
}

fn derive_tags(turn: &TurnRecord) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    if let Some(ref provider) = turn.provider {
        tags.push(format!("provider:{}", provider.to_ascii_lowercase()));
    }
    if let Some(ref mode) = turn.coding_mode {
        tags.push(format!("mode:{}", mode.to_ascii_lowercase()));
    }
    for outcome in turn.tool_outcomes.iter().take(4) {
        tags.push(format!("tool:{}", outcome.name.to_ascii_lowercase()));
    }
    tags
}

fn compute_signature(turn: &TurnRecord, headline: &str) -> String {
    let mut hasher = DefaultHasher::new();
    if let Some(ref mode) = turn.coding_mode {
        mode.hash(&mut hasher);
    }
    for outcome in &turn.tool_outcomes {
        outcome.name.hash(&mut hasher);
        outcome.ok.hash(&mut hasher);
    }
    headline.trim().to_ascii_lowercase().hash(&mut hasher);
    format!("sig_{:016x}", hasher.finish())
}

fn redact(text: &str, paths: bool, secrets: bool, workspace: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut out = text.to_string();
    if paths && !workspace.is_empty() {
        out = out.replace(workspace, "<workspace>");
        let alt = workspace.replace('\\', "/");
        if alt != *workspace {
            out = out.replace(&alt, "<workspace>");
        }
    }
    if secrets {
        let lower = out.to_ascii_lowercase();
        for pattern in SECRET_PATTERNS {
            if lower.contains(pattern) {
                out = redact_lines_containing(&out, pattern);
            }
        }
    }
    out
}

fn redact_lines_containing(text: &str, pattern: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let lowered = pattern.to_ascii_lowercase();
    let mut first = true;
    for line in text.lines() {
        if !first {
            result.push('\n');
        }
        first = false;
        if line.to_ascii_lowercase().contains(&lowered) {
            result.push_str("[redacted: secret-like value]");
        } else {
            result.push_str(line);
        }
    }
    result
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars).collect();
    out.push('…');
    out
}
