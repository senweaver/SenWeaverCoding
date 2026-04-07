// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Output style types — mirrors claude-code-typescript-src`outputStyles/loadOutputStylesDir.ts`.

use serde::{Deserialize, Serialize};

/// Source of an output style definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStyleSource {
    Builtin,
    Project,
    User,
}

/// Loaded output style definition (prompt additions from builtin/project/user sources).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputStyleDefinition {
    pub name: String,
    pub description: String,
    pub source: OutputStyleSource,
    /// System prompt addition injected when the style is active.
    pub system_prompt_addition: String,
    /// File path for user/project styles (None for builtin).
    pub file_path: Option<String>,
}
