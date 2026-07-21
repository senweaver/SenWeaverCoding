// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use std::sync::Arc;

use crate::agent::verification::{
    Artifact as VerifyArtifact, ArtifactKind, Language, VerificationPipeline,
};
use crate::apply_model::{
    ApplyError, ApplyOptions, HeuristicApplier, LlmRefiner,
    llm_refine::{FailureKind, PreviousAttempt},
    traits::Applier,
    validator::ValidationKind,
};

use super::preview::DiffPreview;
use super::prompts::build_instruction_prompt;
use super::request::{InlineEditOutcome, InlineEditRequest};

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete_diff(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, anyhow::Error>;
    fn name(&self) -> &'static str;
}

#[derive(Debug, Clone)]
pub struct RunnerOptions {
    pub apply: ApplyOptions,

    pub checkpoint: bool,

    pub max_refine_attempts: u8,

    pub max_recursive_attempts: u8,
}

impl Default for RunnerOptions {
    fn default() -> Self {
        Self {
            apply: ApplyOptions {
                max_fuzz: 3,
                dry_run: false,
                validate: true,
                path: None,
            },
            checkpoint: true,
            max_refine_attempts: 1,
            max_recursive_attempts: 2,
        }
    }
}

pub struct InlineEditRunner {
    llm: Arc<dyn LlmClient>,
    refiner: Option<Arc<dyn LlmRefiner>>,

    pipeline: Option<Arc<VerificationPipeline>>,
    opts: RunnerOptions,
}

impl std::fmt::Debug for InlineEditRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InlineEditRunner")
            .field("llm", &self.llm.name())
            .field("refiner", &self.refiner.as_ref().map(|r| r.name()))
            .field("pipeline_stages", &self.pipeline.as_ref().map(|p| p.stage_count()))
            .field("opts", &self.opts)
            .finish_non_exhaustive()
    }
}

impl InlineEditRunner {
    pub fn new(llm: Arc<dyn LlmClient>, opts: RunnerOptions) -> Self {
        Self {
            llm,
            refiner: None,
            pipeline: None,
            opts,
        }
    }

    #[must_use]
    pub fn with_refiner(mut self, refiner: Arc<dyn LlmRefiner>) -> Self {
        self.refiner = Some(refiner);
        self
    }

    #[must_use]
    pub fn with_fast_refiner(
        self,
        fast_refiner: Arc<crate::apply_model::FastApplyRefiner>,
    ) -> Self {
        let dyn_refiner: Arc<dyn LlmRefiner> = fast_refiner;
        self.with_refiner(dyn_refiner)
    }

    #[must_use]
    pub fn with_pipeline(mut self, pipeline: Arc<VerificationPipeline>) -> Self {
        self.pipeline = Some(pipeline);
        self
    }

    pub async fn run(
        &self,
        source: &str,
        req: InlineEditRequest,
    ) -> Result<InlineEditOutcome, InlineEditError> {
        let system = super::prompts::SYSTEM_PROMPT;
        let user = build_instruction_prompt(&req);
        let raw_diff = self
            .llm
            .complete_diff(system, &user)
            .await
            .map_err(InlineEditError::Llm)?;

        let preview = DiffPreview::from_unified(&raw_diff);
        if preview.hunks.is_empty() {
            return Err(InlineEditError::EmptyDiff);
        }

        let applier = HeuristicApplier;
        let (final_diff, outcome) = match applier.apply(source, &raw_diff, &self.opts.apply) {
            Ok(outcome) => (raw_diff, outcome),
            Err(first_err) => {
                self.refine_and_retry(source, &req, raw_diff, first_err)
                    .await?
            }
        };

        crate::observability::subsystem_metrics::incr_inline_edit_run();
        crate::observability::subsystem_metrics::incr_inline_edit_hunks_applied(
            (outcome.hunks_exact + outcome.hunks_fuzzy) as u64,
        );

        let (mut issues, verification_failed) = self.verify_applied(&req, &outcome.applied).await;

        let (final_diff, outcome) = if let Some((summary, kind)) = verification_failed {
            let synthetic = ApplyError::Validation {
                reasons: vec![summary],
            };
            match self
                .refine_and_retry_with_failure(
                    source,
                    &req,
                    final_diff.clone(),
                    synthetic,
                    kind,
                )
                .await
            {
                Ok((d, o)) => {
                    let (refined_issues, refined_failed) =
                        self.verify_applied(&req, &o.applied).await;
                    match refined_failed {
                        None => {
                            issues = refined_issues;
                            (d, o)
                        }
                        Some((refined_summary, _)) => {
                            crate::observability::subsystem_metrics::incr_inline_edit_validator_failure();
                            return Err(InlineEditError::Apply(ApplyError::Validation {
                                reasons: vec![format!(
                                    "refined result still fails verification: {refined_summary}"
                                )],
                            }));
                        }
                    }
                }
                Err(_) => {

                    (final_diff, outcome)
                }
            }
        } else {
            (final_diff, outcome)
        };

        let checkpoint_id = if self.opts.checkpoint {
            push_checkpoint(&req, source)
        } else {
            None
        };

        Ok(InlineEditOutcome {
            diff: final_diff,
            applied: outcome.applied,
            hunks_exact: outcome.hunks_exact,
            hunks_fuzzy: outcome.hunks_fuzzy,
            validator_issues: issues,
            checkpoint_id,
        })
    }

