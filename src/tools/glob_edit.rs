// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

//! Glob-based batch file editing tool.
//!
//! Allows editing multiple files matching a glob pattern in a single operation.
//! This is useful for applying consistent changes across multiple files.
//!
//! Security: All file paths are validated through SecurityPolicy including
//! path traversal prevention, workspace confinement, and symlink escape detection.

use super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use anyhow::Context;
use async_trait::async_trait;
use glob::glob as glob_pattern;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Apply text replacements across all files matching a glob pattern.
/// Similar to `multi_edit` but uses glob patterns for file discovery.
pub struct GlobEditTool {
    security: Arc<SecurityPolicy>,
    workspace_root: PathBuf,
}

impl GlobEditTool {
    pub fn new(security: Arc<SecurityPolicy>, workspace_root: PathBuf) -> Self {
        Self {
            security,
            workspace_root,
        }
    }

    /// Validate that a path is allowed by security policy.
    fn validate_path(&self, path: &str) -> anyhow::Result<()> {
        if !self.security.is_path_allowed(path) {
            anyhow::bail!("Path not allowed by security policy: {path}");
        }
        Ok(())
    }

    /// Resolve and validate a path for workspace confinement (sync version).
    fn resolve_and_validate_path_sync(&self, path: &Path) -> anyhow::Result<PathBuf> {
        let parent = path
            .parent()
            .context("Invalid path: missing parent directory")?;

        // Use std::fs instead of tokio::fs for sync context
        let resolved_parent = std::fs::canonicalize(parent)?;
        if !self.security.is_resolved_path_allowed(&resolved_parent) {
            anyhow::bail!(
                "Path escapes workspace boundary: {}",
                self.security.resolved_path_violation_message(&resolved_parent)
            );
        }

        let file_name = path
            .file_name()
            .context("Invalid path: missing file name")?;
        let resolved = resolved_parent.join(file_name);

        // Check for runtime config files
        if self.security.is_runtime_config_path(&resolved) {
            anyhow::bail!(
                "Refusing to modify runtime config/state file: {}",
                self.security.runtime_config_violation_message(&resolved)
            );
        }

        Ok(resolved)
    }

    /// Resolve and validate a path for workspace confinement (async version).
    async fn resolve_and_validate_path(&self, path: &Path) -> anyhow::Result<PathBuf> {
        let parent = path
            .parent()
            .context("Invalid path: missing parent directory")?;

        let resolved_parent = tokio::fs::canonicalize(parent).await?;
        if !self.security.is_resolved_path_allowed(&resolved_parent) {
            anyhow::bail!(
                "Path escapes workspace boundary: {}",
                self.security.resolved_path_violation_message(&resolved_parent)
            );
        }

        let file_name = path
            .file_name()
            .context("Invalid path: missing file name")?;
        let resolved = resolved_parent.join(file_name);

        // Check for runtime config files
        if self.security.is_runtime_config_path(&resolved) {
            anyhow::bail!(
                "Refusing to modify runtime config/state file: {}",
                self.security.runtime_config_violation_message(&resolved)
            );
        }

        Ok(resolved)
    }

    /// Check for symlink attacks (both direct and parent directory symlinks).
    async fn verify_no_symlink(&self, path: &Path) -> anyhow::Result<()> {
        // Check if the target file itself is a symlink
        if let Ok(meta) = tokio::fs::symlink_metadata(path).await {
            if meta.file_type().is_symlink() {
                anyhow::bail!(
                    "Refusing to edit through symlink: {}",
                    path.display()
                );
            }
        }

        // Recursively check all parent directories for symlinks
        let mut current = path.to_path_buf();
        while let Some(parent) = current.parent() {
            if let Ok(meta) = tokio::fs::symlink_metadata(parent).await {
                if meta.file_type().is_symlink() {
                    anyhow::bail!(
                        "Refusing to edit through symlinked parent directory: {}",
                        parent.display()
                    );
                }
            }
            current = parent.to_path_buf();
        }

        Ok(())
    }

    /// Find all files matching the glob pattern within the workspace
    fn find_matching_files(&self, pattern: &str) -> anyhow::Result<Vec<PathBuf>> {
        // Security: validate pattern before using
        self.validate_path(pattern)?;

        let full_pattern = if pattern.starts_with('/') || pattern.contains(':') {
            pattern.to_string()
        } else {
            format!("{}/{}", self.workspace_root.display(), pattern)
        };

        let matches: Vec<PathBuf> = glob_pattern(&full_pattern)?
            .filter_map(|entry| entry.ok())
            .filter(|path| path.is_file())
            .collect();

        Ok(matches)
    }

