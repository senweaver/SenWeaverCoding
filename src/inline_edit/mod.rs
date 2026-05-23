// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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
