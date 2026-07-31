// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::services::lsp::{
    DiagnosticSeverity, LspDiagnostic, canonical_diag_key, detect_language,
    infer_workspace_root,
};

const MAX_FILES: usize = 8;
const MAX_REPORTED_ERRORS: usize = 12;
const PER_FILE_TIMEOUT: Duration = Duration::from_secs(3);
const TOTAL_BUDGET: Duration = Duration::from_secs(8);

pub type DiagnosticsBaseline = HashMap<PathBuf, HashSet<String>>;

fn diag_signature(diag: &LspDiagnostic) -> String {
    format!(
        "{}:{}:{}",
        diag.range.start_line,
        diag.code.as_deref().unwrap_or(""),
        diag.message
    )
}

pub async fn baseline(paths: &[PathBuf]) -> DiagnosticsBaseline {
    let mut out: DiagnosticsBaseline = HashMap::new();
    let Some(services) = crate::services::try_get_services() else {
        return out;
    };
    for path in paths.iter().take(MAX_FILES) {
        let key = canonical_diag_key(path);
        let signatures: HashSet<String> = services
            .lsp
            .get_diagnostics(&key)
            .await
            .iter()
            .filter(|d| matches!(d.severity, DiagnosticSeverity::Error))
            .map(diag_signature)
            .collect();
        out.insert(key, signatures);
    }
    out
}

pub async fn new_error_feedback(
    paths: &[PathBuf],
    baseline: &DiagnosticsBaseline,
) -> Option<String> {
    let services = crate::services::try_get_services()?;
    let started = std::time::Instant::now();
    let mut lines: Vec<String> = Vec::new();
    let mut total_new = 0usize;

    let mut seen: HashSet<PathBuf> = HashSet::new();
    for path in paths.iter().take(MAX_FILES) {
        if started.elapsed() > TOTAL_BUDGET {
            break;
        }
        let key = canonical_diag_key(path);
        if !seen.insert(key.clone()) {
            continue;
        }
        let Some(language) = detect_language(path) else {
            continue;
        };
        let root = infer_workspace_root(path)
            .unwrap_or_else(|| path.parent().unwrap_or(path).to_path_buf());
        if !services.lsp.is_server_running(language, &root).await {
            continue;
        }
        let refreshed = tokio::time::timeout(
            PER_FILE_TIMEOUT,
            services.lsp.refresh_diagnostics(path, language, &root),
        )
        .await;
        let diags = match refreshed {
            Ok(Ok(diags)) => diags,
            _ => continue,
        };
        let known = baseline.get(&key);
        for diag in diags
            .iter()
            .filter(|d| matches!(d.severity, DiagnosticSeverity::Error))
        {
            if known.is_some_and(|set| set.contains(&diag_signature(diag))) {
                continue;
            }
            total_new += 1;
            if lines.len() < MAX_REPORTED_ERRORS {
                lines.push(format_error_line(path, &root, diag));
            }
        }
    }

    if total_new == 0 {
        return None;
    }
    let mut out = format!(
        "\n\nPost-edit diagnostics: {total_new} new error(s) introduced by this edit:"
    );
    for line in &lines {
        out.push('\n');
        out.push_str("  ");
        out.push_str(line);
    }
    if total_new > lines.len() {
        out.push_str(&format!(
            "\n  ... and {} more (run the diagnostics tool for the full list)",
            total_new - lines.len()
        ));
    }
    out.push_str("\nFix these before continuing if they are caused by your change.");
    Some(out)
}

fn format_error_line(file: &Path, workspace: &Path, diag: &LspDiagnostic) -> String {
    let display_path = crate::util::path_relative_to(file, workspace)
        .unwrap_or_else(|| file.to_path_buf());
    let line = diag.range.start_line + 1;
    let col = diag.range.start_character + 1;
    let mut suffix = String::new();
    if let Some(code) = diag.code.as_ref() {
        suffix.push_str(&format!(" [{code}]"));
    }
    format!(
        "{}:{line}:{col}: {}{suffix}",
        display_path.display(),
        diag.message
    )
}
