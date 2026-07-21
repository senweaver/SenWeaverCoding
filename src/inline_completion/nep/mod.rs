// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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
    workspace_root: &std::path::Path,
) -> Result<(crate::apply_model::BatchOutcome, crate::apply_model::FastPathTier), anyhow::Error> {
    use crate::apply_model::{EditOrigin, OpsApplier};

    // Hard workspace-containment check: a hallucinated/absolute suggestion path
    // must never escape the workspace. Do NOT add the file's own parent to
    // allowed_roots (that made the containment guard a no-op and let the model
    // write anywhere on disk).
    if !crate::util::path_is_within(&suggestion.file_path, workspace_root) {
        anyhow::bail!(
            "NEP suggestion path escapes workspace: {}",
            suggestion.file_path.display()
        );
    }

    if let Some(parent) = suggestion.file_path.parent()
        && !parent.as_os_str().is_empty()
    {
        tokio::fs::create_dir_all(parent).await.ok();
    }

    let applier = OpsApplier::locked_for_workspace(workspace_root.to_path_buf());

    let mut options = options.clone();
    if options.path.is_none() {
        options.path = Some(suggestion.file_path.clone());
    }

    let (outcome, tier) = applier
        .apply_unified_diff_with_fast_path(
            suggestion.file_path.clone(),
            &suggestion.diff,
            &options,
            refiner,
            Some(suggestion.rationale.as_str()),
            EditOrigin::InlineEdit,
        )
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    crate::session::record_write_for_current_session(&suggestion.file_path);
    Ok((outcome, tier))
}

pub type NepHandle = Arc<dyn NepProvider>;
