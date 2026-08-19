// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use async_trait::async_trait;

use super::llm_refine::{FailureKind, PreviousAttempt};
use super::traits::ApplyError;
use super::{HeuristicApplier, LlmRefiner};
use crate::apply_model::heuristic::apply_unified_diff;
use crate::apply_model::traits::{ApplyOptions, ApplyOutcome, Applier};

pub struct FastApplyRefiner {
    fast: Option<Arc<dyn LlmRefiner>>,
    full: Arc<dyn LlmRefiner>,

    prefer_fast_for_attempts: u8,
}

impl std::fmt::Debug for FastApplyRefiner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FastApplyRefiner")
            .field("fast", &self.fast.as_ref().map(|r| r.name()))
            .field("full", &self.full.name())
            .field("prefer_fast_for_attempts", &self.prefer_fast_for_attempts)
            .finish()
    }
}

impl FastApplyRefiner {

    #[must_use]
    pub fn new(fast: Option<Arc<dyn LlmRefiner>>, full: Arc<dyn LlmRefiner>) -> Self {
        Self {
            fast,
            full,
            prefer_fast_for_attempts: 1,
        }
    }

    #[must_use]
    pub fn with_prefer_fast_for_attempts(mut self, n: u8) -> Self {
        self.prefer_fast_for_attempts = n;
        self
    }

    fn tier_for(&self, attempt_idx: u8) -> &Arc<dyn LlmRefiner> {
        match self.fast.as_ref() {
            Some(fast) if attempt_idx < self.prefer_fast_for_attempts => fast,
            _ => &self.full,
        }
    }

    pub async fn merge_lazy_snippet(
        &self,
        source: &str,
        edit_snippet: &str,
        instruction: Option<&str>,
        path: Option<&std::path::Path>,
    ) -> Result<String, ApplyError> {
        let merged = self.merge_full_file(source, edit_snippet, instruction).await?;
        if source.len() >= MERGE_SHRINK_GUARD_MIN_SOURCE_LEN && merged.len() < source.len() / 2 {
            return Err(ApplyError::LlmError(
                "merged result shrank by more than half; rejecting as likely truncated".to_string(),
            ));
        }
        let report = super::validator::validate_edit(Some(source), &merged, path);
        if report.is_confident_failure() {
            return Err(ApplyError::LlmError(format!(
                "merged result failed tree-sitter validation: {}",
                report.advisory_summary()
            )));
        }
        Ok(merged)
    }

    async fn merge_full_file(
        &self,
        source: &str,
        edit_snippet: &str,
        instruction: Option<&str>,
    ) -> Result<String, ApplyError> {
        if let Some(fast) = self.fast.as_ref() {
            if fast.supports_full_file_merge() {
                if let Ok(out) = fast.merge_full_file(source, edit_snippet, instruction).await {
                    return Ok(out);
                }
            }
        }
        if self.full.supports_full_file_merge() {
            return self.full.merge_full_file(source, edit_snippet, instruction).await;
        }
        Err(ApplyError::LlmError(
            "no refiner tier supports full-file merge".to_string(),
        ))
    }
}

#[async_trait]
impl LlmRefiner for FastApplyRefiner {
    async fn refine(
        &self,
        source: &str,
        failed_diff: &str,
        hint: Option<&str>,
    ) -> Result<String, ApplyError> {

        if let Some(fast) = self.fast.as_ref() {
            crate::observability::code_intel_metrics::incr_apply_model_refine_attempt();
            match fast.refine(source, failed_diff, hint).await {
                Ok(out) => {
                    crate::observability::code_intel_metrics::incr_apply_model_refine_success();
                    return Ok(out);
                }
                Err(err) => {
                    tracing::debug!(
                        target: "apply_model.fast_apply",
                        error = %err,
                        fast = fast.name(),
                        "fast tier failed; escalating to full refiner"
                    );
                }
            }
        }
        self.full.refine(source, failed_diff, hint).await
    }

    async fn refine_with_context(
        &self,
        source: &str,
        failed_diff: &str,
        hint: Option<&str>,
        failure: Option<&FailureKind>,
        prev: Option<&PreviousAttempt>,
        attempt_idx: u8,
    ) -> Result<String, ApplyError> {
        let tier = self.tier_for(attempt_idx).clone();
        tier
            .refine_with_context(source, failed_diff, hint, failure, prev, attempt_idx)
            .await
    }

    fn max_recursive_attempts(&self) -> u8 {

        let full_cap = self.full.max_recursive_attempts();
        full_cap.saturating_add(self.prefer_fast_for_attempts)
    }

