// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Result;
use chrono::Utc;
use std::sync::Arc;
use tokio::sync::mpsc;

use super::types::{AuditEvent, Lesson, TurnRecord};
use super::EvolutionEngine;

pub const DISTILL_QUEUE_CAPACITY: usize = 64;

const DISTILL_SYSTEM_PROMPT: &str = "You distil reusable coding lessons from successful AI assistant turns.\n\
Read the assistant's response and tool outcomes, then output 0–3 short, GENERAL lessons that future similar tasks should follow.\n\
\n\
STRICT format — output ONE JSON array on a single line, no surrounding text:\n\
[{\"title\":\"...\",\"body\":\"...\",\"tags\":[\"...\"]}]\n\
\n\
Rules:\n\
- title ≤ 10 words, action-oriented (\"Always run cargo check after editing X\").\n\
- body ≤ 240 characters, captures WHY and HOW.\n\
- tags 1–4 short keywords (lowercase) describing topic (e.g. rust, sqlite, async, refactor).\n\
- DROP anything that is project-specific path or one-off detail.\n\
- If nothing generalisable can be extracted, output exactly [].";

#[derive(Clone, Debug)]
pub struct DistillRequest {
    pub turn: TurnRecord,
}

pub async fn run_distill_worker(
    engine: Arc<EvolutionEngine>,
    mut rx: mpsc::Receiver<DistillRequest>,
) {
    while let Some(req) = rx.recv().await {
        if let Err(error) = process_request(Arc::clone(&engine), req).await {
            tracing::warn!(error = %error, "evolution distiller iteration failed");
        }
    }
}

async fn process_request(engine: Arc<EvolutionEngine>, req: DistillRequest) -> Result<()> {
    let turn = req.turn;
    let snapshot = engine.config_snapshot();
    let provider_ref = match engine.judge_provider() {
        Some(p) => p,
        None => return Ok(()),
    };
    let model = snapshot.judge_model.clone().unwrap_or(provider_ref.model.clone());
    if turn.response.content.as_deref().unwrap_or("").trim().is_empty() {
        return Ok(());
    }
    let user_prompt = build_distill_user_prompt(&turn);
    let answer = provider_ref
        .provider
        .chat_with_system(Some(DISTILL_SYSTEM_PROMPT), &user_prompt, &model, 0.1)
        .await?;
    let lessons = parse_lessons(&answer, turn.coding_mode.as_deref(), &turn.id);
    let store = engine.store();
    let mut produced = 0_u32;
    let mut skipped_dup = 0_u32;
    for mut lesson in lessons {
        match store.lesson_exists_by_title(lesson.coding_mode.as_deref(), &lesson.title) {
            Ok(true) => {
                skipped_dup += 1;
                continue;
            }
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(error = %error, "lesson dedup lookup failed");
            }
        }
        lesson.id = format!("lesson_{}", uuid::Uuid::new_v4().simple());
        lesson.created_at = Utc::now();
        lesson.updated_at = Utc::now();
        if let Err(error) = store.upsert_lesson(&lesson) {
            tracing::warn!(error = %error, "failed to upsert lesson");
        } else {
            produced += 1;
        }
    }
    let audit = AuditEvent {
        id: format!("ev_{}", uuid::Uuid::new_v4().simple()),
        kind: "distill".into(),
        turn_id: Some(turn.id.clone()),
        session_id: Some(turn.session_id.clone()),
        payload: serde_json::json!({
            "model": model,
            "produced": produced,
            "skippedDuplicates": skipped_dup,
            "raw_excerpt": answer.trim().chars().take(240).collect::<String>(),
        }),
        ts: Utc::now(),
    };
    let _ = store.append_audit(&audit);
    Ok(())
}

fn build_distill_user_prompt(turn: &TurnRecord) -> String {
    let mut buf = String::new();
    if let Some(ref mode) = turn.coding_mode {
        buf.push_str(&format!("[Coding mode] {}\n\n", mode));
    }
    if let Some(ref response) = turn.response.content {
        buf.push_str("[Assistant response]\n");
        buf.push_str(&truncate(response, 4_000));
        buf.push_str("\n\n");
    }
    if !turn.tool_outcomes.is_empty() {
        buf.push_str("[Tool outcomes]\n");
        for outcome in &turn.tool_outcomes {
            buf.push_str(&format!(
                "- {} → {}\n",
                outcome.name,
                if outcome.ok { "ok" } else { "fail" }
            ));
        }
        buf.push('\n');
    }
    buf.push_str(&format!(
        "[Reward] final={:.2} thumbs={:?} next_state={:?} tool={:?} verification={:?} cost={:?}\n",
        turn.reward.final_score,
        turn.reward.thumbs,
        turn.reward.next_state,
        turn.reward.tool,
        turn.reward.verification,
        turn.reward.cost,
    ));
    buf.push_str("\nProduce the JSON array of lessons (no extra text).");
    buf
}

fn parse_lessons(raw: &str, coding_mode: Option<&str>, source_turn_id: &str) -> Vec<Lesson> {
    let trimmed = raw.trim();
    let json_payload = match extract_json_array(trimmed) {
        Some(s) => s,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    let parsed: serde_json::Value = match serde_json::from_str(&json_payload) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let Some(arr) = parsed.as_array() else {
        return Vec::new();
    };
    for entry in arr {
        let title = entry.get("title").and_then(|v| v.as_str()).unwrap_or("").trim();
        let body = entry.get("body").and_then(|v| v.as_str()).unwrap_or("").trim();
        if title.is_empty() || body.is_empty() {
            continue;
        }
        let tags: Vec<String> = entry
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| t.as_str())
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty())
                    .take(8)
                    .collect()
            })
            .unwrap_or_default();
        let title_clipped: String = title.chars().take(120).collect();
        let body_clipped: String = body.chars().take(720).collect();
        out.push(Lesson {
            id: String::new(),
            title: title_clipped,
            body: body_clipped,
            tags,
            coding_mode: coding_mode.map(str::to_string),
            source_turn_ids: vec![source_turn_id.to_string()],
            hits: 0,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        });
        if out.len() >= 3 {
            break;
        }
    }
    out
}

fn extract_json_array(input: &str) -> Option<String> {
    let start = input.find('[')?;
    let mut depth = 0i32;
    for (idx, ch) in input[start..].char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(input[start..=(start + idx)].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars).collect();
    out.push_str("\n…[truncated]");
    out
}
