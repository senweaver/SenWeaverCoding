// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;

use serde_json::Value;

use super::syntactic::SyntacticVerifier;
use super::traits::{Artifact, ArtifactKind, IssueSeverity, Language, Verifier};

const MAX_FILES: usize = 4;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_ISSUES_PER_FILE: usize = 5;

pub fn is_checkable_mutation(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "file_write"
            | "file_edit"
            | "multi_edit"
            | "patch_apply"
            | "diff_apply"
            | "notebook_edit"
            | "lsp_rename"
            | "lsp_format"
            | "restore_file"
    )
}

// Runs an in-process syntax check plus a cached-LSP-diagnostics lookup on the
// files a mutation tool just touched, returning a compact report the loop
// appends to the tool result. Silent when everything is clean.
pub async fn post_edit_check(tool_name: &str, args: &Value) -> Option<String> {
    let paths = crate::agent::tool_handler::focus::extract_tool_paths(tool_name, args);
    if paths.is_empty() {
        return None;
    }

    let lsp_diags = match crate::services::try_get_services() {
        Some(svc) => Some(svc.lsp.get_all_diagnostics().await),
        None => None,
    };

    let mut sections: Vec<String> = Vec::new();
    for path in paths.into_iter().take(MAX_FILES) {
        if path
            .extension()
            .map(|e| e.eq_ignore_ascii_case("ipynb"))
            .unwrap_or(false)
        {
            continue;
        }
        let language = Language::from_path(&path);
        if matches!(language, Language::Unknown) {
            continue;
        }
        match tokio::fs::metadata(&path).await {
            Ok(m) if m.is_file() && m.len() <= MAX_FILE_BYTES => {}
            _ => continue,
        }
        let Ok(contents) = tokio::fs::read_to_string(&path).await else {
            continue;
        };

        let mut file_lines: Vec<String> = Vec::new();

        let artifact = Artifact {
            kind: ArtifactKind::File,
            path: path.clone(),
            contents,
            language,
        };
        if let Ok(report) = SyntacticVerifier::new().verify(&artifact).await {
            if !report.passed {
                let errors: Vec<_> = report
                    .issues
                    .iter()
                    .filter(|i| matches!(i.severity, IssueSeverity::Error))
                    .take(MAX_ISSUES_PER_FILE)
                    .collect();
                if !errors.is_empty() {
                    file_lines.push(format!(
                        "syntax: {} error(s)",
                        report.error_count()
                    ));
                    for issue in errors {
                        file_lines.push(format!(
                            "  - {}:{} {}",
                            issue.line, issue.column, issue.message
                        ));
                    }
                }
            }
        }

        if let Some(all) = lsp_diags.as_ref() {
            use crate::services::lsp::DiagnosticSeverity;
            let key = crate::services::lsp::canonical_diag_key(&path);
            if let Some(diags) = all.get(&key).or_else(|| all.get(&path)) {
                let errors: Vec<_> = diags
                    .iter()
                    .filter(|d| d.severity == DiagnosticSeverity::Error)
                    .take(MAX_ISSUES_PER_FILE)
                    .collect();
                if !errors.is_empty() {
                    file_lines.push(format!(
                        "lsp: {} error diagnostic(s)",
                        diags
                            .iter()
                            .filter(|d| d.severity == DiagnosticSeverity::Error)
                            .count()
                    ));
                    for d in errors {
                        file_lines.push(format!(
                            "  - {}:{} {}",
                            d.range.start_line + 1,
                            d.range.start_character + 1,
                            truncate_message(&d.message)
                        ));
                    }
                }
            }
        }

        if !file_lines.is_empty() {
            sections.push(format!("{}\n{}", display_path(&path), file_lines.join("\n")));
        }
    }

    if sections.is_empty() {
        return None;
    }
    Some(format!(
        "[Post-edit check] Problems detected in the files you just modified. \
         Fix these before proceeding:\n{}\n[/Post-edit check]",
        sections.join("\n")
    ))
}

fn display_path(path: &PathBuf) -> String {
    let workspace = crate::session::current_session_context()
        .map(|c| PathBuf::from(c.workspace_dir));
    if let Some(ws) = workspace {
        if let Ok(rel) = path.strip_prefix(&ws) {
            return rel.to_string_lossy().replace('\\', "/");
        }
    }
    path.to_string_lossy().replace('\\', "/")
}

fn truncate_message(msg: &str) -> String {
    const MAX: usize = 200;
    let flat = msg.replace('\n', " ");
    if flat.chars().count() <= MAX {
        return flat;
    }
    let truncated: String = flat.chars().take(MAX).collect();
    format!("{truncated}\u{2026}")
}
