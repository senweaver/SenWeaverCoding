// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const ANALYSIS_FILE: &str = "analysis.json";
pub const ANALYSIS_MD_FILE: &str = "analysis.md";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

impl Default for Confidence {
    fn default() -> Self {
        Confidence::Medium
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisStep {
    pub id: String,
    pub title: String,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_ms: Option<i64>,
    #[serde(default)]
    pub apps: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub confidence: Confidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackStepNote {
    pub step_id: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackEntry {
    pub revision: u32,
    pub at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overall: Option<String>,
    #[serde(default)]
    pub steps: Vec<FeedbackStepNote>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Analysis {
    pub version: u32,
    pub session_id: String,
    pub revision: u32,
    pub created_at: i64,
    #[serde(default)]
    pub narration_source_updated_at: Option<i64>,
    #[serde(default)]
    pub title: String,
    pub intent: String,
    #[serde(default)]
    pub intent_confidence: Confidence,
    #[serde(default)]
    pub intent_rationale: String,
    #[serde(default)]
    pub steps: Vec<AnalysisStep>,
    #[serde(default)]
    pub feedback_log: Vec<FeedbackEntry>,
    #[serde(default)]
    pub approved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_at: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct AnalysisFeedback {
    pub overall: Option<String>,
    pub steps: Vec<FeedbackStepNote>,
}

pub fn parse_submission(args: &serde_json::Value) -> Result<(String, String, Confidence, String, Vec<AnalysisStep>)> {
    let intent = args
        .get("intent")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("submit_analysis requires a non-empty 'intent' string"))?
        .to_string();
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let confidence = parse_confidence(args.get("intentConfidence"));
    let rationale = args
        .get("intentRationale")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    let mut steps = Vec::new();
    if let Some(raw_steps) = args.get("steps").and_then(|v| v.as_array()) {
        for (idx, raw) in raw_steps.iter().enumerate() {
            let step_title = raw
                .get("title")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow!("steps[{idx}] is missing a non-empty 'title'"))?;
            let id = raw
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("s{}", idx + 1));
            steps.push(AnalysisStep {
                id,
                title: step_title.to_string(),
                detail: raw
                    .get("detail")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string(),
                start_ms: raw.get("startMs").and_then(|v| v.as_i64()),
                end_ms: raw.get("endMs").and_then(|v| v.as_i64()),
                apps: string_array(raw.get("apps")),
                evidence: string_array(raw.get("evidence")),
                confidence: parse_confidence(raw.get("confidence")),
            });
        }
    }
    Ok((title, intent, confidence, rationale, steps))
}

fn parse_confidence(value: Option<&serde_json::Value>) -> Confidence {
    match value.and_then(|v| v.as_str()) {
        Some("high") => Confidence::High,
        Some("low") => Confidence::Low,
        _ => Confidence::Medium,
    }
}

fn string_array(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub fn load_analysis(dir: &Path) -> Option<Analysis> {
    let content = std::fs::read_to_string(dir.join(ANALYSIS_FILE)).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn save_analysis(dir: &Path, analysis: &Analysis) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(analysis)?;
    std::fs::write(dir.join(ANALYSIS_FILE), bytes)?;
    let _ = std::fs::write(dir.join(ANALYSIS_MD_FILE), render_analysis_md(analysis));
    Ok(())
}

pub fn render_analysis_md(analysis: &Analysis) -> String {
    let mut lines = Vec::new();
    if !analysis.title.is_empty() {
        lines.push(format!("# {}", analysis.title));
    } else {
        lines.push("# Session analysis".to_string());
    }
    lines.push(String::new());
    lines.push(format!("**Intent:** {}", analysis.intent));
    if !analysis.intent_rationale.is_empty() {
        lines.push(String::new());
        lines.push(format!("_{}_", analysis.intent_rationale));
    }
    lines.push(String::new());
    lines.push("## Steps".to_string());
    for (idx, step) in analysis.steps.iter().enumerate() {
        lines.push(String::new());
        lines.push(format!("{}. **{}**", idx + 1, step.title));
        if !step.detail.is_empty() {
            lines.push(format!("   {}", step.detail));
        }
        if !step.apps.is_empty() {
            lines.push(format!("   Apps: {}", step.apps.join(", ")));
        }
    }
    lines.push(String::new());
    lines.join("\n")
}

pub struct AnalysisEdit {
    pub title: Option<String>,
    pub intent: Option<String>,
    pub steps: Option<Vec<AnalysisStep>>,
    pub approved: Option<bool>,
}

pub fn edit_analysis(dir: &Path, edit: AnalysisEdit) -> Result<Analysis> {
    let mut analysis =
        load_analysis(dir).ok_or_else(|| anyhow!("this recording has no analysis yet"))?;
    if let Some(title) = edit.title {
        analysis.title = title.trim().to_string();
    }
    if let Some(intent) = edit.intent {
        let trimmed = intent.trim();
        if !trimmed.is_empty() {
            analysis.intent = trimmed.to_string();
        }
    }
    if let Some(steps) = edit.steps {
        analysis.steps = steps;
    }
    if let Some(approved) = edit.approved {
        analysis.approved = approved;
        analysis.approved_at = approved.then(|| chrono::Utc::now().timestamp_millis());
    }
    save_analysis(dir, &analysis)?;
    Ok(analysis)
}
