// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod lsp;

pub mod pipeline;
pub mod syntactic;
pub mod test_runner;
pub mod traits;

pub use lsp::bridge::{LspPoolDiagnosticFetcher, infer_lsp_language_id};
pub use lsp::diag::LspDiagVerifier;
pub use pipeline::{PipelineReport, VerificationPipeline, VerificationPolicy};
pub use syntactic::SyntacticVerifier;
pub use test_runner::{TestRunnerBuilder, TestRunnerConfig, TestRunnerVerifier};
pub use traits::{
    Artifact, ArtifactKind, IssueSeverity, Language, VerificationIssue, VerificationReport,
    Verifier,
};
