// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{anyhow, bail, Result};
use base64::Engine as _;
use futures_util::stream::{self, StreamExt};
use std::fmt::Write as _;
use std::path::Path;
use tokio::sync::mpsc::UnboundedSender;

use super::types::{RecordedStep, RecorderEvent, RecorderStatus, RecordingManifest};
use crate::computer::vision::VisionClient;
use crate::config::Config;

const ANNOTATE_CONCURRENCY: usize = 3;

const ANNOTATE_SYSTEM_PROMPT: &str = "You are a precise UI analyst. You are shown a desktop \
screenshot taken immediately before a user action. Describe the specific UI element at the given \
location so another agent could find it later, in one short sentence (for example: 'the blue \
Submit button below the login form'). Respond with only the description, no quotes, no extra text.";

const DRAFT_SYSTEM_PROMPT: &str = "You write SKILL.md instruction files for a desktop automation \
agent that controls mouse and keyboard through screenshots. Given a task description and the \
recorded semantic steps of a human demonstration, write clear markdown instructions that let the \
agent repeat the workflow later, adapting to small UI changes. Structure the document with these \
sections: a one-paragraph summary, '## When to use', '## Inputs' (list values the user may want \
to change between runs, derived from typed text or the task), '## Steps' (numbered, referencing \
UI elements by description rather than coordinates), and '## Verification' (how to confirm \
success). Respond with only the markdown body, without YAML frontmatter and without code fences \
around the whole document.";

pub async fn generate(
    config: &Config,
    provider: &str,
    model: &str,
    workspace_dir: &Path,
    name: &str,
    event_tx: &UnboundedSender<RecorderEvent>,
) -> Result<String> {
    let skill_dir = workspace_dir.join("skills").join(name);
    let manifest_path = skill_dir.join("recording.json");
    let data = tokio::fs::read_to_string(&manifest_path)
        .await
        .map_err(|e| anyhow!("recording '{name}' not found: {e}"))?;
    let mut manifest: RecordingManifest =
        serde_json::from_str(&data).map_err(|e| anyhow!("invalid recording '{name}': {e}"))?;

    if manifest.steps.is_empty() {
        bail!("recording contains no steps to generate a skill from");
    }

    let client = VisionClient::from_config(config, provider, model)
        .map_err(|e| anyhow!("failed to initialize vision model '{model}': {e}"))?;

    let jobs: Vec<(usize, RecordedStep)> = manifest
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step_needs_annotation(step))
        .map(|(i, step)| (i, step.clone()))
        .collect();
    let annotate_total = jobs.len();
    if annotate_total > 0 {
        let _ = event_tx.send(RecorderEvent::status_code(
            RecorderStatus::Generating,
            "skill_annotating",
            format!("annotating 0/{annotate_total} steps"),
        ));
        let client_ref = &client;
        let dir_ref = &skill_dir;
        let mut results = stream::iter(jobs.into_iter().map(|(i, step)| async move {
            (i, annotate_step(client_ref, dir_ref, &step).await)
        }))
        .buffer_unordered(ANNOTATE_CONCURRENCY);
        let mut done = 0usize;
        while let Some((i, result)) = results.next().await {
            done += 1;
            let _ = event_tx.send(RecorderEvent::status_code(
                RecorderStatus::Generating,
                "skill_annotating",
                format!("annotating {done}/{annotate_total} steps"),
            ));
            match result {
                Ok(description) => {
                    manifest.steps[i].element_description = Some(description);
                }
                Err(e) => {
                    tracing::warn!("recorder skillgen annotation failed for step {i}: {e}");
                    manifest.steps[i].element_description =
                        Some(fallback_description(&manifest.steps[i]));
                }
            }
        }
    }

    let _ = event_tx.send(RecorderEvent::status_code(
        RecorderStatus::Generating,
        "skill_drafting",
        "drafting SKILL.md",
    ));

    let body = draft_skill_body(&client, &manifest).await?;
    let description = one_line(&manifest.task, 200);
    let skill_md = compose_skill_md(name, &description, &body);

    manifest.skill_name = Some(name.to_string());

    tokio::fs::write(skill_dir.join("SKILL.md"), skill_md.as_bytes()).await?;
    tokio::fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?).await?;

    Ok(name.to_string())
}

