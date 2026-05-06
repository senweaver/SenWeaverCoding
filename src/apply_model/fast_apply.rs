// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Fast-apply tiered refiner.
//!
//! Cursor / Windsurf / Claude Code all run a *small, cheap, fast*
//! model dedicated to "applying" a high-level diff to source code.
//! The main reasoning model produces a rough patch; the fast-apply
//! model rewrites the patch against the actual file so the heuristic
//! locator in [`super::heuristic`] can land it.  Decoupling the two
//! tiers is the single most-impactful latency / cost optimization on
//! the inline-edit hot path.
//!
//! [`FastApplyRefiner`] wraps two [`super::LlmRefiner`] instances
//! (typically backed by [`super::HttpLlmRefiner`] with different
//! `model` strings):
//!
//! - **fast tier** — invoked first.  Runs against the user-configured
//!   `fast_apply_model` with a tight timeout / low temperature so it
//!   resolves common drift / context-mismatch cases in <2 s.
//! - **full tier** — invoked when the fast tier raises an error or
//!   the recursive cap is reached.  Falls back to the heavier
//!   reasoning model so the runner remains correct even when the
//!   fast model can't repair the diff.
//!
//! The refiner is also used by
//! [`super::OpsApplier::apply_unified_diff_with_fast_path`] so any
//! tool that wants to apply a unified diff can opt into the same
//! tiered policy without re-implementing the loop.

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
    let refined = refiner.full.refine(source, seed_diff, hint).await?;
    let outcome = apply_unified_diff(source, &refined, options)?;
    Ok((outcome, refined, FastPathTier::Full))
}
