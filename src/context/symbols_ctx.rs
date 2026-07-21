// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct SymbolSnapshot {

    pub name: String,

    pub kind: String,

    pub path: PathBuf,

    pub line: u32,

    pub line_end: u32,

    pub signature: Option<String>,

    pub dependents: Vec<String>,

    pub imports: Vec<String>,
}
