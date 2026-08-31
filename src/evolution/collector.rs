// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use chrono::Utc;
use parking_lot::Mutex;
use std::sync::Arc;
use tokio::task_local;

use super::types::{
    AnthropicMessageView, ChatMessageView, CostView, ResponseView, ToolOutcome, TurnClass,
    TurnRecord,
};
use super::{EvolutionEngine, store::Store};

#[derive(Default, Debug)]
pub struct TurnAccumulator {
    pub openai_messages: Vec<ChatMessageView>,
    pub anthropic_messages: Vec<AnthropicMessageView>,
    pub anthropic_system: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub response: ResponseView,
    pub tool_outcomes: Vec<ToolOutcome>,
    pub cost: CostView,
    pub injected_lesson_ids: Vec<String>,
    pub finalized: bool,
}

#[derive(Clone)]
pub struct EvolutionCtx {
    engine: Arc<EvolutionEngine>,
    session_id: String,
    coding_mode: Option<String>,
    user_message: Option<String>,
    turn_class: TurnClass,
    turn_idx: u64,
    accumulator: Arc<Mutex<TurnAccumulator>>,
}

impl EvolutionCtx {
    pub fn new(engine: Arc<EvolutionEngine>, session_id: impl Into<String>) -> Self {
        Self {
            engine,
            session_id: session_id.into(),
            coding_mode: None,
            user_message: None,
            turn_class: TurnClass::Main,
            turn_idx: 0,
            accumulator: Arc::new(Mutex::new(TurnAccumulator::default())),
        }
    }

    pub fn with_coding_mode(mut self, coding_mode: impl Into<String>) -> Self {
        self.coding_mode = Some(coding_mode.into());
        self
    }

    pub fn with_user_message(mut self, message: impl Into<String>) -> Self {
        let value: String = message.into();
        if !value.trim().is_empty() {
            self.user_message = Some(value);
        }
        self
    }

    pub fn user_message(&self) -> Option<&str> {
        self.user_message.as_deref()
    }

    pub fn with_turn_class(mut self, class: TurnClass) -> Self {
        self.turn_class = class;
        self
    }

    pub fn with_turn_idx(mut self, idx: u64) -> Self {
        self.turn_idx = idx;
        self
    }

    pub fn engine(&self) -> &Arc<EvolutionEngine> {
        &self.engine
    }

