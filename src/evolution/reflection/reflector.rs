// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;

use super::lesson::{ReflectionLesson, ReflectionLessonKind};
use super::trigger::ReflectionTriggerCause;
use super::types::{ReflectionRunStatus, ReflectionWritebackReport};
use super::writeback::apply_writeback;
use crate::config::domain::evolution::{ReflectionDepth, SelfReflectionConfig};
use crate::evolution::types::{AuditEvent, TurnRecord};
use crate::evolution::EvolutionEngine;

pub const REFLECTION_QUEUE_CAPACITY: usize = 16;

const SYSTEM_PROMPT_QUICK: &str = "You reflect on an AI coding assistant's recent turns and distil 0–3 short, GENERAL lessons.\n\
Read the turn summaries and rewards, then output strictly a JSON object on one line:\n\
{\"summary\":\"...\",\"lessons\":[{\"kind\":\"insight|avoid|followup\",\"title\":\"...\",\"body\":\"...\",\"tags\":[\"...\"]}]}\n\
\n\
Rules:\n\
- title ≤ 10 words, action-oriented.\n\
- body ≤ 240 characters, capturing WHY and HOW.\n\
- kind=insight for positive patterns; kind=avoid for anti-patterns; kind=followup for explicit next-action recommendations.\n\
- DROP project-specific paths or one-off details. If nothing generalisable can be extracted, output {\"summary\":\"no actionable signal\",\"lessons\":[]}.";

const SYSTEM_PROMPT_DEEP: &str = "You are a senior software engineering reflector. Read the AI coding assistant's recent turns and produce a deep self-review.\n\
Reply STRICTLY with a single JSON object on one line:\n\
{\"summary\":\"...\",\"lessons\":[{\"kind\":\"insight|avoid|followup\",\"title\":\"...\",\"body\":\"...\",\"tags\":[\"...\"]}]}\n\
\n\
Rules:\n\
- summary ≤ 320 characters: identify the root pattern across turns (success or failure mode).\n\
- lessons up to 3 entries, ranked by leverage.\n\
- title ≤ 10 words, action-oriented (\"Always run cargo check before claiming a fix\").\n\
- body ≤ 320 characters, capturing WHY/HOW/WHEN; reference reward/tool signal when relevant.\n\
- kind=avoid for anti-patterns observed in failures; kind=insight for transferable wins; kind=followup for explicit next-actions to retain context across sessions.\n\
- tags 1–4 short lowercase keywords (e.g. rust, sqlite, async, refactor).";