    /// Check if a file contains the search string (sync version).
    fn file_contains(&self, path: &Path, search: &str) -> anyhow::Result<bool> {
        let content = std::fs::read_to_string(path)?;
        Ok(content.contains(search))
    }
}

#[async_trait]
impl Tool for GlobEditTool {
    fn name(&self) -> &str {
        "glob_edit"
    }

    fn description(&self) -> &str {
        "Apply text replacements across multiple files matching a glob pattern. \
         Finds all files matching the pattern, optionally filters by content, \
         and replaces old_string with new_string in each file. Returns a summary \
         of changes made."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match files (e.g., '**/*.rs', 'src/**/*.ts')"
                },
                "old_string": {
                    "type": "string",
                    "description": "Text to find in each file"
                },
                "new_string": {
                    "type": "string",
                    "description": "Replacement text"
                },
                "filter_contains": {
                    "type": "string",
                    "description": "Optional: only edit files that contain this string (filters before edit)"
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "If true, show what would be changed without making edits (default: false)"
                },
                "max_files": {
                    "type": "integer",
                    "description": "Maximum number of files to edit (default: 100)"
                }
            },
            "required": ["pattern", "old_string", "new_string"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        // ── 1. Autonomy check ──────────────────────────────────────
        if !self.security.can_act() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Action blocked: autonomy is read-only".into()),
            });
        }

        // ── 2. Rate limit check ────────────────────────────────────
        if self.security.is_rate_limited() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: too many actions in the last hour".into()),
            });
        }

        // Check dry-run mode from CLI
        let cli_dry_run = std::env::var("SEN_DRY_RUN").as_deref() == Ok("1");
        let dry_run = args
            .get("dry_run")
            .and_then(|v| v.as_bool())
            .unwrap_or(cli_dry_run);

        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'pattern' parameter"))?;

        let old_string = args
            .get("old_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'old_string' parameter"))?;

        let new_string = args
            .get("new_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'new_string' parameter"))?;

        let filter_contains = args
            .get("filter_contains")
            .and_then(|v| v.as_str());

        let max_files = args
            .get("max_files")
            .and_then(|v| v.as_i64())
            .unwrap_or(100) as usize;

        // Find matching files
        let matches = self.find_matching_files(pattern)?;
        let total_found = matches.len();

        if matches.is_empty() {
            return Ok(ToolResult {
                success: true,
                output: format!("No files found matching pattern: {}", pattern),
                error: None,
            });
        }

        // Apply optional content filter
        let files_to_edit: Vec<PathBuf> = if let Some(filter) = filter_contains {
            matches
                .into_iter()
                .filter(|path| self.file_contains(path, filter).unwrap_or(false))
                .take(max_files)
                .collect()
        } else {
            matches.into_iter().take(max_files).collect()
        };

        if files_to_edit.is_empty() {
            return Ok(ToolResult {
                success: true,
                output: format!(
                    "No files to edit after filtering. Pattern '{}' matched {} file(s).",
                    pattern,
                    total_found
                ),
                error: None,
            });
        }

        // ── 3. Record action (only for actual edits) ───────────────
        if !dry_run && !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: action budget exhausted".into()),
            });
        }

        // ── 4. Validate all resolved paths before modification ─────
        if !dry_run {
            for path in &files_to_edit {
                let resolved = match self.resolve_and_validate_path(path).await {
                    Ok(r) => r,
                    Err(e) => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!(
                                "Security validation failed for {}: {}",
                                path.display(),
                                e
                            )),
                        });
                    }
                };

                if let Err(e) = self.verify_no_symlink(&resolved).await {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Security check failed for {}: {}",
                            resolved.display(),
                            e
                        )),
                    });
                }
            }
        }

        let mut results: Vec<String> = Vec::new();
        results.push(format!("Found {} file(s) matching pattern '{}'", files_to_edit.len(), pattern));

        let mut edited_count = 0;
        let mut error_count = 0;
        let mut error_details = Vec::new();

        if dry_run {
            results.push(format!("[DRY RUN] Would edit {} file(s):", files_to_edit.len()));
            for path in &files_to_edit {
                results.push(format!("  - {}", path.display()));
            }
        } else {
            for path in &files_to_edit {
                match self.apply_edit(path, old_string, new_string).await {
                    Ok(changed) => {
                        if changed {
                            edited_count += 1;
                            results.push(format!("  ✓ Edited: {}", path.display()));
                        }
                    }
                    Err(e) => {
                        error_count += 1;
                        error_details.push(format!("  ✗ {}: {e}", path.display()));
                    }
                }
            }

            if error_count > 0 {
                results.push(format!(
                    "\n⚠️  Partial success: {} edited, {} failed",
                    edited_count, error_count
                ));
                results.push("Errors:".to_string());
                results.extend(error_details);
            } else {
                results.push(format!("\nSuccessfully edited {} file(s)", edited_count));
            }
        }

        let overall_success = error_count == 0;

        Ok(ToolResult {
            success: overall_success,
            output: results.join("\n"),
            error: if error_count > 0 {
                Some(format!(
                    "Failed to edit {} of {} file(s)",
                    error_count,
                    files_to_edit.len()
                ))
            } else {
                None
            },
        })
    }
}