    pub fn store(&self) -> Arc<Store> {
        Arc::clone(self.engine.store())
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn coding_mode(&self) -> Option<&str> {
        self.coding_mode.as_deref()
    }

    pub fn turn_class(&self) -> TurnClass {
        self.turn_class
    }

    pub fn turn_idx(&self) -> u64 {
        self.turn_idx
    }

    pub fn record_provider_model(&self, provider: Option<String>, model: Option<String>) {
        let mut acc = self.accumulator.lock();
        if provider.is_some() {
            acc.provider = provider;
        }
        if model.is_some() {
            acc.model = model;
        }
    }

    pub fn set_prompt_messages(
        &self,
        openai_messages: Vec<ChatMessageView>,
        anthropic_messages: Vec<AnthropicMessageView>,
        anthropic_system: Option<String>,
    ) {
        let mut acc = self.accumulator.lock();
        acc.openai_messages = openai_messages;
        acc.anthropic_messages = anthropic_messages;
        acc.anthropic_system = anthropic_system;
    }

    pub fn observe_tool_outcome(&self, outcome: ToolOutcome) {
        let mut acc = self.accumulator.lock();
        acc.tool_outcomes.push(outcome);
    }

    pub fn add_cost(&self, input_tokens: u64, output_tokens: u64, total_tokens: u64, usd: f64) {
        let mut acc = self.accumulator.lock();
        acc.cost.input_tokens = acc.cost.input_tokens.saturating_add(input_tokens);
        acc.cost.output_tokens = acc.cost.output_tokens.saturating_add(output_tokens);
        acc.cost.total_tokens = acc.cost.total_tokens.saturating_add(total_tokens);
        acc.cost.usd += usd;
    }

    pub fn set_response_text(&self, text: impl Into<String>) {
        let mut acc = self.accumulator.lock();
        acc.response.content = Some(text.into());
    }

    pub fn set_thinking_text(&self, text: impl Into<String>) {
        const MAX_THINKING_CHARS: usize = 64 * 1024;
        let mut acc = self.accumulator.lock();
        let value = text.into();
        if value.is_empty() {
            return;
        }
        match acc.response.thinking {
            Some(ref mut existing) => {
                if existing.chars().count() >= MAX_THINKING_CHARS {
                    return;
                }
                existing.push_str("\n\n");
                existing.push_str(&value);
            }
            None => acc.response.thinking = Some(value),
        }
    }

    pub fn record_injected_lessons(&self, ids: &[String]) {
        if ids.is_empty() {
            return;
        }
        let mut acc = self.accumulator.lock();
        for id in ids {
            if !acc.injected_lesson_ids.iter().any(|existing| existing == id) {
                acc.injected_lesson_ids.push(id.clone());
            }
        }
    }

    pub fn finalize_turn(
        &self,
        final_text: Option<String>,
        aborted_reason: Option<String>,
    ) -> Option<TurnRecord> {
        let mut acc = self.accumulator.lock();
        if acc.finalized {
            return None;
        }
        acc.finalized = true;
        if let Some(text) = final_text {
            if acc.response.content.is_none() && !text.is_empty() {
                acc.response.content = Some(text);
            }
        }
        let effective_turn_idx = if self.turn_idx == 0 {
            self.store().session_turn_count(&self.session_id)
        } else {
            self.turn_idx
        };
        let mut record =
            TurnRecord::new(self.session_id.clone(), effective_turn_idx, self.turn_class);
        record.coding_mode = self.coding_mode.clone();
        record.provider = acc.provider.take();
        record.model = acc.model.take();
        record.openai_messages = std::mem::take(&mut acc.openai_messages);
        record.anthropic_messages = std::mem::take(&mut acc.anthropic_messages);
        record.anthropic_system = acc.anthropic_system.take();
        record.response = std::mem::take(&mut acc.response);
        record.tool_outcomes = std::mem::take(&mut acc.tool_outcomes);
        record.cost = std::mem::take(&mut acc.cost);
        record.injected_lesson_ids = std::mem::take(&mut acc.injected_lesson_ids);
        record.completed_ts = Some(Utc::now());
        record.aborted = aborted_reason;

        let weights = self.engine.config_snapshot().signal_weights;
        let scores = super::evaluators::run_fast_evaluators(&record);
        record.reward = super::reward::fuse_signals(&scores, &weights);
        drop(acc);

        if let Err(error) = self.store().append_turn(&record) {
            tracing::warn!(error = %error, "evolution: failed to append turn record");
            return None;
        }
        if let Some(ref response_text) = record.response.content {
            self.engine.record_recent_turn(
                &record.session_id,
                &record.id,
                response_text,
                record.coding_mode.as_deref(),
            );
        }
        if record.reward.final_score >= 0.5 && record.response.content.is_some() {
            let _ = self
                .engine
                .enqueue_distill(super::distiller::DistillRequest { turn: record.clone() });
        }
        let recycling_cfg = self.engine.config_snapshot().recycling.clone();
        if recycling_cfg.enabled {
            if let Some(rstore) = self.engine.recycling_store() {
                match super::recycling::harvest_turn(
                    &rstore,
                    &record,
                    &recycling_cfg,
                    Some(self.engine.workspace_dir()),
                ) {
                    Ok(report) => {
                        if report.stored > 0 {
                            for _ in 0..report.stored {
                                self.engine.note_recycling_harvested();
                            }
                        }
                    }
                    Err(error) => {
                        tracing::debug!(error = %error, "evolution: recycling harvest failed");
                    }
                }
            }
        }
        self.engine.record_turn_signal(&record);
        Some(record)
    }

}

task_local! {
    pub static EVOLUTION_CTX: Option<EvolutionCtx>;
}

pub async fn scope_evolution_ctx<F, R>(ctx: Option<EvolutionCtx>, f: F) -> R
where
    F: std::future::Future<Output = R>,
{
    EVOLUTION_CTX.scope(ctx, f).await
}

pub fn try_ctx() -> Option<EvolutionCtx> {
    EVOLUTION_CTX.try_with(Clone::clone).ok().flatten()
}

pub fn observe_tool_outcome(name: &str, success: bool, latency_ms: Option<u64>) {
    record_tool_outcome(name, success, latency_ms, None, None, None, None);
}

pub fn record_tool_outcome(
    name: &str,
    success: bool,
    latency_ms: Option<u64>,
    exit_code: Option<i32>,
    arguments: Option<&serde_json::Value>,
    output_excerpt: Option<&str>,
    error_excerpt: Option<&str>,
) {
    if let Some(ctx) = try_ctx() {
        let payload_excerpt = build_payload_excerpt(arguments, output_excerpt);
        ctx.observe_tool_outcome(ToolOutcome {
            name: name.to_string(),
            ok: success,
            exit_code,
            latency_ms,
            error_excerpt: error_excerpt.map(|s| truncate_excerpt(s, 480)),
            arguments: arguments.cloned(),
            payload_excerpt,
        });
    }
}

fn build_payload_excerpt(
    arguments: Option<&serde_json::Value>,
    output_excerpt: Option<&str>,
) -> Option<String> {
    let mut combined = String::new();
    if let Some(args) = arguments {
        let s = match args {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        combined.push_str(&truncate_excerpt(&s, 480));
    }
    if let Some(output) = output_excerpt {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&truncate_excerpt(output, 480));
    }
    if combined.is_empty() {
        None
    } else {
        Some(combined)
    }
}

fn truncate_excerpt(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars).collect();
    out.push('…');
    out
}

