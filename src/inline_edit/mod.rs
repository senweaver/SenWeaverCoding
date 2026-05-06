// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Inline Edit (Cmd+K style) subsystem.
//!
//! Inline Edit lets the user highlight a range of code, type a
//! natural-language instruction ("refactor to use Result", "add
//! inline docs"), and receive a diff preview.  Accepting the preview
//! runs the change through [`apply_model`](crate::apply_model) which
//! handles fuzzy patch application, validation, and checkpoint
//! capture so the user can [`flow_rollback`](crate::tools::flow_rollback)
//! even after the edit.
//!
//! The module is intentionally surface-neutral: GUI / TUI / CLI each
//! instantiate an [`InlineEditRunner`] with their own prompt input
//! channel and diff preview renderer, while sharing the request /
//! runner / preview data types here.

pub mod preview;
pub mod prompts;
pub mod request;
pub mod runner;

pub mod service;

pub mod runtime_config;

pub use preview::{DiffPreview, Hunk};
pub use prompts::build_instruction_prompt;
pub use request::{InlineEditOutcome, InlineEditRequest};
pub use runner::{InlineEditRunner, LlmClient};
pub use runtime_config::{
    build_code_edit_config, build_pipeline_from_config, build_refiner_from_config,
    ApplyModelSection, CodeEditSection, RefineSection, RuntimeConfig, VerificationSection,
};
