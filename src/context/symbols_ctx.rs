// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! symbol snapshots attached to the query context.
//!
//! [`SymbolSnapshot`] is a compact, renderer-friendly view over one
//! symbol that the agent currently cares about (typically derived
//! from `focus_files` + [`crate::code_intel::symbol_graph::SymbolGraph`]).
//! It is intentionally smaller than the full `SymbolEntry`: the
//! context layer only needs name/kind + location + optional
//! signature / dependents for prompt rendering.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct SymbolSnapshot {

    pub name: String,

    pub kind: String,

    pub path: PathBuf,

    pub line: u32,

    pub signature: Option<String>,

    pub dependents: Vec<String>,
}
