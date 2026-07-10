// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::security::SecurityPolicy;
use std::path::PathBuf;

pub fn resolve_write_target(security: &SecurityPolicy, path: &str) -> Result<PathBuf, String> {
    if !security.can_act() {
        return Err("Action blocked: autonomy is read-only".to_string());
    }
    if security.is_rate_limited() {
        return Err("Rate limit exceeded: too many actions in the last hour".to_string());
    }
    if !security.is_path_allowed(path) {
        return Err(format!("Path not allowed by security policy: {path}"));
    }
    let full = security.resolve_tool_path(path);
    let Some(parent) = full.parent() else {
        return Err("Invalid path: missing parent directory".to_string());
    };
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("Could not create output directory: {e}"))?;
    let resolved_parent = std::fs::canonicalize(parent)
        .map_err(|e| format!("Failed to resolve file path: {e}"))?;
    if !security.is_resolved_path_allowed(&resolved_parent) {
        return Err(security.resolved_path_violation_message(&resolved_parent));
    }
    if !crate::security::sandbox_allows_path(&resolved_parent) {
        return Err(format!(
            "Sandbox policy denies write to {}",
            resolved_parent.display()
        ));
    }
    let Some(file_name) = full.file_name() else {
        return Err("Invalid path: missing file name".to_string());
    };
    let target = resolved_parent.join(file_name);
    if security.is_runtime_config_path(&target) {
        return Err(security.runtime_config_violation_message(&target));
    }
    if let Ok(meta) = std::fs::symlink_metadata(&target) {
        if meta.file_type().is_symlink() {
            return Err(format!(
                "Refusing to write through symlink: {}",
                target.display()
            ));
        }
    }
    if !security.record_action() {
        return Err("Rate limit exceeded: action budget exhausted".to_string());
    }
    Ok(target)
}

pub fn resolve_write_dir(security: &SecurityPolicy, path: &str) -> Result<PathBuf, String> {
    if !security.can_act() {
        return Err("Action blocked: autonomy is read-only".to_string());
    }
    if !security.is_path_allowed(path) {
        return Err(format!("Path not allowed by security policy: {path}"));
    }
    let full = security.resolve_tool_path(path);
    std::fs::create_dir_all(&full)
        .map_err(|e| format!("Could not create output directory: {e}"))?;
    let resolved = std::fs::canonicalize(&full)
        .map_err(|e| format!("Failed to resolve directory: {e}"))?;
    if !security.is_resolved_path_allowed(&resolved) {
        return Err(security.resolved_path_violation_message(&resolved));
    }
    if !crate::security::sandbox_allows_path(&resolved) {
        return Err(format!(
            "Sandbox policy denies write to {}",
            resolved.display()
        ));
    }
    if !security.record_action() {
        return Err("Rate limit exceeded: action budget exhausted".to_string());
    }
    Ok(resolved)
}

pub fn resolve_read_source(security: &SecurityPolicy, path: &str) -> Result<PathBuf, String> {
    if !security.is_path_allowed(path) {
        return Err(format!("Path not allowed by security policy: {path}"));
    }
    let full = security.resolve_tool_path(path);
    let resolved = std::fs::canonicalize(&full)
        .map_err(|e| format!("Failed to resolve file path `{path}`: {e}"))?;
    if !security.is_resolved_path_allowed(&resolved) {
        return Err(security.resolved_path_violation_message(&resolved));
    }
    Ok(resolved)
}

pub fn is_table_row(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.ends_with('|') && t.matches('|').count() >= 2
}

pub fn is_table_separator(line: &str) -> bool {
    let t = line.trim();
    if !is_table_row(t) {
        return false;
    }
    let inner = t.trim_matches('|');
    inner
        .split('|')
        .map(|c| c.trim())
        .all(|cell| !cell.is_empty() && cell.chars().all(|c| c == '-' || c == ':' || c == ' '))
}

pub fn split_table_row(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    let mut cells: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut escaped = false;
    for ch in t.chars() {
        if escaped {
            if ch != '|' && ch != '\\' {
                cur.push('\\');
            }
            cur.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '|' {
            cells.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(ch);
        }
    }
    if escaped {
        cur.push('\\');
    }
    cells.push(cur.trim().to_string());
    cells
}

pub fn parse_page_ranges(spec: &str, total: usize) -> Result<Vec<u32>, String> {
    let mut out: Vec<u32> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (start, end) = if let Some((a, b)) = part.split_once('-') {
            let a = a.trim();
            let b = b.trim();
            let start = if a.is_empty() {
                1
            } else {
                a.parse::<usize>().map_err(|_| format!("invalid page '{a}'"))?
            };
            let end = if b.is_empty() {
                total
            } else {
                b.parse::<usize>().map_err(|_| format!("invalid page '{b}'"))?
            };
            (start, end)
        } else {
            let n = part
                .parse::<usize>()
                .map_err(|_| format!("invalid page '{part}'"))?;
            (n, n)
        };
        if start == 0 || end == 0 || start > end {
            return Err(format!("invalid page range '{part}'"));
        }
        for p in start..=end.min(total) {
            if seen.insert(p) {
                out.push(p as u32);
            }
        }
    }
    if out.is_empty() {
        return Err("no valid pages selected".to_string());
    }
    Ok(out)
}
