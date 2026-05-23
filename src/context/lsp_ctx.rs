// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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
