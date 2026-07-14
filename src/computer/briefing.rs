// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Result;

use super::vision::VisionClient;
use crate::config::Config;

const MAX_DOCUMENT_CHARS: usize = 8_000;
const MAX_DOCUMENTS: usize = 8;
const SUPPORTED_IMAGE_MIMES: [&str; 4] = ["image/png", "image/jpeg", "image/webp", "image/gif"];

const BRIEFING_SYSTEM: &str = "You prepare execution plans for an autonomous desktop \
automation agent that controls a real computer by looking at screenshots and performing \
click / double_click / right_click / type / key_press / scroll / drag / wait actions.\n\n\
Given the user's goal plus any attached reference images and documents, write a concrete, \
numbered step-by-step plan the agent can follow on screen. Each step must be a single \
observable UI interaction with a concrete target (button label, menu name, field name) and \
exact values to type where applicable. Include verification hints where a step's success is \
visible on screen. Do not invent data that is not present in the goal or the attachments.\n\n\
Respond in the same language as the user's goal. Reply with the plan text only - no preamble, \
no closing remarks, no code fences.";

#[derive(Debug, Clone, Default)]
pub struct NormalizedAttachments {
    pub image_data_uris: Vec<String>,
    pub document_block: String,
}

impl NormalizedAttachments {
    pub fn is_empty(&self) -> bool {
        self.image_data_uris.is_empty() && self.document_block.is_empty()
    }
}

pub fn parse_attachments(
    value: Option<&serde_json::Value>,
    config: &Config,
) -> Result<NormalizedAttachments, (&'static str, String)> {
    let mut out = NormalizedAttachments::default();
    let Some(items) = value.and_then(|v| v.as_array()) else {
        return Ok(out);
    };
    let (max_images, max_image_size_mb) = config.multimodal.effective_limits();
    let max_image_bytes = max_image_size_mb.saturating_mul(1024 * 1024);
    let mut documents = 0usize;

    for item in items {
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("attachment")
            .to_string();
        let mime = item
            .get("mime")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();

        if SUPPORTED_IMAGE_MIMES.contains(&mime.as_str()) {
            let Some(data) = item
                .get("data_base64")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            else {
                return Err((
                    "attachment_unsupported",
                    format!("image attachment '{name}' is missing data"),
                ));
            };
            let estimated_bytes = data.len().saturating_mul(3) / 4;
            if estimated_bytes > max_image_bytes {
                return Err((
                    "attachment_too_large",
                    format!(
                        "image attachment '{name}' exceeds the {max_image_size_mb}MB limit"
                    ),
                ));
            }
            if out.image_data_uris.len() >= max_images {
                return Err((
                    "attachment_too_large",
                    format!("too many image attachments (limit {max_images})"),
                ));
            }
            out.image_data_uris.push(format!("data:{mime};base64,{data}"));
            continue;
        }

        let text = item
            .get("text")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if let Some(text) = text {
            if documents >= MAX_DOCUMENTS {
                return Err((
                    "attachment_too_large",
                    format!("too many document attachments (limit {MAX_DOCUMENTS})"),
                ));
            }
            documents += 1;
            let truncated: String = text.chars().take(MAX_DOCUMENT_CHARS).collect();
            out.document_block
                .push_str(&format!("\n\n--- Attached document: {name} ---\n{truncated}"));
            if text.chars().count() > MAX_DOCUMENT_CHARS {
                out.document_block.push_str("\n[document truncated]");
            }
            continue;
        }

        return Err((
            "attachment_unsupported",
            format!("attachment '{name}' has unsupported type '{mime}'"),
        ));
    }

    Ok(out)
}

pub async fn draft_execution_steps(
    client: &VisionClient,
    task: &str,
    attachments: &NormalizedAttachments,
) -> Result<String> {
    let mut user = String::new();
    if task.trim().is_empty() {
        user.push_str("User goal: derive the goal from the attached materials.\n");
    } else {
        user.push_str(&format!("User goal:\n{}\n", task.trim()));
    }
    if !attachments.document_block.is_empty() {
        user.push_str(&attachments.document_block);
        user.push('\n');
    }
    if !attachments.image_data_uris.is_empty() {
        user.push_str(&format!(
            "\n{} reference image(s) are attached.\n",
            attachments.image_data_uris.len()
        ));
    }
    user.push_str("\nWrite the execution plan now.");

    let raw = if attachments.image_data_uris.is_empty() {
        client.complete_text(BRIEFING_SYSTEM, &user).await?
    } else {
        let uris: Vec<&str> = attachments
            .image_data_uris
            .iter()
            .map(String::as_str)
            .collect();
        client
            .complete_with_images(BRIEFING_SYSTEM, &user, &uris)
            .await?
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("model returned an empty plan");
    }
    Ok(trimmed.to_string())
}