    async fn verify_applied(
        &self,
        req: &InlineEditRequest,
        applied: &str,
    ) -> (Vec<String>, Option<(String, Option<FailureKind>)>) {
        let mut issues = Vec::new();
        let mut verification_failed: Option<(String, Option<FailureKind>)> = None;
        if let Some(pipeline) = self.pipeline.clone() {
            let art = VerifyArtifact {
                kind: ArtifactKind::Patch,
                path: req.file_path.clone(),
                contents: applied.to_string(),
                language: Language::from_path(&req.file_path),
            };
            match pipeline.run(&art).await {
                Ok(report) => {
                    issues.extend(
                        report
                            .reports
                            .iter()
                            .flat_map(|r| r.issues.iter().map(|i| i.message.clone())),
                    );
                    if !report.passed {
                        crate::observability::subsystem_metrics::incr_inline_edit_validator_failure();
                        let summary = report.joined_summary();
                        let kind = pipeline_to_failure_kind(&report);
                        verification_failed = Some((summary, kind));
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        target: "inline_edit.runner",
                        error = %e,
                        "verification pipeline raised an infrastructure error"
                    );
                    issues.push(format!("verification pipeline error: {e}"));
                }
            }
        } else if self.opts.apply.validate {
            let lang_id = Language::from_path(&req.file_path).grammar_id();
            let report = crate::apply_model::validate_bytes_with_lang(applied, lang_id);
            if !report.is_ok() {
                crate::observability::subsystem_metrics::incr_inline_edit_validator_failure();
                let summary = report
                    .issues
                    .iter()
                    .map(|i| format!("{}={}", i.code, i.message))
                    .collect::<Vec<_>>()
                    .join(" | ");
                let kind = report.first_kind().map(validation_to_failure_kind);
                verification_failed = Some((summary, kind));
            }
            issues.extend(report.issues.iter().map(|i| i.message.clone()));
        }
        (issues, verification_failed)
    }

    async fn refine_and_retry(
        &self,
        source: &str,
        req: &InlineEditRequest,
        first_diff: String,
        first_err: ApplyError,
    ) -> Result<(String, crate::apply_model::ApplyOutcome), InlineEditError> {
        let kind = apply_error_to_failure_kind(&first_err);
        self.refine_and_retry_with_failure(source, req, first_diff, first_err, kind)
            .await
    }

    async fn refine_and_retry_with_failure(
        &self,
        source: &str,
        req: &InlineEditRequest,
        first_diff: String,
        first_err: ApplyError,
        first_kind: Option<FailureKind>,
    ) -> Result<(String, crate::apply_model::ApplyOutcome), InlineEditError> {
        let Some(refiner) = self.refiner.clone() else {
            return Err(InlineEditError::Apply(first_err));
        };
        if self.opts.max_refine_attempts == 0 {
            return Err(InlineEditError::Apply(first_err));
        }

        let applier = HeuristicApplier;

        let runner_budget = self
            .opts
            .max_refine_attempts
            .saturating_add(self.opts.max_recursive_attempts);
        let refiner_budget = refiner.max_recursive_attempts().saturating_add(1);
        let total_budget = runner_budget.min(refiner_budget) as usize;

        let mut last_diff = first_diff;
        let mut last_err = first_err;
        let mut last_kind = first_kind;
        let mut prev: Option<PreviousAttempt> = None;

        for attempt_idx in 0..total_budget {
            if attempt_idx > 0 {
                crate::observability::code_intel_metrics::incr_apply_model_refine_recursive_attempt();
            }
            let hint = req.instruction.as_str();
            let refined = match refiner
                .refine_with_context(
                    source,
                    &last_diff,
                    Some(hint),
                    last_kind.as_ref(),
                    prev.as_ref(),
                    attempt_idx as u8,
                )
                .await
            {
                Ok(d) => d,
                Err(e) => {

                    return Err(InlineEditError::RefineFailed {
                        apply_error: last_err,
                        refine_error: e,
                    });
                }
            };
            match applier.apply(source, &refined, &self.opts.apply) {
                Ok(outcome) => return Ok((refined, outcome)),
                Err(e) => {
                    let next_kind = apply_error_to_failure_kind(&e);
                    prev = Some(PreviousAttempt {
                        diff: refined.clone(),
                        error: e.to_string(),
                    });
                    last_diff = refined;
                    last_err = e;
                    last_kind = next_kind;
                }
            }
        }
        Err(InlineEditError::Apply(last_err))
    }
}

