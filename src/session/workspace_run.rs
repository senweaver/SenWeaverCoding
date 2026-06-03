// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::Path;

pub fn workspace_key_from_path(path: &Path, fallback_session_id: &str) -> String {
    let s = path.to_string_lossy().trim().to_string();
    if s.is_empty() {
        return format!("__solo::{fallback_session_id}");
    }
    normalize_workspace_key(&s, fallback_session_id)
}

pub fn normalize_workspace_key(raw: &str, fallback_session_id: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return format!("__solo::{fallback_session_id}");
    }
    let unified = trimmed.replace('\\', "/");
    let no_trailing = unified.trim_end_matches('/').to_string();
    let final_key = if no_trailing.is_empty() {
        unified
    } else {
        no_trailing
    };
    if cfg!(target_os = "windows") {
        final_key.to_lowercase()
    } else {
        final_key
    }
}
