// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;

/// Queries project diagnostics (errors and warnings) from cached LSP results
/// or by running a language-appropriate check command.
///
/// Unlike the full LSP tool, this focuses specifically on diagnostics output
/// formatted for LLM consumption (file:line:severity: message).
pub struct DiagnosticsTool {
    workspace: PathBuf,
}

impl DiagnosticsTool {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }

    async fn run_check(&self, path: Option<&str>) -> anyhow::Result<String> {
        // Detect project type and run the appropriate check command
        let (cmd, args) = if self.workspace.join("Cargo.toml").exists() {
            ("cargo", vec!["check", "--message-format=short"])
        } else if self.workspace.join("package.json").exists() {
            if self.workspace.join("node_modules/.bin/tsc").exists()
                || self.workspace.join("tsconfig.json").exists()
            {
                ("npx", vec!["tsc", "--noEmit", "--pretty", "false"])
            } else {
                ("npx", vec!["eslint", "--format", "compact", "."])
            }
        } else if self.workspace.join("pyproject.toml").exists()
            || self.workspace.join("setup.py").exists()
        {
            ("python", vec!["-m", "py_compile"])
        } else if self.workspace.join("go.mod").exists() {
            ("go", vec!["vet", "./..."])
        } else {
            return Ok("No recognized project type found. Supported: Cargo.toml (Rust), \
                        package.json (JS/TS), pyproject.toml (Python), go.mod (Go)."
                .to_string());
        };

        let mut command = tokio::process::Command::new(cmd);
        command.args(&args).current_dir(&self.workspace);

        if let Some(file_path) = path {
            // For Rust, cargo check doesn't accept file args, but we filter the output
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

                // Filter by path if specified for cargo
                if cmd == "cargo" {
                    if let Some(filter) = path {
                        let filtered: Vec<&str> = combined
                            .lines()
                            .filter(|l| l.contains(filter))
                            .collect();
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
                    Ok("No diagnostics found — project compiles cleanly.".to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_name() {
        let tool = DiagnosticsTool::new(PathBuf::from("/tmp"));
        assert_eq!(tool.name(), "diagnostics");
    }

    #[test]
    fn schema_valid() {
        let tool = DiagnosticsTool::new(PathBuf::from("/tmp"));
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
    }
}
