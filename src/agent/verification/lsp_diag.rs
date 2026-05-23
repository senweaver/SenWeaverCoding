// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;

use super::traits::{Artifact, IssueSeverity, VerificationIssue, VerificationReport, Verifier};

#[derive(Debug, Clone)]
pub struct LspDiagnostic {
    pub line: u32,
    pub column: u32,
    pub message: String,
    pub severity: IssueSeverity,
}

#[async_trait]
pub trait LspDiagnosticFetcher: Send + Sync {

    async fn fetch(&self, path: &std::path::Path) -> anyhow::Result<Vec<LspDiagnostic>>;
}

pub struct LspDiagVerifier {
    fetcher: std::sync::Arc<dyn LspDiagnosticFetcher>,

    promote_warnings_to_errors: bool,

    timeout_status_summary: bool,
}

impl LspDiagVerifier {
    pub fn new(fetcher: std::sync::Arc<dyn LspDiagnosticFetcher>) -> Self {
        Self {
            fetcher,
            promote_warnings_to_errors: false,
            timeout_status_summary: false,
        }
    }

    pub fn promote_warnings_to_errors(mut self) -> Self {
        self.promote_warnings_to_errors = true;
        self
    }

    pub fn strict(self) -> Self {
        self.promote_warnings_to_errors()
    }

    pub fn with_timeout_status_summary(mut self, on: bool) -> Self {
        self.timeout_status_summary = on;
        self
    }
}

#[async_trait]
impl Verifier for LspDiagVerifier {
    fn name(&self) -> &'static str {
        "lsp_diag"
    }

    async fn verify(&self, artifact: &Artifact) -> anyhow::Result<VerificationReport> {
        let diagnostics = self.fetcher.fetch(&artifact.path).await?;
        let was_empty = diagnostics.is_empty();
        let issues: Vec<VerificationIssue> = diagnostics
            .into_iter()
            .map(|d| VerificationIssue {
                line: d.line,
                column: d.column,
                message: d.message,
                severity: d.severity,
            })
            .collect();

        let is_fail = if self.promote_warnings_to_errors {
            issues
                .iter()
                .any(|i| matches!(i.severity, IssueSeverity::Error | IssueSeverity::Warning))
        } else {
            issues
                .iter()
                .any(|i| matches!(i.severity, IssueSeverity::Error))
        };

        let summary = if was_empty && self.timeout_status_summary {

            "lsp.status=timeout".to_string()
        } else {
            String::new()
        };

        Ok(if is_fail {
            VerificationReport::failed(self.name(), issues, summary)
        } else {
            VerificationReport {
                verifier: self.name(),
                passed: true,
                issues,
                summary,
            }
        })
    }
}