fn step_needs_annotation(step: &RecordedStep) -> bool {
    step.element_description.is_none()
        && step.x_norm.is_some()
        && step.screenshot_file.is_some()
        && matches!(
            step.action_type.as_str(),
            "click" | "double_click" | "right_click" | "drag"
        )
}

async fn annotate_step(
    client: &VisionClient,
    recording_dir: &Path,
    step: &RecordedStep,
) -> Result<String> {
    let file = step
        .screenshot_file
        .as_ref()
        .ok_or_else(|| anyhow!("step has no screenshot"))?;
    let bytes = tokio::fs::read(recording_dir.join(file)).await?;
    let mime = if file.to_ascii_lowercase().ends_with(".png") {
        "image/png"
    } else {
        "image/jpeg"
    };
    let data_uri = format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    );

    let x = step.x_norm.unwrap_or(0.0).round() as i64;
    let y = step.y_norm.unwrap_or(0.0).round() as i64;
    let mut prompt = format!(
        "The user performed a '{}' action at normalized coordinates ({x}, {y}) on a 0-1000 \
         scale (x from left, y from top) in this screenshot.",
        step.action_type
    );
    if let (Some(tx), Some(ty)) = (step.to_x_norm, step.to_y_norm) {
        let _ = write!(
            prompt,
            " The action ended at normalized coordinates ({}, {}).",
            tx.round() as i64,
            ty.round() as i64
        );
    }
    prompt.push_str(" Describe the UI element at the action location.");

    let response = client
        .complete_with_image(ANNOTATE_SYSTEM_PROMPT, &prompt, &data_uri)
        .await?;
    let cleaned = response.trim().trim_matches('"').trim().to_string();
    if cleaned.is_empty() {
        bail!("vision model returned an empty description");
    }
    Ok(one_line(&cleaned, 300))
}

fn fallback_description(step: &RecordedStep) -> String {
    format!(
        "the element at normalized position ({}, {})",
        step.x_norm.unwrap_or(0.0).round() as i64,
        step.y_norm.unwrap_or(0.0).round() as i64
    )
}

async fn draft_skill_body(client: &VisionClient, manifest: &RecordingManifest) -> Result<String> {
    let mut steps_text = String::new();
    for step in &manifest.steps {
        let _ = write!(steps_text, "{}. {}", step.index + 1, step.action_type);
        if let Some(desc) = &step.element_description {
            let _ = write!(steps_text, " on {desc}");
        }
        if let Some(value) = &step.value {
            let _ = write!(steps_text, " with value \"{value}\"");
        }
        if let Some(amount) = step.amount {
            let _ = write!(steps_text, " (amount {amount})");
        }
        steps_text.push('\n');
    }

    let user_prompt = format!(
        "Task description provided by the user before recording:\n{}\n\nRecorded semantic steps \
         of the demonstration:\n{}\nWrite the SKILL.md body now.",
        manifest.task, steps_text
    );

    let body = client
        .complete_text(DRAFT_SYSTEM_PROMPT, &user_prompt)
        .await?;
    let cleaned = strip_outer_fence(body.trim());
    if cleaned.is_empty() {
        bail!("model returned an empty skill draft");
    }
    Ok(cleaned)
}

fn compose_skill_md(name: &str, description: &str, body: &str) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    let _ = writeln!(out, "name: {name}");
    let _ = writeln!(out, "description: \"{}\"", escape_yaml(description));
    out.push_str("version: 0.1.0\n");
    out.push_str("author: sen-recorder\n");
    out.push_str("tags: [recorded, computer-use]\n");
    out.push_str("---\n\n");
    out.push_str(body);
    if !body.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn escape_yaml(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn strip_outer_fence(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("```") {
        let rest = rest
            .strip_prefix("markdown")
            .or_else(|| rest.strip_prefix("md"))
            .unwrap_or(rest);
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim().to_string();
        }
    }
    trimmed.to_string()
}

fn one_line(text: &str, max_chars: usize) -> String {
    let collapsed: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.chars().count() <= max_chars {
        return collapsed;
    }
    let truncated: String = collapsed.chars().take(max_chars).collect();
    format!("{truncated}…")
}
