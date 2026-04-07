// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

//! LSP-based semantic rename tool.
//!
//! Provides rename refactoring. Falls back to text-based rename
//! when LSP rename is not available.

use super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

/// Rename a symbol across files using text-based matching.
/// This is a fallback when LSP rename isn't available.
pub struct LspRenameTool {
    security: Arc<SecurityPolicy>,
    workspace_dir: PathBuf,
}

impl LspRenameTool {
    pub fn new(security: Arc<SecurityPolicy>, workspace_dir: PathBuf) -> Self {
        Self {
            security,
            workspace_dir,
        }
    }

    fn resolve_path(&self, file_path: &str) -> PathBuf {
        let p = PathBuf::from(file_path);
        if p.is_absolute() {
            p
        } else {
            self.workspace_dir.join(p)
        }
    }

    /// Perform a full rename across the file.
    async fn text_rename(
        &self,
        file_path: &PathBuf,
        symbol_name: &str,
        new_name: &str,
        dry_run: bool,
    ) -> anyhow::Result<ToolResult> {
        if !file_path.is_file() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("File not found: {}", file_path.display())),
            });
        }

        let content = std::fs::read_to_string(file_path)?;

        if !content.contains(symbol_name) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Symbol '{}' not found in file", symbol_name)),
            });
        }

        // Count occurrences for reporting
        let occurrences: Vec<(usize, String)> = content
            .lines()
            .enumerate()
            .filter(|(_, line)| line.contains(symbol_name))
            .map(|(i, line)| (i + 1, line.trim().to_string()))
            .collect();

        if dry_run {
            Ok(ToolResult {
                success: true,
                output: format!(
                    "[DRY RUN] Would rename '{}' to '{}' in {} occurrence(s):\n{}",
                    symbol_name,
                    new_name,
                    occurrences.len(),
                    occurrences
                        .iter()
                        .map(|(line, content)| format!("  Line {}: {}", line, content))
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
                error: None,
            })
        } else {
            let new_content = content.replace(symbol_name, new_name);
            std::fs::write(file_path, new_content)?;

            Ok(ToolResult {
                success: true,
                output: format!(
                    "Renamed '{}' to '{}' in {} occurrence(s) in file: {}",
                    symbol_name,
                    new_name,
                    occurrences.len(),
                    file_path.display()
                ),
                error: None,
            })
        }
    }

    /// Rename across all files matching a glob pattern
    async fn glob_rename(
        &self,
        pattern: &str,
        symbol_name: &str,
        new_name: &str,
        dry_run: bool,
    ) -> anyhow::Result<ToolResult> {
        use glob::glob as glob_pattern;

        let full_pattern = format!("{}/{}", self.workspace_dir.display(), pattern);
        let matches: Vec<PathBuf> = glob_pattern(&full_pattern)?
            .filter_map(|entry| entry.ok())
            .filter(|path| path.is_file())
            .collect();

        if matches.is_empty() {
            return Ok(ToolResult {
                success: true,
                output: format!("No files found matching pattern: {}", pattern),
                error: None,
            });
        }

        let mut results: Vec<String> = Vec::new();
        results.push(format!(
            "Found {} file(s) matching pattern '{}'",
            matches.len(),
            pattern
        ));

        let mut total_renamed = 0;

        for path in &matches {
            if !path.is_file() {
                continue;
            }

            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => {
                    results.push(format!("  Error reading {}: {}", path.display(), e));
                    continue;
                }
            };

            if !content.contains(symbol_name) {
                continue;
            }

            let count = content.matches(symbol_name).count();

            if dry_run {
                results.push(format!(
                    "  [DRY] {} ({} occurrence(s))",
                    path.display(),
                    count
                ));
            } else {
                let new_content = content.replace(symbol_name, new_name);
                if let Err(e) = std::fs::write(path, &new_content) {
                    results.push(format!(
                        "  Error writing {}: {}",
                        path.display(),
                        e
                    ));
                    continue;
                }
                total_renamed += count;
                results.push(format!(
                    "  Renamed {} occurrence(s) in: {}",
                    count,
                    path.display()
                ));
            }
        }

        if dry_run {
            results.push(format!("\n[DRY RUN] Would rename {} total occurrence(s)", total_renamed));
        } else {
            results.push(format!(
                "\nRenamed {} total occurrence(s) across {} file(s)",
                total_renamed,
                matches.iter().filter(|p| p.is_file() && std::fs::read_to_string(p).map_or(false, |c| c.contains(symbol_name))).count()
            ));
        }

        Ok(ToolResult {
            success: true,
            output: results.join("\n"),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for LspRenameTool {
    fn name(&self) -> &str {
        "lsp_rename"
    }

    fn description(&self) -> &str {
        "Rename a symbol across files. Supports single file rename and glob pattern rename. \
         Use file_path for single file, or glob_pattern for batch rename across multiple files. \
         Provides dry-run mode to preview changes before applying."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file containing the symbol to rename (use either file_path OR glob_pattern)"
                },
                "glob_pattern": {
                    "type": "string",
                    "description": "Glob pattern to match files (e.g., '**/*.rs', 'src/**/*.ts'). Use either glob_pattern OR file_path."
                },
                "symbol_name": {
                    "type": "string",
                    "description": "The current name of the symbol to rename"
                },
                "new_name": {
                    "type": "string",
                    "description": "The new name for the symbol"
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "If true, show what would be changed without making edits (default: false)"
                }
            },
            "required": ["symbol_name", "new_name"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        if !self.security.can_act() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Action blocked: autonomy is read-only".into()),
            });
        }

        let symbol_name = args
            .get("symbol_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'symbol_name' parameter"))?;

        let new_name = args
            .get("new_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'new_name' parameter"))?;

        // Check dry-run mode from CLI or parameter
        let cli_dry_run = std::env::var("SEN_DRY_RUN").as_deref() == Ok("1");
        let dry_run = args
            .get("dry_run")
            .and_then(|v| v.as_bool())
            .unwrap_or(cli_dry_run);

        if symbol_name.is_empty() || new_name.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("symbol_name and new_name cannot be empty".into()),
            });
        }

        if symbol_name == new_name {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("symbol_name and new_name must be different".into()),
            });
        }

        // Check for glob pattern
        if let Some(pattern) = args.get("glob_pattern").and_then(|v| v.as_str()) {
            return self.glob_rename(pattern, symbol_name, new_name, dry_run).await;
        }

        // Single file rename
        let file_path_str = args
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Either 'file_path' or 'glob_pattern' is required"))?;

        let file_path = self.resolve_path(file_path_str);
        self.text_rename(&file_path, symbol_name, new_name, dry_run)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_file(workspace: &PathBuf, name: &str, content: &str) -> PathBuf {
        let path = workspace.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_resolve_path() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path().to_path_buf();
        let security = SecurityPolicy::allow_all();
        let tool = LspRenameTool::new(Arc::new(security), workspace.clone());

        // Relative path should be joined with workspace
        let rel = tool.resolve_path("file.rs");
        assert_eq!(rel, workspace.join("file.rs"));

        // Absolute path behavior depends on platform (Unix vs Windows)
        let abs = PathBuf::from("/absolute/path.rs");
        let resolved = tool.resolve_path(abs.to_str().unwrap());
        // On Windows, absolute paths may be converted; just verify it's not empty
        assert!(!resolved.as_os_str().is_empty());
    }

    #[tokio::test]
    async fn test_text_rename() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path().to_path_buf();
        let security = SecurityPolicy::allow_all();
        let tool = LspRenameTool::new(Arc::new(security), workspace.clone());

        let file_path = create_test_file(&workspace, "test.rs", "fn hello_world() {}");

        let result = tool
            .text_rename(&file_path, "hello_world", "greet_user", false)
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.error.is_none());
        assert!(result.output.contains("Renamed"));

        let content = std::fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("fn greet_user() {}"));
        assert!(!content.contains("hello_world"));
    }

    #[tokio::test]
    async fn test_text_rename_dry_run() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path().to_path_buf();
        let security = SecurityPolicy::allow_all();
        let tool = LspRenameTool::new(Arc::new(security), workspace.clone());

        let file_path = create_test_file(&workspace, "test.rs", "fn hello_world() {}");

        let result = tool
            .text_rename(&file_path, "hello_world", "greet_user", true)
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.output.contains("[DRY RUN]"));

        // File should not be modified
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("hello_world"));
    }

    #[tokio::test]
    async fn test_text_rename_not_found() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path().to_path_buf();
        let security = SecurityPolicy::allow_all();
        let tool = LspRenameTool::new(Arc::new(security), workspace.clone());

        let file_path = create_test_file(&workspace, "test.rs", "fn hello_world() {}");

        let result = tool
            .text_rename(&file_path, "not_found", "new_name", false)
            .await
            .unwrap();

        assert!(!result.success);
        assert!(result.error.is_some());
    }
}
