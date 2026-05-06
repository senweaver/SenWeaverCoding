// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Artifact verification subsystem (D2.1).
//!
//! Provides a uniform [`Verifier`] trait with three built-in
//! implementations exercising three different verification modes:
//!
//! - [`syntactic::SyntacticVerifier`] — tree-sitter parse-error check
//!   (language-aware, in-process, microsecond-scale).
//! - [`lsp_diag::LspDiagVerifier`] — LSP `textDocument/diagnostics`
//!   roundtrip via an injected client.
//! - [`test_runner::TestRunnerVerifier`] — external test command
//!   (`cargo check`, `pytest -x`, `npm test --silent`) dispatched by
//!   language.
//!
//! The trait is deliberately small and synchronous-shaped (`async`
//! only): concrete verifiers are free to fan out internally
//! (e.g. spawning a child process) without leaking detail to callers.
//! Callers compose verifiers sequentially; a failing verifier does not
//! short-circuit the caller — the caller decides whether to retry
//! (see M5 `PlanExecVerify`).

pub mod lsp_bridge;
pub mod lsp_diag;

pub mod pipeline;
pub mod syntactic;
pub mod test_runner;
pub mod traits;

pub use lsp_bridge::{LspPoolDiagnosticFetcher, infer_lsp_language_id};
pub use lsp_diag::LspDiagVerifier;
pub use pipeline::{PipelineReport, VerificationPipeline, VerificationPolicy};
pub use syntactic::SyntacticVerifier;
pub use test_runner::{TestRunnerBuilder, TestRunnerConfig, TestRunnerVerifier};
pub use traits::{
    Artifact, ArtifactKind, IssueSeverity, Language, VerificationIssue, VerificationReport,
    Verifier,
};
