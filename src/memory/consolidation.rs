// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! LLM-driven memory consolidation.
//!
//! After each conversation turn, extracts structured information:
//! - `history_entry`: A timestamped summary for the daily conversation log.
//! - `memory_update`: New facts, preferences, or decisions worth remembering
//!   long-term (or `null` if nothing new was learned).
//!
//! This two-phase approach replaces the naive raw-message auto-save with
//! semantic extraction, similar to Nanobot's `save_memory` tool call pattern.

use crate::memory::conflict;
use crate::memory::importance;
use crate::memory::traits::{Memory, MemoryCategory};
use crate::providers::traits::Provider;

#[derive(Debug, serde::Deserialize)]
pub struct ConsolidationResult {

    pub history_entry: String,

    pub memory_update: Option<String>,

    #[serde(default)]
    pub facts: Vec<String>,

    #[serde(default)]
    pub trend: Option<String>,
}

const CONSOLIDATION_SYSTEM_PROMPT: &str = r#"You are a memory consolidation engine. Given a conversation turn, extract:
1. "history_entry": A brief summary of what happened in this turn (1-2 sentences). Include the key topic or action.
2. "memory_update": Any NEW facts, preferences, decisions, or commitments worth remembering long-term. Return null if nothing new was learned.

Respond ONLY with valid JSON: {"history_entry": "...", "memory_update": "..." or null}
Do not include any text outside the JSON object."#;

fn strip_media_markers(text: &str) -> String {

    static RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\[(?:IMAGE|DOCUMENT|FILE|VIDEO|VOICE|AUDIO):[^\]]*\]").unwrap()
    });
    RE.replace_all(text, "[media attachment]").into_owned()
}

pub async fn consolidate_turn(
    provider: &dyn Provider,
    model: &str,
    memory: &dyn Memory,
    user_message: &str,
    assistant_response: &str,
) -> anyhow::Result<()> {
    let turn_text = format!(
        "User: {}\nAssistant: {}",
        strip_media_markers(user_message),
        strip_media_markers(assistant_response),
    );

    let truncated = if turn_text.len() > 4000 {
        let end = turn_text
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i <= 4000)
            .last()
            .unwrap_or(0);
        format!("{}…", &turn_text[..end])
    } else {
        turn_text.clone()
    };

    let raw = provider
        .chat_with_system(Some(CONSOLIDATION_SYSTEM_PROMPT), &truncated, model, 0.1)
        .await?;

    let result: ConsolidationResult = parse_consolidation_response(&raw, &turn_text);

    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let history_key = format!("daily_{date}_{}", uuid::Uuid::new_v4());
    memory
        .store(
            &history_key,
            &result.history_entry,
            MemoryCategory::Daily,
            None,
        )
        .await?;

    if let Some(ref update) = result.memory_update {
        if !update.trim().is_empty() {
            let mem_key = format!("core_{}", uuid::Uuid::new_v4());

            let imp = importance::compute_importance(update, &MemoryCategory::Core);

            match conflict::check_and_resolve_conflicts(
                memory,
                &mem_key,
                update,
                &MemoryCategory::Core,
                0.85,
            )
            .await
            {
                Ok(superseded_ids) if !superseded_ids.is_empty() => {
                    tracing::debug!(
                        "conflict resolution superseded {} existing entries",
                        superseded_ids.len()
                    );
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::debug!("conflict check skipped: {e}");
                }
            }

            memory
                .store_with_metadata(
                    &mem_key,
                    update,
                    MemoryCategory::Core,
                    None,
                    None,
                    Some(imp),
                )
                .await?;
        }
    }

    Ok(())
}

fn parse_consolidation_response(raw: &str, fallback_text: &str) -> ConsolidationResult {

    let cleaned = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    serde_json::from_str(cleaned).unwrap_or_else(|_| {

        let summary = if fallback_text.len() > 200 {
            let end = fallback_text
                .char_indices()
                .map(|(i, _)| i)
                .take_while(|&i| i <= 200)
                .last()
                .unwrap_or(0);
            format!("{}…", &fallback_text[..end])
        } else {
            fallback_text.to_string()
        };
        ConsolidationResult {
            history_entry: summary,
            memory_update: None,
            facts: Vec::new(),
            trend: None,
        }
    })
}
