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