impl GlobEditTool {
    const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10 MB

    async fn apply_edit(
        &self,
        path: &Path,
        old_string: &str,
        new_string: &str,
    ) -> anyhow::Result<bool> {
        // Resolve the path first to ensure workspace confinement
        let resolved = self.resolve_and_validate_path(path).await?;

        // File size guard
        if let Ok(meta) = tokio::fs::metadata(&resolved).await {
            if meta.len() > Self::MAX_FILE_SIZE {
                anyhow::bail!(
                    "File too large ({:.1} MB). Maximum supported size is 10 MB.",
                    meta.len() as f64 / (1024.0 * 1024.0)
                );
            }
        }

        let content = tokio::fs::read_to_string(&resolved).await?;

        if !content.contains(old_string) {
            return Ok(false);
        }

        let new_content = content.replace(old_string, new_string);
        tokio::fs::write(&resolved, new_content).await?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_env() -> (TempDir, PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let workspace = temp_dir.path().to_path_buf();

        // Create test files
        std::fs::write(workspace.join("file1.txt"), "Hello World").unwrap();
        std::fs::write(workspace.join("file2.txt"), "Hello Rust").unwrap();
        std::fs::write(workspace.join("file3.txt"), "Goodbye World").unwrap();

        (temp_dir, workspace)
    }

    #[test]
    fn test_find_matching_files() {
        let (_temp, workspace) = create_test_env();
        let security = SecurityPolicy::allow_all();
        let tool = GlobEditTool::new(Arc::new(security), workspace);

        let matches = tool.find_matching_files("*.txt").unwrap();
        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn test_glob_edit_blocks_path_traversal() {
        let (_temp, workspace) = create_test_env();
        let security = SecurityPolicy::default(); // Uses workspace confinement
        let tool = GlobEditTool::new(Arc::new(security), workspace.clone());

        // Attempt to escape workspace should fail
        let result = tool.find_matching_files("../outside.txt");
        assert!(result.is_err(), "Path traversal should be blocked by security policy");
    }

    #[tokio::test]
    async fn test_apply_edit_with_security() {
        let (_temp, workspace) = create_test_env();
        let security = SecurityPolicy::allow_all();
        let tool = GlobEditTool::new(Arc::new(security), workspace.clone());

        let path = workspace.join("file1.txt");
        let changed = tool.apply_edit(&path, "World", "Universe").await.unwrap();
        assert!(changed);

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "Hello Universe");
    }

    #[tokio::test]
    async fn test_apply_edit_no_match() {
        let (_temp, workspace) = create_test_env();
        let workspace_for_tool = workspace.clone();
        let security = SecurityPolicy::allow_all();
        let tool = GlobEditTool::new(Arc::new(security), workspace_for_tool);

        let path = workspace.join("file1.txt");
        let changed = tool.apply_edit(&path, "NotFound", "Replacement").await.unwrap();
        assert!(!changed);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_glob_edit_blocks_symlink_escape() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::TempDir::new().unwrap();
        let workspace = temp.path().to_path_buf();
        let outside = workspace.join("outside");

        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("target.txt"), "sensitive").unwrap();

        let security = SecurityPolicy::default();
        let tool = GlobEditTool::new(Arc::new(security), workspace.clone());

        // Create a symlink inside workspace pointing outside
        let link_path = workspace.join("escape_link");
        symlink(&outside, &link_path).unwrap();

        // Pre-flight validation should detect symlink escape
        let result = tool.verify_no_symlink(&link_path).await;
        assert!(result.is_err(), "Symlink to outside should be blocked");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("symlink") || err.to_string().contains("escapes"),
            "Error should mention symlink or escape"
        );
    }
}