    fn name(&self) -> &'static str {
        "fast_apply_refiner"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FastPathTier {

    Heuristic,

    Fast,

    Full,
}

pub struct LadderRefinedContent {
    pub contents: String,
    pub encoding: Option<String>,
    pub tier: FastPathTier,
    pub pre_sha256: Option<String>,
}

pub fn runtime_ladder_refiner() -> Option<Arc<FastApplyRefiner>> {
    let services = crate::services::try_get_services()?;
    let config = services.config();
    if !config.agent_runtime.apply_ladder_enabled {
        return None;
    }
    crate::inline_edit::service::default_fast_refiner(&config)
}

pub async fn refine_failing_diff_to_content(
    refiner: &FastApplyRefiner,
    path: &std::path::Path,
    raw_diff: &str,
    max_fuzz: usize,
) -> Option<LadderRefinedContent> {
    let path_for_read = path.to_path_buf();
    let raw_bytes = tokio::task::spawn_blocking(move || std::fs::read(&path_for_read))
        .await
        .ok()?
        .ok()?;
    if crate::tools::file::encoding::is_probably_binary(&raw_bytes) {
        return None;
    }
    let pre_sha256 = super::edit_op::sha256_hex(&raw_bytes);
    let (source, encoding_label) =
        crate::tools::file::encoding::decode_for_edit(&raw_bytes).ok()?;
    let options = ApplyOptions {
        max_fuzz,
        dry_run: false,
        validate: true,
        path: Some(path.to_path_buf()),
    };
    if HeuristicApplier.apply(&source, raw_diff, &options).is_ok() {
        return None;
    }
    match apply_unified_diff_with_fast_path(&source, raw_diff, &options, Some(refiner), None)
        .await
    {
        Ok((outcome, _final_diff, tier)) => {
            tracing::info!(
                target: "apply_model.fast_apply",
                path = %path.display(),
                tier = ?tier,
                "apply ladder recovered a failing diff"
            );
            let encoding = if crate::tools::file::encoding::is_utf8_label(encoding_label) {
                None
            } else {
                Some(encoding_label.to_string())
            };
            Some(LadderRefinedContent {
                contents: outcome.applied,
                encoding,
                tier,
                pre_sha256: Some(pre_sha256),
            })
        }
        Err(err) => {
            tracing::debug!(
                target: "apply_model.fast_apply",
                path = %path.display(),
                error = %err,
                "apply ladder could not recover failing diff"
            );
            None
        }
    }
}

pub async fn apply_unified_diff_with_fast_path(
    source: &str,
    raw_diff: &str,
    options: &ApplyOptions,
    refiner: Option<&FastApplyRefiner>,
    hint: Option<&str>,
) -> Result<(ApplyOutcome, String, FastPathTier), ApplyError> {
    let applier = HeuristicApplier;
    match applier.apply(source, raw_diff, options) {
        Ok(outcome) => Ok((outcome, raw_diff.to_string(), FastPathTier::Heuristic)),
        Err(first_err) => {
            let Some(refiner) = refiner else {
                return Err(first_err);
            };

            if let Some(fast) = refiner.fast.as_ref() {
                match fast.refine(source, raw_diff, hint).await {
                    Ok(refined) => match apply_unified_diff(source, &refined, options) {
                        Ok(outcome) => return Ok((outcome, refined, FastPathTier::Fast)),
                        Err(_) => {

                            return apply_via_full_tier(
                                refiner,
                                source,
                                &refined,
                                options,
                                hint,
                            )
                            .await;
                        }
                    },
                    Err(err) => {
                        tracing::debug!(
                            target: "apply_model.fast_apply",
                            error = %err,
                            "fast refiner errored; escalating"
                        );
                    }
                }
            }

            apply_via_full_tier(refiner, source, raw_diff, options, hint).await
        }
    }
}

async fn apply_via_full_tier(
    refiner: &FastApplyRefiner,
    source: &str,
    seed_diff: &str,
    options: &ApplyOptions,
    hint: Option<&str>,
) -> Result<(ApplyOutcome, String, FastPathTier), ApplyError> {
    match refiner.full.refine(source, seed_diff, hint).await {
        Ok(refined) => match apply_unified_diff(source, &refined, options) {
            Ok(outcome) => Ok((outcome, refined, FastPathTier::Full)),
            Err(diff_err) => {
                merge_full_file_fallback(refiner, source, seed_diff, options, hint, diff_err)
                    .await
                    .map(|(outcome, merged)| (outcome, merged, FastPathTier::Full))
            }
        },
        Err(refine_err) => {
            merge_full_file_fallback(refiner, source, seed_diff, options, hint, refine_err)
                .await
                .map(|(outcome, merged)| (outcome, merged, FastPathTier::Full))
        }
    }
}

const MERGE_SHRINK_GUARD_MIN_SOURCE_LEN: usize = 1024;

async fn merge_full_file_fallback(
    refiner: &FastApplyRefiner,
    source: &str,
    edit_snippet: &str,
    options: &ApplyOptions,
    hint: Option<&str>,
    prior_err: ApplyError,
) -> Result<(ApplyOutcome, String), ApplyError> {
    match refiner.merge_full_file(source, edit_snippet, hint).await {
        Ok(merged) => {
            if source.len() >= MERGE_SHRINK_GUARD_MIN_SOURCE_LEN
                && merged.len() < source.len() / 2
            {
                tracing::warn!(
                    target: "apply_model.fast_apply",
                    source_len = source.len(),
                    merged_len = merged.len(),
                    "full-file merge result shrank by more than half; rejecting as likely truncated"
                );
                return Err(prior_err);
            }
            if options.validate {
                let report = super::validator::validate_edit(
                    Some(source),
                    &merged,
                    options.path.as_deref(),
                );
                if report.is_confident_failure() {
                    tracing::warn!(
                        target: "apply_model.fast_apply",
                        issues = %report.advisory_summary(),
                        "full-file merge result failed validation; rejecting"
                    );
                    return Err(prior_err);
                }
            }
            crate::observability::code_intel_metrics::incr_apply_model_refine_success();
            let outcome = ApplyOutcome {
                applied: merged.clone(),
                hunks_exact: 0,
                hunks_fuzzy: 1,
                hunks_failed: 0,
            };
            Ok((outcome, merged))
        }
        Err(_) => Err(prior_err),
    }
}
