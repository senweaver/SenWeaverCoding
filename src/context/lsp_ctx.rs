// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! LSP snapshot attached to the query context.
//!
//! Captures a tiny slice of what the running language server has told
//! us about the focus files: number of diagnostics and (optionally) a
//! condensed hover blurb.  The full LSP payload is much larger; the
//! context layer only needs enough to steer the LLM toward fixes.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct LspSnapshot {
    pub path: PathBuf,

    pub diagnostics: usize,

    pub summary: String,

    pub hover: Option<String>,
}

impl LspSnapshot {
    #[must_use]
    pub fn empty(path: PathBuf) -> Self {
        Self {
            path,
            diagnostics: 0,
            summary: String::new(),
            hover: None,
        }
    }
}
