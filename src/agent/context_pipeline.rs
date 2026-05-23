// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;

use crate::providers::traits::{ChatMessage, Provider};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreserveTag {

    EditTarget,

    FocusFile,

    CriticalSystem,
}

impl PreserveTag {

    #[must_use]
    pub fn as_slug(self) -> &'static str {
        match self {
            PreserveTag::EditTarget => "edit_target",
            PreserveTag::FocusFile => "focus_file",
            PreserveTag::CriticalSystem => "critical_system",
        }
    }
}

#[must_use]
pub fn preserve_from_focus_paths(
    history: &[ChatMessage],
    focus_paths: &[std::path::PathBuf],
) -> Vec<PreservedMessage> {
    let mut out = Vec::new();
    if let Some(first) = history.first() {
        if first.role == "system" {
            out.push(PreservedMessage {
                index: 0,
                tag: PreserveTag::CriticalSystem,
            });
        }
    }
    if focus_paths.is_empty() {
        return out;
    }
    let needles: Vec<String> = focus_paths
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    for (i, msg) in history.iter().enumerate() {
        if out.iter().any(|p| p.index == i) {
            continue;
        }

        let meta_hit = needles.iter().any(|n| {
            !n.is_empty()
                && msg
                    .metadata
                    .get("source_path")
                    .and_then(|v| v.as_str())
                    == Some(n.as_str())
        });

        let content_hit = !meta_hit
            && needles
                .iter()
                .any(|n| !n.is_empty() && msg.content.contains(n.as_str()));
        if meta_hit || content_hit {
            out.push(PreservedMessage {
                index: i,
                tag: PreserveTag::EditTarget,
            });
        }
    }
    out
}

pub fn tag_message_source_path(msg: &mut ChatMessage, path: &std::path::Path) {
    msg.metadata.insert(
        "source_path".to_string(),
        serde_json::Value::String(path.to_string_lossy().into_owned()),
    );
}

#[derive(Debug, Clone, Copy)]
pub struct PreservedMessage {

    pub index: usize,
    pub tag: PreserveTag,
}

pub struct PipelineState {
    pub target_tokens: usize,
    pub current_tokens: usize,
    pub tokens_before_stage: usize,
    pub iteration: u32,

    pub preserved: Vec<PreservedMessage>,
}

impl PipelineState {
    pub fn new(target_tokens: usize, current_tokens: usize) -> Self {
        Self {
            target_tokens,
            current_tokens,
            tokens_before_stage: current_tokens,
            iteration: 0,
            preserved: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_preserved(mut self, preserved: Vec<PreservedMessage>) -> Self {
        self.preserved = preserved;
        self
    }

    pub fn over_target(&self) -> bool {
        self.current_tokens > self.target_tokens
    }

    pub fn is_preserved(&self, idx: usize) -> bool {
        self.preserved.iter().any(|p| p.index == idx)
    }
}

#[derive(Debug, Clone)]
pub struct StageReport {
    pub stage_name: &'static str,
    pub executed: bool,
    pub tokens_before: usize,
    pub tokens_after: usize,
}

impl StageReport {
    pub fn skipped(name: &'static str, tokens: usize) -> Self {
        Self {
            stage_name: name,
            executed: false,
            tokens_before: tokens,
            tokens_after: tokens,
        }
    }
}

pub trait Stage: Send + Sync {
    fn name(&self) -> &'static str;

    fn should_run(&self, state: &PipelineState) -> bool {
        state.over_target()
    }

    fn apply(&self, history: &mut Vec<ChatMessage>, state: &mut PipelineState) -> StageReport;
}

pub struct HardTrimStage {
    pub max_messages: usize,
}

impl Stage for HardTrimStage {
    fn name(&self) -> &'static str {
        "hard_trim"
    }

    fn should_run(&self, _state: &PipelineState) -> bool {

        true
    }

    fn apply(&self, history: &mut Vec<ChatMessage>, state: &mut PipelineState) -> StageReport {
        let before = history.len();
        if before > self.max_messages {

            let keep_tail = self.max_messages.saturating_sub(1);
            let split_at = before.saturating_sub(keep_tail);
            let system = history.first().cloned();

            let preserved_in_range: Vec<usize> = state
                .preserved
                .iter()
                .map(|p| p.index)
                .filter(|i| *i >= 1 && *i < split_at)
                .collect();

            if preserved_in_range.is_empty() {
                history.drain(1..split_at);
            } else {

                let mut kept_tail: Vec<ChatMessage> = Vec::with_capacity(before);
                for (i, m) in history.drain(..).enumerate() {
                    if i == 0 {
                        kept_tail.push(m);
                    } else if i >= split_at {
                        kept_tail.push(m);
                    } else if preserved_in_range.contains(&i) {
                        kept_tail.push(m);

                        crate::observability::code_intel_metrics::incr_context_preserve_skip_compress();
                    }
                }
                *history = kept_tail;
            }

            if let Some(sys) = system {
                if history.first().map(|m| m.role.as_str()) != Some("system") {
                    history.insert(0, sys);
                }
            }
        }

        let after_tokens = history.iter().map(|m| m.content.len() / 4).sum::<usize>();
        let report = StageReport {
            stage_name: self.name(),
            executed: before != history.len(),
            tokens_before: state.current_tokens,
            tokens_after: after_tokens,
        };
        state.current_tokens = after_tokens;
        report
    }
}

pub struct ContextPipeline {
    stages: Vec<Box<dyn Stage>>,
}

impl ContextPipeline {
    pub fn new() -> Self {
        Self { stages: Vec::new() }
    }

