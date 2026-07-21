// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod edit;
pub mod search;

pub(crate) const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".venv",
    ".tox",
    "__pycache__",
    ".idea",
    ".vscode",
    ".next",
];

pub(crate) fn crosses_skip_dir(path: &std::path::Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|name| SKIP_DIRS.contains(&name))
    })
}

pub(crate) const GLOB_WALK_TIMEOUT_SECS: u64 = 10;
