// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! fast, safe patch application engine.
//!
//! Apply Model is the common substrate behind:
//!   * `tools::file_edit` / `tools::patch_apply` / `tools::multi_edit`,
//!   * `inline_edit::runner` Cmd+K diff application, and
//!   * `agent::flows::builtins::CodeEditFlow` write step.
//!
//! The engine is deliberately provider-independent: the LLM supplies
//! a unified diff (or a structured edit), Apply Model converts it into
//! a safe byte-level mutation that honours line-number drift and UTF-8
//! boundaries.
//!
//! The default implementation is a pure-Rust heuristic matcher
//! ([`heuristic::HeuristicApplier`]).  `apply-model-llm`
//! feature enables a follow-up refinement pass using the provider
//! layer — the trait point is reserved in [`llm_refine`] for that
//! expansion.

pub mod edit_op;

pub mod fast_apply;
pub mod heuristic;

pub mod hunk_renderer;
pub mod llm_refine;

pub mod lock_manager_provider;
pub mod ops_applier;

pub mod prompts;
pub mod traits;
pub mod validator;

pub use edit_op::{
    EditAnchor, EditBatch, EditOp, EditOrigin, NotebookCellOp, PreconditionError, ScopeKind,
};
pub use fast_apply::{
    FastApplyRefiner, FastPathTier, apply_unified_diff_with_fast_path,
};
pub use hunk_renderer::UnifiedHunkRenderer;
pub use heuristic::{
    HeuristicApplier, LocateContext, LocateError, LocateOutcome, LocateStrategy, NamedScope,
    apply_unified_diff,
};
pub use llm_refine::{HttpLlmRefiner, LlmRefiner, ScriptedRefiner};
pub use lock_manager_provider::LockManagerProvider;
pub use ops_applier::{
    ApplyBatchError, BatchOutcome, BatchPreview, BatchValidator, BatchValidatorError,
    LockGuard, LockProvider, LockProviderError, NoopBatchValidator, NoopLockProvider, OpOutcome,
    OpsApplier, RegionLockRequest, RollbackError, UnifiedDiffPreview,
};
pub use traits::{Applier, ApplyError, ApplyOptions, ApplyOutcome};
pub use validator::{
    ValidationIssue, ValidationKind, ValidationReport, validate_bytes, validate_bytes_with_lang,
};
