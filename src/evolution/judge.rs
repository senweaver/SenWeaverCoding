// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Result;
use chrono::Utc;
use regex::Regex;
use std::sync::Arc;
use std::sync::OnceLock as StdOnceLock;
use tokio::sync::mpsc;

use super::types::{AuditEvent, NextStateView, SignalScore, SignalSource};
use super::EvolutionEngine;
use crate::providers::Provider;

pub const JUDGE_QUEUE_CAPACITY: usize = 256;

const JUDGE_SYSTEM_PROMPT: &str = "You are a Process Reward Judge for an AI coding assistant.\n\
You are given the assistant's previous response and the next message that came afterwards (from the user or a tool).\n\
Decide if the assistant's previous response was helpful, neutral, or wrong, based on what happened next.\n\
Reply with EXACTLY one of:\n\
\\boxed{1}   -  clearly helpful / made real progress / next state confirms success\n\
\\boxed{-1}  -  clearly wrong / regressed / user had to correct it / verification failed\n\
\\boxed{0}   -  unclear or only partially helpful / neutral\n\
Output only the boxed verdict, no other text.";

#[derive(Clone, Debug)]
pub struct JudgeRequest {
    pub turn_id: String,
    pub session_id: String,
    pub prev_response: String,
    pub next_state: NextStateView,
    pub coding_mode: Option<String>,
}

#[derive(Clone)]
pub struct JudgeProviderRef {
    pub provider: Arc<dyn Provider>,
    pub model: String,
}

fn boxed_pattern() -> &'static Regex {
    static RE: StdOnceLock<Regex> = StdOnceLock::new();
    RE.get_or_init(|| Regex::new(r"\\boxed\s*\{\s*(-?\s*[01])\s*\}").expect("valid boxed regex"))
}

pub fn parse_boxed_verdict(text: &str) -> Option<i8> {
    let captures = boxed_pattern().captures(text)?;
    let raw = captures.get(1)?.as_str().replace(' ', "");
    match raw.as_str() {
        "1" => Some(1),
        "-1" => Some(-1),
        "0" => Some(0),
        _ => None,
    }
}

pub async fn run_judge_worker(
    engine: Arc<EvolutionEngine>,
    mut rx: mpsc::Receiver<JudgeRequest>,
) {
    while let Some(req) = rx.recv().await {
        match process_request(Arc::clone(&engine), req).await {
            Ok(()) => {
                engine.note_judge_processed();
            }
            Err(error) => {
                let message = error.to_string();
                tracing::warn!(error = %message, "evolution judge worker iteration failed");
                engine.note_judge_error(&message);
            }
        }
    }
}

async fn process_request(engine: Arc<EvolutionEngine>, req: JudgeRequest) -> Result<()> {
    let snapshot = engine.config_snapshot();
    if !snapshot.next_state_judge_enabled {
        return Ok(());
    }
    let provider_ref = match engine.judge_provider() {
        Some(p) => p,
        None => return Ok(()),
    };
    let model = snapshot
        .judge_model
        .clone()
        .unwrap_or(provider_ref.model.clone());
    let prompt = format!(
        "[Previous assistant response]\n{}\n\n[Next state  -  {} message that came after]\n{}\n\n\
         Verdict (only \\boxed{{1}}, \\boxed{{-1}}, or \\boxed{{0}}):",
        truncate_for_prompt(&req.prev_response, 4_000),
        req.next_state.role,
        truncate_for_prompt(&req.next_state.content, 4_000),
    );
    let answer = provider_ref
        .provider
        .chat_with_system(Some(JUDGE_SYSTEM_PROMPT), &prompt, &model, 0.0)
        .await?;
    let verdict = parse_boxed_verdict(&answer);
    let store = engine.store();
    let mut merged_final: Option<f32> = None;
    if let Some(verdict) = verdict {
        let score = SignalScore {
            source: SignalSource::NextState,
            score: f32::from(verdict).clamp(-1.0, 1.0),
            confidence: 0.7,
            reason: Some(answer.trim().chars().take(120).collect()),
            ts: Utc::now(),
        };
        let merged = store.merge_turn_signal(&req.turn_id, &score, &snapshot.signal_weights)?;
        merged_final = Some(merged.final_score);
        engine.sync_recycled_reward(&req.turn_id, &merged);
        if verdict < 0 {
            engine.apply_lesson_feedback(&req.turn_id, merged.final_score);
        }
    }
    let audit = AuditEvent {
        id: format!("ev_{}", uuid::Uuid::new_v4().simple()),
        kind: "next_state_judge".into(),
        turn_id: Some(req.turn_id.clone()),
        session_id: Some(req.session_id.clone()),
        payload: serde_json::json!({
            "verdict": verdict,
            "raw": answer.trim().chars().take(240).collect::<String>(),
            "model": model,
            "finalReward": merged_final,
        }),
        ts: Utc::now(),
    };
    let _ = store.append_audit(&audit);
    Ok(())
}

fn truncate_for_prompt(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars).collect();
    out.push_str("\n…[truncated]");
    out
}
