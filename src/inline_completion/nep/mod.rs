// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Next-Edit Prediction (NEP / "Agent Tab").
//!
//! Cursor's *Agent Tab* / Windsurf's *Cascade Next Edit* / Claude
//! Code's `--continue` rely on the same primitive: look at the user's
//! most-recent edit and predict the **next** edit they're likely to
//! make.  The classic case is "user changed a function signature →
//! suggest updating the call sites"; another is "user added an
//! import → suggest finishing the type usage that motivated it".
//!
//! The module exposes:
//!
//! - [`NepProvider`] — the dispatch trait.  All providers consume an
//!   [`NepRequest`] (recent edit + open-buffer view) and produce an
//!   [`NepResponse`] (zero or more [`NepSuggestion`]s).  Each
//!   suggestion carries a unified diff so the surface can re-use the
//!   M1.5 [`crate::apply_model::FastApplyRefiner`] / heuristic apply
//!   path without any extra plumbing.
//! - [`HeuristicNep`] — pure-Rust pattern-matcher that handles the
//!   "obvious" cases (signature change / unused import resolution /
//!   newly-introduced TODO).  Always available, no provider needed.
//! - [`LlmNep`] — provider-backed predictor that asks the configured
//!   LLM to produce a unified diff aimed at the next edit.  Used when
//!   the heuristic finds nothing or when the user explicitly tabs
//!   through to the "smarter" suggestion.
//! - [`NepRegistry`] — fan-out helper that runs each registered
//!   provider until one returns a non-empty response, mirroring the
//!   shape of [`crate::inline_completion::InlineCompletionRegistry`].
//!
//! The triggering surface (TUI editor / GUI editor / CLI `sen
//! complete --next-edit`) calls [`NepRegistry::predict`] after a save
//! or after an accepted ghost-text suggestion; the renderer then
//! displays the resulting diff alongside the cursor and lets the
//! user accept it via Tab.
//!
//! ## Design choices
//!
//! - **Diff-shaped output, not free text.**  By emitting a unified
//!   diff we can apply the suggestion through the same M1.5
//!   `apply_unified_diff_with_fast_path` pipeline that handles
//!   `Cmd+K`.  This keeps the apply path single-source-of-truth and
//!   guarantees the same locking / journaling guarantees.
//! - **Stateless providers.**  Edit-history bookkeeping lives in the
//!   call site; providers see a snapshot in [`NepRequest`].  This
//!   avoids the classic pitfall of a long-lived predictor cache that
//!   drifts out of sync with the buffer.
//! - **Bounded context.**  The request carries at most a handful of
//!   recent edits (`recent_edits`) and a window of the active file
//!   so an aggressive editor can't blow up the LLM context budget.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

pub mod heuristic;
pub mod llm;
pub mod registry;

pub use heuristic::HeuristicNep;
pub use llm::LlmNep;
pub use registry::NepRegistry;

#[derive(Debug, Clone)]
pub struct RecentEdit {
    pub file_path: PathBuf,
    pub diff: String,

    pub instruction: Option<String>,

    pub since_start_ms: u64,
}

#[derive(Debug, Clone)]
pub struct NepRequest {

    pub active_file: PathBuf,

    pub source: String,

    pub cursor_line: u32,

    pub recent_edits: Vec<RecentEdit>,

    pub workspace_root: PathBuf,

    pub request_id: uuid::Uuid,
}

#[derive(Debug, Clone)]
pub struct NepSuggestion {

    pub file_path: PathBuf,

    pub diff: String,

    pub rationale: String,

    pub confidence: Option<f32>,

    pub origin: &'static str,
}

#[derive(Debug, Clone)]
pub struct NepResponse {
    pub suggestions: Vec<NepSuggestion>,

    pub latency_ms: u64,

    pub provider: String,
}

#[derive(Debug, thiserror::Error)]
pub enum NepError {
    #[error("nep provider {provider} timed out after {timeout_ms}ms")]
    Timeout { provider: String, timeout_ms: u64 },
    #[error("nep provider {provider} failed: {source}")]
    Provider {
        provider: String,
        #[source]
        source: anyhow::Error,
    },
    #[error("nep disabled: {reason}")]
    Disabled { reason: String },
}

#[async_trait]
pub trait NepProvider: Send + Sync {
    async fn predict(&self, req: NepRequest) -> Result<NepResponse, NepError>;
    fn name(&self) -> &'static str;
}

pub async fn apply_suggestion(
    suggestion: &NepSuggestion,
    refiner: Option<&crate::apply_model::FastApplyRefiner>,
    options: &crate::apply_model::ApplyOptions,
) -> Result<(crate::apply_model::ApplyOutcome, crate::apply_model::FastPathTier), anyhow::Error>
{
    let source = tokio::fs::read_to_string(&suggestion.file_path)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "failed to read {}: {e}",
                suggestion.file_path.display()
            )
        })?;
    let (outcome, _final_diff, tier) =
        crate::apply_model::apply_unified_diff_with_fast_path(
            &source,
            &suggestion.diff,
            options,
            refiner,
            Some(suggestion.rationale.as_str()),
        )
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    if let Some(parent) = suggestion.file_path.parent()
        && !parent.as_os_str().is_empty()
    {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    tokio::fs::write(&suggestion.file_path, outcome.applied.as_bytes())
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "failed to write {}: {e}",
                suggestion.file_path.display()
            )
        })?;
    Ok((outcome, tier))
}

pub type NepHandle = Arc<dyn NepProvider>;