#[derive(Debug, Clone)]
pub struct ReflectionRequest {
    pub run_id: String,
    pub trigger: ReflectionTriggerCause,
    pub session_id: Option<String>,
    pub turns: Vec<TurnRecord>,
    pub coding_mode: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct ReflectionPayload {
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    lessons: Vec<RawLesson>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct RawLesson {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

pub async fn run_reflection_worker(
    engine: Arc<EvolutionEngine>,
    mut rx: mpsc::Receiver<ReflectionRequest>,
) {
    while let Some(request) = rx.recv().await {
        if let Err(error) = process_request(Arc::clone(&engine), request).await {
            tracing::warn!(error = %error, "evolution reflection iteration failed");
        }
    }
}

async fn process_request(engine: Arc<EvolutionEngine>, req: ReflectionRequest) -> Result<()> {
    let snapshot = engine.config_snapshot();
    let reflection_cfg = snapshot.reflection.clone();
    let resolved = match engine.resolve_reflection_provider() {
        Some(r) => r,
        None => {
            let reason = if engine.has_registered_models() {
                "no_provider_for_registered_model"
            } else {
                "no_provider"
            };
            tracing::warn!(
                run_id = req.run_id.as_str(),
                reason = reason,
                "evolution reflection skipped: no resolvable provider"
            );
            if let Some(store) = engine.reflection_store() {
                let _ = store.record_skipped(&req.run_id, reason);
            }
            record_skip_audit(&engine, &req, reason, None);
            return Ok(());
        }
    };
    let model = resolved.model.clone();
    if !engine.is_model_registered(&model) {
        tracing::warn!(
            run_id = req.run_id.as_str(),
            model = model.as_str(),
            "evolution reflection skipped: model_not_registered"
        );
        if let Some(store) = engine.reflection_store() {
            let _ = store.record_skipped(&req.run_id, "model_not_registered");
        }
        record_skip_audit(&engine, &req, "model_not_registered", resolved.provider_id.clone());
        return Ok(());
    }
    if let Some(store) = engine.reflection_store() {
        let _ = store.record_start(
            &req.run_id,
            req.session_id.as_deref(),
            req.trigger.as_str(),
            reflection_cfg.depth.as_str(),
            Some(&model),
        );
    }
    let user_prompt = build_user_prompt(&req, &reflection_cfg);
    let system_prompt = match reflection_cfg.depth {
        ReflectionDepth::Deep => SYSTEM_PROMPT_DEEP,
        ReflectionDepth::Quick => SYSTEM_PROMPT_QUICK,
    };
    let response = match resolved
        .provider
        .chat_with_system(Some(system_prompt), &user_prompt, &model, 0.2)
        .await
    {
        Ok(text) => text,
        Err(error) => {
            if let Some(store) = engine.reflection_store() {
                let _ = store.record_completion(
                    &req.run_id,
                    ReflectionRunStatus::Failed,
                    0,
                    u32::try_from(req.turns.len()).unwrap_or(0),
                    None,
                    Some(&error.to_string()),
                );
            }
            return Err(error);
        }
    };
    let payload = parse_payload(&response);
    let lessons = lessons_from_payload(&payload, reflection_cfg.max_lessons_per_run);
    let writeback_report = if lessons.is_empty() {
        ReflectionWritebackReport::default()
    } else {
        apply_writeback(&engine, &lessons, &reflection_cfg, &req).await
    };
    if !writeback_report.errors.is_empty() {
        for err in &writeback_report.errors {
            let audit = AuditEvent {
                id: format!("ev_{}", uuid::Uuid::new_v4().simple()),
                kind: "reflection.writeback_failed".into(),
                turn_id: None,
                session_id: req.session_id.clone(),
                payload: serde_json::json!({
                    "runId": req.run_id,
                    "error": err,
                }),
                ts: Utc::now(),
            };
            let _ = engine.store().append_audit(&audit);
        }
    }
    let lessons_count = u32::try_from(lessons.len()).unwrap_or(0);
    let summary_clean = payload
        .summary
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let aggregated_error: Option<String> = if writeback_report.errors.is_empty() {
        None
    } else {
        Some(writeback_report.errors.join("; "))
    };
    if let Some(store) = engine.reflection_store() {
        let _ = store.record_completion(
            &req.run_id,
            if lessons.is_empty() {
                ReflectionRunStatus::Skipped
            } else {
                ReflectionRunStatus::Completed
            },
            lessons_count,
            u32::try_from(req.turns.len()).unwrap_or(0),
            summary_clean.as_deref(),
            aggregated_error.as_deref(),
        );
    }
    let audit = AuditEvent {
        id: format!("ev_{}", uuid::Uuid::new_v4().simple()),
        kind: "reflection".into(),
        turn_id: None,
        session_id: req.session_id.clone(),
        payload: serde_json::json!({
            "runId": req.run_id,
            "trigger": req.trigger.as_str(),
            "depth": reflection_cfg.depth.as_str(),
            "lessonsProduced": lessons_count,
            "writeback": writeback_report,
            "model": model,
            "providerId": resolved.provider_id,
            "summary": summary_clean,
        }),
        ts: Utc::now(),
    };
    let _ = engine.store().append_audit(&audit);
    Ok(())
}

fn record_skip_audit(
    engine: &Arc<EvolutionEngine>,
    req: &ReflectionRequest,
    reason: &str,
    provider_id: Option<String>,
) {
    let audit = AuditEvent {
        id: format!("ev_{}", uuid::Uuid::new_v4().simple()),
        kind: "reflection.skipped".into(),
        turn_id: None,
        session_id: req.session_id.clone(),
        payload: serde_json::json!({
            "runId": req.run_id,
            "reason": reason,
            "providerId": provider_id,
        }),
        ts: Utc::now(),
    };
    let _ = engine.store().append_audit(&audit);
}

fn build_user_prompt(req: &ReflectionRequest, cfg: &SelfReflectionConfig) -> String {
    let mut buf = String::new();
    buf.push_str(&format!(
        "[Reflection trigger] {} (depth {})\n\n",
        req.trigger.as_str(),
        cfg.depth.as_str()
    ));
    if let Some(ref mode) = req.coding_mode {
        buf.push_str(&format!("[Coding mode] {}\n\n", mode));
    }
    if req.turns.is_empty() {
        buf.push_str(
            "[Notice] No recent turn records were available; produce 'no actionable signal' if nothing else applies.\n\n",
        );
    } else {
        buf.push_str(&format!(
            "[Recent turns] {} entries (most recent first)\n",
            req.turns.len()
        ));
        for (idx, turn) in req.turns.iter().take(cfg.lookback_turns.max(1)).enumerate() {
            buf.push_str(&format!(
                "\n--- turn #{} id={} reward={:.2} aborted={}\n",
                idx + 1,
                turn.id,
                turn.reward.final_score,
                turn.aborted.as_deref().unwrap_or("no")
            ));
            if let Some(ref mode) = turn.coding_mode {
                buf.push_str(&format!("mode: {}\n", mode));
            }
            if let Some(ref response) = turn.response.content {
                buf.push_str("response: ");
                buf.push_str(&truncate(response, 720));
                buf.push('\n');
            }
            if !turn.tool_outcomes.is_empty() {
                buf.push_str("tools:");
                for outcome in turn.tool_outcomes.iter().take(8) {
                    buf.push_str(&format!(
                        " {}:{}",
                        outcome.name,
                        if outcome.ok { "ok" } else { "fail" }
                    ));
                }
                buf.push('\n');
            }
        }
    }
    buf.push_str(
        "\nReturn only the JSON object specified by the system prompt (no extra text or code fences).\n",
    );
    buf
}

fn parse_payload(raw: &str) -> ReflectionPayload {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return ReflectionPayload::default();
    }
    let extracted = extract_json_object(trimmed).unwrap_or_else(|| trimmed.to_string());
    serde_json::from_str::<ReflectionPayload>(&extracted).unwrap_or_default()
}

fn lessons_from_payload(payload: &ReflectionPayload, max_lessons: usize) -> Vec<ReflectionLesson> {
    let cap = max_lessons.max(1);
    let mut out: Vec<ReflectionLesson> = Vec::new();
    for raw in &payload.lessons {
        let title = raw
            .title
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.chars().take(120).collect::<String>());
        let body = raw
            .body
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.chars().take(720).collect::<String>());
        let (title, body) = match (title, body) {
            (Some(t), Some(b)) => (t, b),
            _ => continue,
        };
        let kind = raw
            .kind
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .map(|s| ReflectionLessonKind::parse(&s))
            .unwrap_or(ReflectionLessonKind::Insight);
        let tags: Vec<String> = raw
            .tags
            .iter()
            .map(|t| t.trim().to_ascii_lowercase())
            .filter(|t| !t.is_empty())
            .take(8)
            .collect();
        out.push(ReflectionLesson {
            kind,
            title,
            body,
            tags,
        });
        if out.len() >= cap {
            break;
        }
    }
    out
}

fn extract_json_object(input: &str) -> Option<String> {
    let start = input.find('{')?;
    let mut depth = 0_i32;
    for (idx, ch) in input[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
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