pub fn record_cost(input_tokens: u64, output_tokens: u64, total_tokens: u64, usd: f64) {
    if let Some(ctx) = try_ctx() {
        ctx.add_cost(input_tokens, output_tokens, total_tokens, usd);
    }
}

pub fn finalize_turn(final_text: Option<String>, aborted: Option<String>) -> Option<TurnRecord> {
    let ctx = try_ctx()?;
    ctx.finalize_turn(final_text, aborted)
}

pub fn set_response_text(text: &str) {
    if let Some(ctx) = try_ctx() {
        ctx.set_response_text(text);
    }
}

pub fn set_thinking_text(text: &str) {
    if let Some(ctx) = try_ctx() {
        ctx.set_thinking_text(text);
    }
}

pub fn record_provider_model(provider: Option<&str>, model: Option<&str>) {
    if let Some(ctx) = try_ctx() {
        ctx.record_provider_model(provider.map(str::to_string), model.map(str::to_string));
    }
}

pub fn snapshot_prompt_messages(messages: &[crate::providers::traits::ChatMessage]) {
    if let Some(ctx) = try_ctx() {
        if !ctx.store().persist_training_data() {
            return;
        }
        let views: Vec<ChatMessageView> = messages
            .iter()
            .map(|m| ChatMessageView {
                role: m.role.clone(),
                content: if m.content.is_empty() {
                    None
                } else {
                    Some(m.content.clone())
                },
                tool_calls: Vec::new(),
                name: None,
                tool_call_id: None,
            })
            .collect();
        ctx.set_prompt_messages(views, Vec::new(), None);
    }
}

pub fn collecting() -> bool {
    try_ctx().is_some()
}

pub fn record_injected_lessons(ids: &[String]) {
    if let Some(ctx) = try_ctx() {
        ctx.record_injected_lessons(ids);
    }
}
