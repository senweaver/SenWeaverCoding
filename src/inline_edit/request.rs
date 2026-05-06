// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Request / outcome shapes shared by CLI / TUI / GUI inline-edit surfaces.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct InlineEditRequest {

    pub file_path: PathBuf,

    pub selection: String,

    pub selection_bytes: (usize, usize),

    pub instruction: String,

    pub context_lines: Option<Vec<String>>,

    pub request_id: uuid::Uuid,
}

#[derive(Debug, Clone)]
pub struct InlineEditOutcome {

    pub diff: String,

    pub applied: String,
    pub hunks_exact: usize,
    pub hunks_fuzzy: usize,

    pub validator_issues: Vec<String>,

    pub checkpoint_id: Option<String>,
}
