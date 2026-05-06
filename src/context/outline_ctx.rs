// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! document outline attached to the query context.
//!
//! Thin adapter around [`crate::code_intel::outline::OutlineEntry`]
//! that carries the originating file path so the context pipeline can
//! render per-file outline blocks without a separate lookup table.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct OutlineNode {
    pub path: PathBuf,
    pub kind: String,
    pub name: String,
    pub line: u32,
    pub children: Vec<OutlineNode>,
}

impl OutlineNode {
    #[must_use]
    pub fn leaf(path: PathBuf, kind: String, name: String, line: u32) -> Self {
        Self {
            path,
            kind,
            name,
            line,
            children: Vec::new(),
        }
    }
}