    pub fn push<S: Stage + 'static>(mut self, stage: S) -> Self {
        self.stages.push(Box::new(stage));
        self
    }

    pub fn run(
        &self,
        history: &mut Vec<ChatMessage>,
        target_tokens: usize,
        current_tokens: usize,
    ) -> Vec<StageReport> {
        self.run_with_preserved(history, target_tokens, current_tokens, Vec::new())
    }

    pub fn run_with_preserved(
        &self,
        history: &mut Vec<ChatMessage>,
        target_tokens: usize,
        current_tokens: usize,
        preserved: Vec<PreservedMessage>,
    ) -> Vec<StageReport> {
        for p in &preserved {
            crate::observability::code_intel_metrics::incr_context_preserve_hit(p.tag.as_slug());
        }
        let mut state =
            PipelineState::new(target_tokens, current_tokens).with_preserved(preserved);
        let mut reports = Vec::with_capacity(self.stages.len());

        for stage in &self.stages {
            state.tokens_before_stage = state.current_tokens;
            state.iteration += 1;

            if !stage.should_run(&state) {
                reports.push(StageReport::skipped(stage.name(), state.current_tokens));
                continue;
            }
            let report = stage.apply(history, &mut state);
            reports.push(report);

            if !state.over_target() {
                break;
            }
        }
        reports
    }
}

impl Default for ContextPipeline {
    fn default() -> Self {
        Self::new().push(HardTrimStage { max_messages: 50 })
    }
}

#[async_trait]
pub trait AsyncStage: Send + Sync {
    fn name(&self) -> &'static str;

    fn should_run(&self, state: &PipelineState) -> bool {
        state.over_target()
    }

    async fn apply(&self, history: &mut Vec<ChatMessage>, state: &mut PipelineState)
    -> StageReport;
}

pub struct SyncAdapter<S: Stage> {
    pub stage: S,
}

#[async_trait]
impl<S: Stage + 'static> AsyncStage for SyncAdapter<S> {
    fn name(&self) -> &'static str {
        self.stage.name()
    }

    fn should_run(&self, state: &PipelineState) -> bool {
        self.stage.should_run(state)
    }

    async fn apply(
        &self,
        history: &mut Vec<ChatMessage>,
        state: &mut PipelineState,
    ) -> StageReport {
        self.stage.apply(history, state)
    }
}

pub struct LlmCompressStage {
    pub compressor: super::context_compressor::ContextCompressor,
    pub provider: std::sync::Arc<dyn Provider>,
    pub model: String,
}

