// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyOptions {

    pub max_fuzz: usize,

    pub dry_run: bool,

    pub validate: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

impl Default for ApplyOptions {
    fn default() -> Self {
        Self {
            max_fuzz: 2,
            dry_run: false,
            validate: true,
            path: None,
        }
    }
}

impl ApplyOptions {
    #[must_use]
    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyOutcome {
    pub applied: String,

    pub hunks_exact: usize,

    pub hunks_fuzzy: usize,

    pub hunks_failed: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error("diff has no hunks")]
    EmptyDiff,
    #[error("failed to apply {failed} of {total} hunks. Details: {details:?}")]
    HunkMismatch {
        failed: usize,
        total: usize,
        details: Vec<String>,
    },
    #[error("validator rejected the result: {reasons:?}")]
    Validation { reasons: Vec<String> },
    #[error("malformed unified diff: {0}")]
    Parse(String),
    #[error("hunk body line counts do not match @@ header: {details}")]
    HunkCountMismatch { details: String },

    #[error("llm refine failed: {0}")]
    LlmError(String),
}

pub trait Applier {

    fn apply(
        &self,
        source: &str,
        diff: &str,
        opts: &ApplyOptions,
    ) -> Result<ApplyOutcome, ApplyError>;
    fn name(&self) -> &'static str;
}
