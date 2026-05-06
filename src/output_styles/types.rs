// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Output style types — mirrors claude-code-typescript-src`outputStyles/loadOutputStylesDir.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStyleSource {
    Builtin,
    Project,
    User,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputStyleDefinition {
    pub name: String,
    pub description: String,
    pub source: OutputStyleSource,

    pub system_prompt_addition: String,

    pub file_path: Option<String>,
}