fn apply_error_to_failure_kind(err: &ApplyError) -> Option<FailureKind> {
    match err {
        ApplyError::EmptyDiff => None,
        ApplyError::HunkMismatch { .. } => Some(FailureKind::ContextMismatch),
        ApplyError::Parse(_) => Some(FailureKind::ContextMismatch),
        ApplyError::LlmError(_) => None,
        ApplyError::Validation { reasons } => {

            let joined = reasons.join(" ");
            let lower = joined.to_ascii_lowercase();
            if lower.contains("tree_sitter") || lower.contains("tree-sitter") {
                Some(FailureKind::TreeSitterError {
                    node_kind: "unknown".into(),
                    line: 0,
                })
            } else if lower.contains("brace")
                || lower.contains("paren")
                || lower.contains("bracket")
            {
                Some(FailureKind::BracketUnbalanced { line: 0 })
            } else if lower.contains("offset") || lower.contains("drift") {
                Some(FailureKind::LineDrift { delta: 0 })
            } else if lower.contains("compile") || lower.contains("error[e") {
                Some(FailureKind::CompileError {
                    code: None,
                    line: 0,
                })
            } else {
                Some(FailureKind::ContextMismatch)
            }
        }
    }
}

fn validation_to_failure_kind(kind: &ValidationKind) -> FailureKind {
    match kind {
        ValidationKind::Empty => FailureKind::ContextMismatch,
        ValidationKind::BracketUnbalanced { line, .. } => {
            FailureKind::BracketUnbalanced { line: *line }
        }
        ValidationKind::TreeSitterError { node_kind, line } => FailureKind::TreeSitterError {
            node_kind: node_kind.clone(),
            line: *line,
        },
        ValidationKind::ValidatorCustom(_) => FailureKind::ContextMismatch,
    }
}

fn pipeline_to_failure_kind(
    report: &crate::agent::verification::PipelineReport,
) -> Option<FailureKind> {
    use crate::agent::verification::IssueSeverity;
    for r in &report.reports {
        if r.passed {
            continue;
        }

        for issue in &r.issues {
            if !matches!(issue.severity, IssueSeverity::Error) {
                continue;
            }
            let lower = issue.message.to_ascii_lowercase();
            if r.verifier == "test_runner" {
                return Some(FailureKind::CompileError {
                    code: extract_rustc_code(&issue.message),
                    line: issue.line,
                });
            }
            if r.verifier == "syntactic" {
                return Some(FailureKind::TreeSitterError {
                    node_kind: extract_node_kind(&issue.message)
                        .unwrap_or_else(|| "syntax".into()),
                    line: issue.line,
                });
            }
            if r.verifier == "lsp_diag" {
                return Some(FailureKind::CompileError {
                    code: None,
                    line: issue.line,
                });
            }
            if lower.contains("brace")
                || lower.contains("paren")
                || lower.contains("bracket")
            {
                return Some(FailureKind::BracketUnbalanced { line: issue.line });
            }
            return Some(FailureKind::ContextMismatch);
        }
    }
    None
}

fn extract_rustc_code(msg: &str) -> Option<String> {
    let start = msg.find("error[")? + "error[".len();
    let end = msg[start..].find(']')? + start;
    Some(msg[start..end].to_string())
}

fn extract_node_kind(msg: &str) -> Option<String> {
    if let Some(rest) = msg.strip_prefix("missing token: ") {
        return Some(format!("missing:{}", rest.trim().trim_matches('"')));
    }
    if let Some(rest) = msg.strip_prefix("error in ") {
        return Some(rest.trim().to_string());
    }
    None
}

fn push_checkpoint(req: &InlineEditRequest, source: &str) -> Option<String> {
    use crate::agent::flows::{Artifact, Checkpoint, global_checkpoint_store};
    let store = global_checkpoint_store();
    let id = format!("inline_edit:{}", req.request_id);
    let artifact = Artifact::new("inline_edit", source).with_language(
        req.file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string(),
    );
    let cp = Checkpoint::new(
        id.clone(),
        format!(
            "inline_edit:{}",
            req.file_path
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("?")
        ),
        vec![artifact],
        vec![],
    );
    store.push(cp);
    Some(id)
}

#[derive(Debug, thiserror::Error)]
pub enum InlineEditError {
    #[error("llm call failed: {0}")]
    Llm(#[source] anyhow::Error),
    #[error("empty diff returned by the llm")]
    EmptyDiff,
    #[error("apply failed: {0}")]
    Apply(#[from] ApplyError),

    #[error("apply failed ({apply_error}); refine also failed: {refine_error}")]
    RefineFailed {
        #[source]
        apply_error: ApplyError,
        refine_error: ApplyError,
    },
}
