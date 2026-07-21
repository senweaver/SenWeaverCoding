// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;

use serde_json::Value;

const MAX_PATHS_PER_CALL: usize = 8;

pub fn note_tool_focus_paths(tool_name: &str, args: &Value) {
    let paths = extract_tool_paths(tool_name, args);
    if paths.is_empty() {
        return;
    }
    crate::context::builder::FocusPathRegistry::note(&paths);
}

pub fn extract_result_paths(tool_name: &str, result_output: &str) -> Vec<PathBuf> {
    let workspace = crate::session::current_session_context()
        .map(|c| PathBuf::from(c.workspace_dir))
        .or_else(|| std::env::current_dir().ok());
    let resolve = |raw: &str| -> Option<PathBuf> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        let p = PathBuf::from(trimmed);
        if p.is_absolute() {
            Some(p)
        } else if let Some(ws) = workspace.as_ref() {
            Some(ws.join(p))
        } else {
            Some(p)
        }
    };

    let mut out: Vec<PathBuf> = Vec::new();
    match tool_name {
        "glob_edit" => {
            for line in result_output.lines() {
                if let Some(rest) = line.trim_start().strip_prefix("\u{2713} Edited: ") {
                    if let Some(abs) = resolve(rest) {
                        if !out.contains(&abs) {
                            out.push(abs);
                        }
                    }
                }
            }
        }
        "code_xfile_refactor" => {
            if let Ok(v) = serde_json::from_str::<Value>(result_output) {
                if let Some(files) = v.get("files").and_then(Value::as_array) {
                    for f in files {
                        let raw = f
                            .as_str()
                            .map(|s| s.to_string())
                            .or_else(|| f.get("file").and_then(Value::as_str).map(String::from));
                        if let Some(raw) = raw {
                            if let Some(abs) = resolve(&raw) {
                                if !out.contains(&abs) {
                                    out.push(abs);
                                }
                            }
                        }
                    }
                }
            }
        }
        "write_plan" => {
            for token in result_output.split(|c: char| c.is_whitespace() || c == ':' || c == ',') {
                let cleaned = token.trim_matches(|c: char| {
                    matches!(c, '`' | '"' | '\'' | '(' | ')' | '[' | ']')
                });
                if cleaned.len() < 4 || !(cleaned.contains('/') || cleaned.contains('\\')) {
                    continue;
                }
                let has_code_ext = std::path::Path::new(cleaned)
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| {
                        matches!(
                            e,
                            "rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "py" | "go"
                                | "java" | "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hh"
                                | "hxx" | "json" | "toml" | "md"
                        )
                    });
                if !has_code_ext {
                    continue;
                }
                if let Some(abs) = resolve(cleaned) {
                    if !out.contains(&abs) {
                        out.push(abs);
                    }
                }
            }
        }
        _ => {}
    }
    out
}

pub fn extract_tool_paths(tool_name: &str, args: &Value) -> Vec<PathBuf> {
    let mut raw: Vec<String> = Vec::new();
    match tool_name {
        "file_read" | "file_write" | "file_edit" | "restore_file" | "lsp_rename"
        | "lsp_format" | "pdf_read" => {
            if let Some(p) = args.get("path").and_then(Value::as_str) {
                raw.push(p.to_string());
            }
        }
        "notebook_edit" => {
            if let Some(p) = args
                .get("notebook_path")
                .or_else(|| args.get("target_notebook"))
                .or_else(|| args.get("path"))
                .and_then(Value::as_str)
            {
                raw.push(p.to_string());
            }
        }
        "multi_edit" => {
            if let Some(edits) = args.get("edits").and_then(Value::as_array) {
                for e in edits {
                    if let Some(p) = e.get("path").and_then(Value::as_str) {
                        raw.push(p.to_string());
                    }
                }
            }
        }
        "diff_apply" => {
            if let Some(files) = args.get("files").and_then(Value::as_array) {
                for e in files {
                    if let Some(p) = e.get("path").and_then(Value::as_str) {
                        raw.push(p.to_string());
                    }
                }
            }
        }
        "patch_apply" => {
            if let Some(patch) = args.get("patch").and_then(Value::as_str) {
                raw.extend(paths_from_patch_text(patch));
            }
        }
        "copy_path" | "move_path" => {
            if let Some(p) = args.get("destination").and_then(Value::as_str) {
                raw.push(p.to_string());
            }
        }
        _ => return Vec::new(),
    }

    let workspace = crate::session::current_session_context()
        .map(|c| PathBuf::from(c.workspace_dir))
        .or_else(|| std::env::current_dir().ok());

    let mut out: Vec<PathBuf> = Vec::new();
    for candidate in raw.into_iter().take(MAX_PATHS_PER_CALL) {
        let trimmed = candidate.trim();
        if trimmed.is_empty() {
            continue;
        }
        let p = PathBuf::from(trimmed);
        let abs = if p.is_absolute() {
            p
        } else if let Some(ws) = workspace.as_ref() {
            ws.join(p)
        } else {
            p
        };
        if !out.contains(&abs) {
            out.push(abs);
        }
    }
    out
}

fn paths_from_patch_text(patch: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in patch.lines() {
        let candidate = if let Some(rest) = line.strip_prefix("+++ ") {
            Some(rest.trim())
        } else if let Some(rest) = line.strip_prefix("*** Update File:") {
            Some(rest.trim())
        } else if let Some(rest) = line.strip_prefix("*** Add File:") {
            Some(rest.trim())
        } else {
            None
        };
        let Some(candidate) = candidate else { continue };
        if candidate.is_empty() || candidate == "/dev/null" {
            continue;
        }
        let cleaned = candidate
            .strip_prefix("b/")
            .or_else(|| candidate.strip_prefix("a/"))
            .unwrap_or(candidate);
        if !cleaned.is_empty() {
            out.push(cleaned.to_string());
        }
        if out.len() >= MAX_PATHS_PER_CALL {
            break;
        }
    }
    out
}