#[async_trait]
impl AsyncStage for LlmCompressStage {
    fn name(&self) -> &'static str {
        "llm_compress"
    }

    async fn apply(
        &self,
        history: &mut Vec<ChatMessage>,
        state: &mut PipelineState,
    ) -> StageReport {
        let before = state.current_tokens;
        let preserved_indices: Vec<usize> =
            state.preserved.iter().map(|p| p.index).collect();
        match self
            .compressor
            .compress_if_needed_with_preserved(
                history,
                self.provider.as_ref(),
                &self.model,
                &preserved_indices,
            )
            .await
        {
            Ok(result) => {
                state.current_tokens = result.tokens_after;
                StageReport {
                    stage_name: self.name(),
                    executed: result.compressed,
                    tokens_before: before,
                    tokens_after: result.tokens_after,
                }
            }
            Err(e) => {
                tracing::warn!("llm_compress stage failed, applying hard-trim fallback: {e}");
                let dropped = hard_trim_fallback(history, state);
                let after_tokens = history.iter().map(|m| m.content.len() / 4).sum::<usize>();
                state.current_tokens = after_tokens;
                StageReport {
                    stage_name: self.name(),
                    executed: dropped > 0,
                    tokens_before: before,
                    tokens_after: after_tokens,
                }
            }
        }
    }
}

fn hard_trim_fallback(history: &mut Vec<ChatMessage>, state: &PipelineState) -> usize {
    if history.len() <= 2 {
        return 0;
    }
    let has_system = history
        .first()
        .map(|m| m.role.as_str() == "system")
        .unwrap_or(false);
    let start = usize::from(has_system);
    let total_non_system = history.len() - start;
    if total_non_system <= 4 {
        return 0;
    }
    let keep_tail = total_non_system / 2;
    let drop_end = start + (total_non_system - keep_tail);

    let preserved: std::collections::HashSet<usize> = state
        .preserved
        .iter()
        .map(|p| p.index)
        .filter(|i| *i >= start && *i < drop_end)
        .collect();

    let mut dropped: usize = 0;
    let original = std::mem::take(history);
    let mut kept: Vec<ChatMessage> = Vec::with_capacity(original.len());
    for (i, msg) in original.into_iter().enumerate() {
        if i < start || i >= drop_end || preserved.contains(&i) {
            kept.push(msg);
        } else {
            dropped += 1;
        }
    }
    if dropped > 0 {
        let note = ChatMessage::system(format!(
            "[Context truncated: {} earlier messages dropped after compression failed]",
            dropped
        ));
        let insert_at = usize::from(has_system);
        let safe_insert = insert_at.min(kept.len());
        kept.insert(safe_insert, note);
    }
    *history = kept;
    dropped
}

pub struct AsyncContextPipeline {
    stages: Vec<Box<dyn AsyncStage>>,
}

impl AsyncContextPipeline {
    pub fn new() -> Self {
        Self { stages: Vec::new() }
    }

    pub fn push(mut self, stage: Box<dyn AsyncStage>) -> Self {
        self.stages.push(stage);
        self
    }

    pub fn push_sync<S: Stage + 'static>(self, stage: S) -> Self {
        self.push(Box::new(SyncAdapter { stage }))
    }

    pub async fn run(
        &self,
        history: &mut Vec<ChatMessage>,
        target_tokens: usize,
        current_tokens: usize,
    ) -> Vec<StageReport> {
        self.run_with_preserved(history, target_tokens, current_tokens, Vec::new())
            .await
    }

    pub async fn run_with_preserved(
        &self,
        history: &mut Vec<ChatMessage>,
        target_tokens: usize,
        current_tokens: usize,
        preserved: Vec<PreservedMessage>,
    ) -> Vec<StageReport> {
        for p in &preserved {
            crate::observability::code_intel_metrics::incr_context_preserve_hit(p.tag.as_slug());
        }
        let mut state =
            PipelineState::new(target_tokens, current_tokens).with_preserved(preserved);
        let mut reports = Vec::with_capacity(self.stages.len());

        for stage in &self.stages {
            state.tokens_before_stage = state.current_tokens;
            state.iteration += 1;
            if !stage.should_run(&state) {
                reports.push(StageReport::skipped(stage.name(), state.current_tokens));
                continue;
            }
            let report = stage.apply(history, &mut state).await;
            reports.push(report);
            if !state.over_target() {
                break;
            }
        }
        reports
    }
}

impl Default for AsyncContextPipeline {
    fn default() -> Self {
        Self::new().push_sync(HardTrimStage { max_messages: 50 })
    }
}
