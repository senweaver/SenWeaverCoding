// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::services::lsp::{DiagnosticSeverity, LspDiagnostic};

pub struct DiagnosticsTool {
    workspace_root: Arc<RwLock<PathBuf>>,
}

impl DiagnosticsTool {
    pub fn new(workspace_root: Arc<RwLock<PathBuf>>) -> Self {
        Self { workspace_root }
    }

    async fn try_cached_lsp_diagnostics(&self, path: Option<&str>) -> Option<String> {
        let services = crate::services::try_get_services()?;
        let workspace = self.workspace_root.read().clone();
        let all = services.lsp.get_all_diagnostics().await;
        if all.is_empty() {
            return None;
        }

        let filter_abs: Option<PathBuf> = path.map(|p| {
            let pb = PathBuf::from(p);
            if pb.is_absolute() {
                pb
            } else {
                workspace.join(p)
            }
        });

        let mut formatted: Vec<String> = Vec::new();
        let mut entries: Vec<(&PathBuf, &Vec<LspDiagnostic>)> = all.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));

        for (file, diags) in entries {
            if let Some(filter) = filter_abs.as_ref() {
                if !paths_equivalent(file, filter) {
                    continue;
                }
            }
            for diag in diags {
                formatted.push(format_lsp_diagnostic(file, &workspace, diag));
            }
        }

        if formatted.is_empty() {

            return None;
        }

        let joined = formatted.join("\n");
        if joined.len() > 32_768 {
            Some(format!(
                "{}\n\n... truncated ({} bytes total)",
                &joined[..32_768],
                joined.len()
            ))
        } else {
            Some(joined)
        }
    }

    async fn run_check(&self, path: Option<&str>) -> anyhow::Result<String> {
        let workspace = self.workspace_root.read().clone();

        let (cmd, args) = if workspace.join("Cargo.toml").exists() {
            ("cargo", vec!["check", "--message-format=short"])
        } else if workspace.join("package.json").exists() {
            if workspace.join("node_modules/.bin/tsc").exists()
                || workspace.join("tsconfig.json").exists()
            {
                ("npx", vec!["tsc", "--noEmit", "--pretty", "false"])
            } else {
                ("npx", vec!["eslint", "--format", "compact", "."])
            }
        } else if workspace.join("pyproject.toml").exists()
            || workspace.join("setup.py").exists()
        {
            ("python", vec!["-m", "py_compile"])
        } else if workspace.join("go.mod").exists() {
            ("go", vec!["vet", "./..."])
        } else {
            return Ok(
                "No recognized project type found. Supported: Cargo.toml (Rust), \
                        package.json (JS/TS), pyproject.toml (Python), go.mod (Go)."
                    .to_string(),
            );
        };

        let mut command = crate::util::hidden_async_command(cmd);
        command.args(&args).current_dir(&workspace);

        if let Some(file_path) = path {

            if cmd != "cargo" {
                command.arg(file_path);
            }
        }

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            command
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output(),
        )
        .await;

        match output {
            Ok(Ok(out)) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let combined = if stdout.is_empty() {
                    stderr.to_string()
                } else if stderr.is_empty() {
                    stdout.to_string()
                } else {
                    format!("{stdout}\n{stderr}")
                };

                if cmd == "cargo" {
                    if let Some(filter) = path {
                        let filtered: Vec<&str> =
                            combined.lines().filter(|l| l.contains(filter)).collect();
                        if filtered.is_empty() {
                            return Ok(format!("No diagnostics found for {filter}"));
                        }
                        return Ok(filtered.join("\n"));
                    }
                }

                let trimmed = if combined.len() > 32_768 {
                    format!(
                        "{}\n\n... truncated ({} bytes total)",
                        &combined[..32_768],
                        combined.len()
                    )
                } else {
                    combined
                };

                if trimmed.trim().is_empty() {
                    Ok("No diagnostics found  -  project compiles cleanly.".to_string())
                } else {
                    Ok(trimmed)
                }
            }
            Ok(Err(e)) => Ok(format!("Failed to run {cmd}: {e}")),
            Err(_) => Ok(format!("{cmd} timed out after 60 seconds")),
        }
    }
}

#[async_trait]
impl Tool for DiagnosticsTool {
    fn name(&self) -> &str {
        "diagnostics"
    }

    fn description(&self) -> &str {
        "Query project diagnostics (errors, warnings) by running the project's \
         type checker or linter. Supports Rust (cargo check), TypeScript (tsc), \
         Python (py_compile), and Go (go vet). Returns file:line:severity format."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Optional file path to filter diagnostics for a specific file"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let path = args.get("path").and_then(|v| v.as_str());

        if let Some(cached) = self.try_cached_lsp_diagnostics(path).await {
            return Ok(ToolResult {
                success: true,
                output: cached,
                error: None,
            });
        }

        match self.run_check(path).await {
            Ok(output) => Ok(ToolResult {
                success: true,
                output,
                error: None,
            }),
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Diagnostics check failed: {e}")),
            }),
        }
    }
}

fn paths_equivalent(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a.file_name() == b.file_name() && a == b,
    }
}

fn format_lsp_diagnostic(file: &Path, workspace: &Path, diag: &LspDiagnostic) -> String {
    let display_path = file
        .strip_prefix(workspace)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| file.to_path_buf());
    let display = display_path.display();
    let line = diag.range.start_line + 1;
    let col = diag.range.start_character + 1;
    let severity = match diag.severity {
        DiagnosticSeverity::Error => "error",
        DiagnosticSeverity::Warning => "warning",
        DiagnosticSeverity::Information => "info",
        DiagnosticSeverity::Hint => "hint",
    };
    let mut suffix = String::new();
    if let Some(code) = diag.code.as_ref() {
        suffix.push_str(&format!(" [{code}]"));
    }
    if let Some(source) = diag.source.as_ref() {
        suffix.push_str(&format!(" ({source})"));
    }
    format!(
        "{display}:{line}:{col}: {severity}: {message}{suffix}",
        message = diag.message
    )
}
