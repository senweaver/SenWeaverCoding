// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::lsp::bridge::LspPoolDiagnosticFetcher;
use super::lsp::diag::LspDiagVerifier;
use super::syntactic::SyntacticVerifier;
use super::test_runner::TestRunnerBuilder;
use super::traits::{Artifact, VerificationReport, Verifier};
use crate::services::lsp::LspService;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VerificationPolicy {

    FailFast,

    CollectAll,

    ScoreBased { min_score: f32 },
}

impl Default for VerificationPolicy {
    fn default() -> Self {
        Self::CollectAll
    }
}

#[derive(Debug, Clone)]
pub struct PipelineReport {
    pub reports: Vec<VerificationReport>,
    pub passed: bool,

    pub failed_stages: Vec<&'static str>,
}

impl PipelineReport {

    pub fn all_issues(&self) -> Vec<&super::traits::VerificationIssue> {
        self.reports.iter().flat_map(|r| r.issues.iter()).collect()
    }

    pub fn joined_summary(&self) -> String {
        self.reports
            .iter()
            .filter(|r| !r.summary.is_empty())
            .map(|r| format!("{}={}", r.verifier, r.summary))
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

pub struct VerificationPipeline {
    stages: Vec<Box<dyn Verifier>>,
    policy: VerificationPolicy,
}

impl VerificationPipeline {
    pub fn new(stages: Vec<Box<dyn Verifier>>, policy: VerificationPolicy) -> Self {
        Self { stages, policy }
    }

    pub fn default_for_workspace(root: &Path, lsp: Option<Arc<LspService>>) -> Self {
        let mut stages: Vec<Box<dyn Verifier>> = Vec::new();

        stages.push(Box::new(SyntacticVerifier::new()));

        let mut detected = TestRunnerBuilder::new(root.to_path_buf()).build();
        if detected.is_empty() {

            detected.push(Box::new(super::test_runner::TestRunnerVerifier::dry_run()));
        }
        stages.extend(detected);

        if let Some(svc) = lsp {
            let fetcher = LspPoolDiagnosticFetcher::new(svc, root.to_path_buf());
            let verifier = LspDiagVerifier::new(Arc::new(fetcher))
                .with_timeout_status_summary(true);
            stages.push(Box::new(verifier));
        }

        Self::new(stages, VerificationPolicy::CollectAll)
    }

    pub async fn run(&self, art: &Artifact) -> anyhow::Result<PipelineReport> {
        crate::observability::subsystem_metrics::incr_verification_pipeline_run();

        let mut reports: Vec<VerificationReport> = Vec::with_capacity(self.stages.len());
        let mut failed_stages: Vec<&'static str> = Vec::new();
        let mut short_circuit = false;

        for stage in &self.stages {
            if short_circuit {
                break;
            }
            let stage_name = stage.name();
            let span = tracing::info_span!("verify", stage = stage_name);
            let _enter = span.enter();
            let started = Instant::now();
            let result = stage.verify(art).await;
            let elapsed_ms = started.elapsed().as_millis() as u64;
            match result {
                Ok(report) => {
                    let passed = report.passed;
                    let error_count = report.error_count();
                    tracing::info!(
                        target: "agent.verification.pipeline",
                        stage = stage_name,
                        passed = passed,
                        elapsed_ms = elapsed_ms,
                        error_count = error_count,
                        "stage_completed",
                    );
                    if passed {
                        crate::observability::subsystem_metrics::incr_verification_stage_pass(
                            stage_name,
                        );
                    } else {
                        crate::observability::subsystem_metrics::incr_verification_stage_fail(
                            stage_name,
                        );
                        failed_stages.push(stage_name);
                        if matches!(self.policy, VerificationPolicy::FailFast) {
                            short_circuit = true;
                        }
                    }
                    reports.push(report);
                }
                Err(e) => {
                    tracing::warn!(
                        target: "agent.verification.pipeline",
                        stage = stage_name,
                        elapsed_ms = elapsed_ms,
                        error = %e,
                        "stage_infrastructure_error",
                    );

                    crate::observability::subsystem_metrics::incr_verification_stage_fail(stage_name);
                    failed_stages.push(stage_name);
                    reports.push(VerificationReport::failed(
                        stage_name,
                        Vec::new(),
                        format!("infrastructure error: {e}"),
                    ));
                    if matches!(self.policy, VerificationPolicy::FailFast) {
                        short_circuit = true;
                    }
                }
            }
        }

        let passed = match self.policy {
            VerificationPolicy::FailFast | VerificationPolicy::CollectAll => failed_stages.is_empty(),
            VerificationPolicy::ScoreBased { min_score } => {
                if reports.is_empty() {
                    true
                } else {
                    let pass_count = reports.iter().filter(|r| r.passed).count();
                    let ratio = pass_count as f32 / reports.len() as f32;
                    ratio >= min_score
                }
            }
        };

        if passed {
            crate::observability::subsystem_metrics::incr_verification_pipeline_pass();
        } else {
            crate::observability::subsystem_metrics::incr_verification_pipeline_fail();
        }

        Ok(PipelineReport {
            reports,
            passed,
            failed_stages,
        })
    }

    pub fn stage_count(&self) -> usize {
        self.stages.len()
    }

    pub async fn run_on_workspace(&self, root: &Path) -> anyhow::Result<PipelineReport> {
        let artifact = Artifact {
            kind: super::traits::ArtifactKind::Workspace,
            path: root.to_path_buf(),
            contents: String::new(),
            language: super::traits::Language::Unknown,
        };
        let report = self.run(&artifact).await?;
        if report.passed {
            crate::observability::code_intel_metrics::incr_code_edit_batch_verify_pass();
        } else {
            crate::observability::code_intel_metrics::incr_code_edit_batch_verify_fail();
        }
        Ok(report)
    }

    pub fn stage_names(&self) -> Vec<&'static str> {
        self.stages.iter().map(|s| s.name()).collect()
    }
}
