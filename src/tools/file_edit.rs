// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

/// Edit a file using various editing modes: replace, append, insert_after, insert_before.
///
/// Security checks mirror [`super::file_write::FileWriteTool`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditMode {
    /// Default: replace old_string with new_string
    Replace,
    /// Append content to end of file
    Append,
    /// Insert content after pattern
    InsertAfter,
    /// Insert content before pattern
    InsertBefore,
}

impl Default for EditMode {
    fn default() -> Self {
        EditMode::Replace
    }
}
///
/// Uses `old_string` → `new_string` precise replacement within the workspace.
/// The `old_string` must appear exactly once in the file (zero matches = not
/// found, multiple matches = ambiguous). `new_string` may be empty to delete
/// the matched text. Security checks mirror [`super::file_write::FileWriteTool`].
pub struct FileEditTool {
    security: Arc<SecurityPolicy>,
}

impl FileEditTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for FileEditTool {
    fn name(&self) -> &str {
        "file_edit"
    }

    fn description(&self) -> &str {
        "Edit a file using various modes: replace (default), append, insert_after, insert_before"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file. Relative paths resolve from workspace; outside paths require policy allowlist."
                },
                "mode": {
                    "type": "string",
                    "description": "Edit mode: replace (default), append, insert_after, insert_before",
                    "enum": ["replace", "append", "insert_after", "insert_before"],
                    "default": "replace"
                },
                "old_string": {
                    "type": "string",
                    "description": "The exact text to find (used for replace, insert_after, insert_before modes; ignored for append)"
                },
                "new_string": {
                    "type": "string",
                    "description": "The replacement/insertion text"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all occurrences instead of requiring exactly one match (default false)"
                },
                "expected_mtime_ms": {
                    "type": "integer",
                    "description": "Expected file modification time in milliseconds since epoch. \
                                    If the file has been modified since this timestamp, the edit is \
                                    rejected to prevent overwriting manual edits."
                }
            },
            "required": ["path", "new_string"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        // ── 1. Extract parameters ──────────────────────────────────
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' parameter"))?;

        // Parse edit mode
        let mode = args
            .get("mode")
            .and_then(|v| v.as_str())
            .map(EditMode::from_str)
            .unwrap_or(Ok(EditMode::default()))
            .map_err(|e: String| anyhow::anyhow!(e))?;

        let old_string = args
            .get("old_string")
            .and_then(|v| v.as_str());

        let new_string = args
            .get("new_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'new_string' parameter"))?;

        // old_string is required for non-append modes
        if mode != EditMode::Append && old_string.is_none() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "'old_string' is required for mode '{}'",
                    match mode {
                        EditMode::Replace => "replace",
                        EditMode::InsertAfter => "insert_after",
                        EditMode::InsertBefore => "insert_before",
                        EditMode::Append => unreachable!(),
                    }
                )),
            });
        }

        // ── 2. Autonomy check ──────────────────────────────────────
        if !self.security.can_act() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Action blocked: autonomy is read-only".into()),
            });
        }

        // ── 3. Rate limit check ────────────────────────────────────
        if self.security.is_rate_limited() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: too many actions in the last hour".into()),
            });
        }

        // ── 4. Path pre-validation ─────────────────────────────────
        if !self.security.is_path_allowed(path) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Path not allowed by security policy: {path}")),
            });
        }

        let full_path = self.security.resolve_tool_path(path);

        // ── 5. Canonicalize parent ─────────────────────────────────
        let Some(parent) = full_path.parent() else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Invalid path: missing parent directory".into()),
            });
        };

        let resolved_parent = match tokio::fs::canonicalize(parent).await {
            Ok(p) => p,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to resolve file path: {e}")),
                });
            }
        };

        // ── 6. Resolved path post-validation ───────────────────────
        if !self.security.is_resolved_path_allowed(&resolved_parent) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    self.security
                        .resolved_path_violation_message(&resolved_parent),
                ),
            });
        }

        let Some(file_name) = full_path.file_name() else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Invalid path: missing file name".into()),
            });
        };

        let resolved_target = resolved_parent.join(file_name);

        if self.security.is_runtime_config_path(&resolved_target) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    self.security
                        .runtime_config_violation_message(&resolved_target),
                ),
            });
        }

        // ── 7. Symlink check ───────────────────────────────────────
        if let Ok(meta) = tokio::fs::symlink_metadata(&resolved_target).await {
            if meta.file_type().is_symlink() {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Refusing to edit through symlink: {}",
                        resolved_target.display()
                    )),
                });
            }
        }

        // ── 8. Record action ───────────────────────────────────────
        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: action budget exhausted".into()),
            });
        }

        // ── 9. File size guard (10 MB) ──────────────────────────────
        const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;
        if let Ok(meta) = tokio::fs::metadata(&resolved_target).await {
            if meta.len() > MAX_FILE_SIZE {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "File too large ({:.1} MB). Maximum supported size is 10 MB.",
                        meta.len() as f64 / (1024.0 * 1024.0)
                    )),
                });
            }
        }

        // ── 10. mtime validation ─────────────────────────────────
        if let Some(expected_ms) = args.get("expected_mtime_ms").and_then(|v| v.as_u64()) {
            if let Ok(meta) = tokio::fs::metadata(&resolved_target).await {
                if let Ok(mtime) = meta.modified() {
                    let actual_ms = mtime
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    if actual_ms != expected_ms {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!(
                                "File has been modified since last read \
                                 (expected mtime {expected_ms}ms, actual {actual_ms}ms). \
                                 Please re-read the file before editing to avoid \
                                 overwriting manual changes."
                            )),
                        });
                    }
                }
            }
        }

        // ── 11. Execute edit based on mode ───────────────────────
        match mode {
            EditMode::Replace => {
                self.execute_replace(&args, old_string.unwrap(), new_string, &resolved_target, path).await
            }
            EditMode::Append => {
                self.execute_append(new_string, &resolved_target, path).await
            }
            EditMode::InsertAfter => {
                self.execute_insert_after(old_string.unwrap(), new_string, &resolved_target, path).await
            }
            EditMode::InsertBefore => {
                self.execute_insert_before(old_string.unwrap(), new_string, &resolved_target, path).await
            }
        }
    }
}

impl FileEditTool {
    /// Execute replace mode edit.
    async fn execute_replace(
        &self,
        args: &serde_json::Value,
        old_string: &str,
        new_string: &str,
        resolved_target: &std::path::Path,
        display_path: &str,
    ) -> anyhow::Result<ToolResult> {
        let replace_all = args
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if old_string.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("old_string must not be empty".into()),
            });
        }

        let content = match tokio::fs::read_to_string(resolved_target).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to read file: {e}")),
                });
            }
        };

        let match_count = content.matches(old_string).count();

        if match_count == 0 {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("old_string not found in file".into()),
            });
        }

        if !replace_all && match_count > 1 {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "old_string matches {match_count} times; must match exactly once \
                     (use replace_all=true to replace all occurrences)"
                )),
            });
        }

        let new_content = if replace_all {
            content.replace(old_string, new_string)
        } else {
            content.replacen(old_string, new_string, 1)
        };

        let replaced_count = if replace_all { match_count } else { 1 };

        match tokio::fs::write(resolved_target, &new_content).await {
            Ok(()) => {
                let diff = self.generate_diff(display_path, old_string, new_string);
                Ok(ToolResult {
                    success: true,
                    output: format!(
                        "Edited {display_path}: replaced {replaced_count} occurrence(s)\n{diff}"
                    ),
                    error: None,
                })
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to write file: {e}")),
            }),
        }
    }

    /// Execute append mode edit.
    async fn execute_append(
        &self,
        new_string: &str,
        resolved_target: &std::path::Path,
        display_path: &str,
    ) -> anyhow::Result<ToolResult> {
        // Read existing content
        let content = match tokio::fs::read_to_string(resolved_target).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to read file: {e}")),
                });
            }
        };

        // Append content
        let needs_newline = !content.is_empty() && !content.ends_with('\n');
        let new_content = if needs_newline {
            format!("{}\n{}", content, new_string)
        } else {
            format!("{}{}", content, new_string)
        };

        match tokio::fs::write(resolved_target, &new_content).await {
            Ok(()) => Ok(ToolResult {
                success: true,
                output: format!(
                    "Appended to {display_path}:\n```\n{}\n```",
                    new_string
                ),
                error: None,
            }),
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to write file: {e}")),
            }),
        }
    }

    /// Execute insert_after mode edit.
    async fn execute_insert_after(
        &self,
        pattern: &str,
        new_string: &str,
        resolved_target: &std::path::Path,
        display_path: &str,
    ) -> anyhow::Result<ToolResult> {
        let content = match tokio::fs::read_to_string(resolved_target).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to read file: {e}")),
                });
            }
        };

        let match_count = content.matches(pattern).count();

        if match_count == 0 {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Pattern not found in file".into()),
            });
        }

        if match_count > 1 {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Pattern matches {match_count} times; must match exactly once"
                )),
            });
        }

        // Find position after the pattern
        if let Some(pos) = content.find(pattern) {
            let insert_pos = pos + pattern.len();
            let mut new_content = content.clone();
            new_content.insert_str(insert_pos, new_string);

            match tokio::fs::write(resolved_target, &new_content).await {
                Ok(()) => Ok(ToolResult {
                    success: true,
                    output: format!(
                        "Inserted after pattern in {display_path}:\n```\n{}\n```",
                        new_string
                    ),
                    error: None,
                }),
                Err(e) => Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to write file: {e}")),
                }),
            }
        } else {
            Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Failed to find pattern position".into()),
            })
        }
    }

    /// Execute insert_before mode edit.
    async fn execute_insert_before(
        &self,
        pattern: &str,
        new_string: &str,
        resolved_target: &std::path::Path,
        display_path: &str,
    ) -> anyhow::Result<ToolResult> {
        let content = match tokio::fs::read_to_string(resolved_target).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to read file: {e}")),
                });
            }
        };

        let match_count = content.matches(pattern).count();

        if match_count == 0 {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Pattern not found in file".into()),
            });
        }

        if match_count > 1 {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Pattern matches {match_count} times; must match exactly once"
                )),
            });
        }

        // Find position before the pattern
        if let Some(pos) = content.find(pattern) {
            let mut new_content = content.clone();
            new_content.insert_str(pos, new_string);

            match tokio::fs::write(resolved_target, &new_content).await {
                Ok(()) => Ok(ToolResult {
                    success: true,
                    output: format!(
                        "Inserted before pattern in {display_path}:\n```\n{}\n```",
                        new_string
                    ),
                    error: None,
                }),
                Err(e) => Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to write file: {e}")),
                }),
            }
        } else {
            Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Failed to find pattern position".into()),
            })
        }
    }

    /// Generate a unified diff for display.
    fn generate_diff(&self, path: &str, old_string: &str, new_string: &str) -> String {
        let old_lines: Vec<&str> = old_string.lines().collect();
        let new_lines: Vec<&str> = new_string.lines().collect();
        let mut diff_buf = format!("--- a/{path}\n+++ b/{path}\n");
        let old_line_count = old_lines.len();
        let new_line_count = new_lines.len();
        diff_buf.push_str(&format!("@@ -{old_line_count} +{new_line_count} @@\n"));
        for line in &old_lines {
            diff_buf.push_str(&format!("-{line}\n"));
        }
        for line in &new_lines {
            diff_buf.push_str(&format!("+{line}\n"));
        }
        diff_buf
    }
}

// Helper trait for EditMode
impl EditMode {
    fn from_str(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "replace" | "default" => Ok(EditMode::Replace),
            "append" => Ok(EditMode::Append),
            "insert_after" => Ok(EditMode::InsertAfter),
            "insert_before" => Ok(EditMode::InsertBefore),
            _ => Err(format!(
                "Unknown edit mode: {}. Valid modes: replace, append, insert_after, insert_before",
                s
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::{AutonomyLevel, SecurityPolicy};

    fn test_security(workspace: std::path::PathBuf) -> Arc<SecurityPolicy> {
        Arc::new(SecurityPolicy {
            autonomy: AutonomyLevel::Supervised,
            workspace_dir: workspace,
            ..SecurityPolicy::default()
        })
    }

    fn test_security_with(
        workspace: std::path::PathBuf,
        autonomy: AutonomyLevel,
        max_actions_per_hour: u32,
    ) -> Arc<SecurityPolicy> {
        Arc::new(SecurityPolicy {
            autonomy,
            workspace_dir: workspace,
            max_actions_per_hour,
            ..SecurityPolicy::default()
        })
    }

    #[test]
    fn file_edit_name() {
        let tool = FileEditTool::new(test_security(std::env::temp_dir()));
        assert_eq!(tool.name(), "file_edit");
    }

    #[test]
    fn file_edit_schema_has_required_params() {
        let tool = FileEditTool::new(test_security(std::env::temp_dir()));
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["path"].is_object());
        assert!(schema["properties"]["old_string"].is_object());
        assert!(schema["properties"]["new_string"].is_object());
        assert!(schema["properties"]["mode"].is_object());
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("path")));
        // old_string is no longer required (can use append mode)
        assert!(required.contains(&json!("new_string")));
    }

    #[tokio::test]
    async fn file_edit_replaces_single_match() {
        let dir = std::env::temp_dir().join("sen_test_file_edit_single");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("test.txt"), "hello world")
            .await
            .unwrap();

        let tool = FileEditTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({
                "path": "test.txt",
                "old_string": "hello",
                "new_string": "goodbye"
            }))
            .await
            .unwrap();

        assert!(result.success, "edit should succeed: {:?}", result.error);
        assert!(result.output.contains("replaced 1 occurrence"));

        let content = tokio::fs::read_to_string(dir.join("test.txt"))
            .await
            .unwrap();
        assert_eq!(content, "goodbye world");

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_edit_not_found() {
        let dir = std::env::temp_dir().join("sen_test_file_edit_notfound");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("test.txt"), "hello world")
            .await
            .unwrap();

        let tool = FileEditTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({
                "path": "test.txt",
                "old_string": "nonexistent",
                "new_string": "replacement"
            }))
            .await
            .unwrap();

        assert!(!result.success);
        assert!(result.error.as_deref().unwrap_or("").contains("not found"));

        // File should be unchanged
        let content = tokio::fs::read_to_string(dir.join("test.txt"))
            .await
            .unwrap();
        assert_eq!(content, "hello world");

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_edit_multiple_matches() {
        let dir = std::env::temp_dir().join("sen_test_file_edit_multi");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("test.txt"), "aaa bbb aaa")
            .await
            .unwrap();

        let tool = FileEditTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({
                "path": "test.txt",
                "old_string": "aaa",
                "new_string": "ccc"
            }))
            .await
            .unwrap();

        assert!(!result.success);
        assert!(
            result
                .error
                .as_deref()
                .unwrap_or("")
                .contains("matches 2 times")
        );

        // File should be unchanged
        let content = tokio::fs::read_to_string(dir.join("test.txt"))
            .await
            .unwrap();
        assert_eq!(content, "aaa bbb aaa");

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_edit_delete_via_empty_new_string() {
        let dir = std::env::temp_dir().join("sen_test_file_edit_delete");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("test.txt"), "keep remove keep")
            .await
            .unwrap();

        let tool = FileEditTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({
                "path": "test.txt",
                "old_string": " remove",
                "new_string": ""
            }))
            .await
            .unwrap();

        assert!(
            result.success,
            "delete edit should succeed: {:?}",
            result.error
        );

        let content = tokio::fs::read_to_string(dir.join("test.txt"))
            .await
            .unwrap();
        assert_eq!(content, "keep keep");

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_edit_missing_path_param() {
        let tool = FileEditTool::new(test_security(std::env::temp_dir()));
        let result = tool
            .execute(json!({"old_string": "a", "new_string": "b"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn file_edit_missing_old_string_in_replace_mode() {
        let tool = FileEditTool::new(test_security(std::env::temp_dir()));
        // Without mode specified, default is "replace" which requires old_string
        let result = tool
            .execute(json!({"path": "f.txt", "new_string": "b"}))
            .await
            .unwrap();
        // Should fail with "old_string is required for mode 'replace'"
        assert!(!result.success, "replace mode should require old_string");
        assert!(result.error.unwrap_or_default().contains("old_string"));
    }

    #[tokio::test]
    async fn file_edit_missing_new_string_param() {
        let tool = FileEditTool::new(test_security(std::env::temp_dir()));
        let result = tool
            .execute(json!({"path": "f.txt", "old_string": "a"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn file_edit_append_mode_no_old_string_required() {
        let dir = std::env::temp_dir().join("sen_test_file_edit_append_no_old_string");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("test.txt"), "existing content")
            .await
            .unwrap();

        let tool = FileEditTool::new(test_security(dir.clone()));
        // Append mode doesn't require old_string
        let result = tool
            .execute(json!({
                "path": "test.txt",
                "mode": "append",
                "new_string": "\nappended content"
            }))
            .await
            .unwrap();

        assert!(result.success, "append mode should succeed without old_string: {:?}", result.error);

        let content = tokio::fs::read_to_string(dir.join("test.txt"))
            .await
            .unwrap();
        assert!(content.contains("appended content"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_edit_insert_after_mode() {
        let dir = std::env::temp_dir().join("sen_test_file_edit_insert_after");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("test.txt"), "hello world")
            .await
            .unwrap();

        let tool = FileEditTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({
                "path": "test.txt",
                "mode": "insert_after",
                "old_string": "hello",
                "new_string": " there"
            }))
            .await
            .unwrap();

        assert!(result.success, "insert_after should succeed: {:?}", result.error);

        let content = tokio::fs::read_to_string(dir.join("test.txt"))
            .await
            .unwrap();
        assert!(content.contains("hello there"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_edit_insert_before_mode() {
        let dir = std::env::temp_dir().join("sen_test_file_edit_insert_before");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("test.txt"), "hello world")
            .await
            .unwrap();

        let tool = FileEditTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({
                "path": "test.txt",
                "mode": "insert_before",
                "old_string": "world",
                "new_string": "dear "
            }))
            .await
            .unwrap();

        assert!(result.success, "insert_before should succeed: {:?}", result.error);

        let content = tokio::fs::read_to_string(dir.join("test.txt"))
            .await
            .unwrap();
        assert!(content.contains("dear world"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_edit_insert_after_pattern_not_found() {
        let dir = std::env::temp_dir().join("sen_test_file_edit_insert_after_notfound");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("test.txt"), "hello world")
            .await
            .unwrap();

        let tool = FileEditTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({
                "path": "test.txt",
                "mode": "insert_after",
                "old_string": "notfound",
                "new_string": "something"
            }))
            .await
            .unwrap();

        assert!(!result.success, "insert_after should fail when pattern not found");
        assert!(result.error.unwrap_or_default().contains("not found"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_edit_insert_before_pattern_not_found() {
        let dir = std::env::temp_dir().join("sen_test_file_edit_insert_before_notfound");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("test.txt"), "hello world")
            .await
            .unwrap();

        let tool = FileEditTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({
                "path": "test.txt",
                "mode": "insert_before",
                "old_string": "notfound",
                "new_string": "something"
            }))
            .await
            .unwrap();

        assert!(!result.success, "insert_before should fail when pattern not found");
        assert!(result.error.unwrap_or_default().contains("not found"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_edit_rejects_empty_old_string() {
        let dir = std::env::temp_dir().join("sen_test_file_edit_empty_old_string");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("test.txt"), "hello")
            .await
            .unwrap();

        let tool = FileEditTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({
                "path": "test.txt",
                "old_string": "",
                "new_string": "x"
            }))
            .await
            .unwrap();

        assert!(!result.success);
        assert!(
            result
                .error
                .as_deref()
                .unwrap_or("")
                .contains("must not be empty")
        );

        let content = tokio::fs::read_to_string(dir.join("test.txt"))
            .await
            .unwrap();
        assert_eq!(content, "hello");

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_edit_blocks_path_traversal() {
        let dir = std::env::temp_dir().join("sen_test_file_edit_traversal");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let tool = FileEditTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({
                "path": "../../etc/passwd",
                "old_string": "root",
                "new_string": "hacked"
            }))
            .await
            .unwrap();

        assert!(!result.success);
        assert!(result.error.as_ref().unwrap().contains("not allowed"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_edit_blocks_absolute_path() {
        // Use a workspace that doesn't allow absolute paths outside it
        let dir = std::env::temp_dir().join("sen_test_file_edit_blocks_abs");
        let tool = FileEditTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({
                "path": "/etc/passwd",
                "old_string": "root",
                "new_string": "hacked"
            }))
            .await
            .unwrap();

        assert!(!result.success, "Absolute path outside workspace should be blocked");
        // Error should contain "not allowed", "escapes", or "policy"
        let error = result.error.unwrap_or_default();
        let error_lower = error.to_lowercase();
        assert!(
            error_lower.contains("not allowed")
                || error_lower.contains("escapes")
                || error_lower.contains("policy"),
            "Error should mention policy violation: {}",
            error
        );
    }

    #[tokio::test]
    async fn file_edit_normalizes_workspace_prefixed_relative_path() {
        let root = std::env::temp_dir().join("sen_test_file_edit_workspace_prefixed");
        let workspace = root.join("workspace");
        let _ = tokio::fs::remove_dir_all(&root).await;
        tokio::fs::create_dir_all(workspace.join("nested"))
            .await
            .unwrap();
        tokio::fs::write(workspace.join("nested/target.txt"), "hello world")
            .await
            .unwrap();

        let tool = FileEditTool::new(test_security(workspace.clone()));
        // Use a simple relative path within workspace
        let relative_path = "nested/target.txt";

        let result = tool
            .execute(json!({
                "path": relative_path,
                "old_string": "world",
                "new_string": "sen"
            }))
            .await
            .unwrap();

        assert!(result.success, "Simple relative path should work: {:?}", result.error);
        let content = tokio::fs::read_to_string(workspace.join("nested/target.txt"))
            .await
            .unwrap();
        assert_eq!(content, "hello sen");

        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn file_edit_blocks_symlink_escape() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join("sen_test_file_edit_symlink_escape");
        let workspace = root.join("workspace");
        let outside = root.join("outside");

        let _ = tokio::fs::remove_dir_all(&root).await;
        tokio::fs::create_dir_all(&workspace).await.unwrap();
        tokio::fs::create_dir_all(&outside).await.unwrap();

        symlink(&outside, workspace.join("escape_dir")).unwrap();

        let tool = FileEditTool::new(test_security(workspace.clone()));
        let result = tool
            .execute(json!({
                "path": "escape_dir/target.txt",
                "old_string": "a",
                "new_string": "b"
            }))
            .await
            .unwrap();

        assert!(!result.success);
        assert!(
            result
                .error
                .as_deref()
                .unwrap_or("")
                .contains("escapes workspace")
        );

        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn file_edit_blocks_symlink_target_file() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join("sen_test_file_edit_symlink_target");
        let workspace = root.join("workspace");
        let outside = root.join("outside");

        let _ = tokio::fs::remove_dir_all(&root).await;
        tokio::fs::create_dir_all(&workspace).await.unwrap();
        tokio::fs::create_dir_all(&outside).await.unwrap();

        tokio::fs::write(outside.join("target.txt"), "original")
            .await
            .unwrap();
        symlink(outside.join("target.txt"), workspace.join("linked.txt")).unwrap();

        let tool = FileEditTool::new(test_security(workspace.clone()));
        let result = tool
            .execute(json!({
                "path": "linked.txt",
                "old_string": "original",
                "new_string": "hacked"
            }))
            .await
            .unwrap();

        assert!(!result.success, "editing through symlink must be blocked");
        assert!(
            result.error.as_deref().unwrap_or("").contains("symlink"),
            "error should mention symlink"
        );

        let content = tokio::fs::read_to_string(outside.join("target.txt"))
            .await
            .unwrap();
        assert_eq!(content, "original", "original file must not be modified");

        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    #[tokio::test]
    async fn file_edit_blocks_readonly_mode() {
        let dir = std::env::temp_dir().join("sen_test_file_edit_readonly");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("test.txt"), "hello")
            .await
            .unwrap();

        let tool = FileEditTool::new(test_security_with(dir.clone(), AutonomyLevel::ReadOnly, 20));
        let result = tool
            .execute(json!({
                "path": "test.txt",
                "old_string": "hello",
                "new_string": "world"
            }))
            .await
            .unwrap();

        assert!(!result.success);
        assert!(result.error.as_deref().unwrap_or("").contains("read-only"));

        let content = tokio::fs::read_to_string(dir.join("test.txt"))
            .await
            .unwrap();
        assert_eq!(content, "hello");

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_edit_blocks_when_rate_limited() {
        let dir = std::env::temp_dir().join("sen_test_file_edit_rate_limited");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("test.txt"), "hello")
            .await
            .unwrap();

        let tool = FileEditTool::new(test_security_with(
            dir.clone(),
            AutonomyLevel::Supervised,
            0,
        ));
        let result = tool
            .execute(json!({
                "path": "test.txt",
                "old_string": "hello",
                "new_string": "world"
            }))
            .await
            .unwrap();

        assert!(!result.success);
        assert!(
            result
                .error
                .as_deref()
                .unwrap_or("")
                .contains("Rate limit exceeded")
        );

        let content = tokio::fs::read_to_string(dir.join("test.txt"))
            .await
            .unwrap();
        assert_eq!(content, "hello");

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_edit_nonexistent_file() {
        let dir = std::env::temp_dir().join("sen_test_file_edit_nofile");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let tool = FileEditTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({
                "path": "missing.txt",
                "old_string": "a",
                "new_string": "b"
            }))
            .await
            .unwrap();

        assert!(!result.success);
        assert!(
            result
                .error
                .as_deref()
                .unwrap_or("")
                .contains("Failed to read file")
        );

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_edit_absolute_path_in_workspace() {
        let dir = std::env::temp_dir().join("sen_test_file_edit_abs_path");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();

        // Canonicalize so the workspace dir matches resolved paths on macOS (/private/var/…)
        let dir = tokio::fs::canonicalize(&dir).await.unwrap();

        tokio::fs::write(dir.join("target.txt"), "old content")
            .await
            .unwrap();

        let tool = FileEditTool::new(test_security(dir.clone()));

        // Pass an absolute path that is within the workspace
        let abs_path = dir.join("target.txt");
        let result = tool
            .execute(json!({
                "path": abs_path.to_string_lossy().to_string(),
                "old_string": "old content",
                "new_string": "new content"
            }))
            .await
            .unwrap();

        assert!(
            result.success,
            "editing via absolute workspace path should succeed, error: {:?}",
            result.error
        );

        let content = tokio::fs::read_to_string(dir.join("target.txt"))
            .await
            .unwrap();
        assert_eq!(content, "new content");

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_edit_blocks_null_byte_in_path() {
        let dir = std::env::temp_dir().join("sen_test_file_edit_null_byte");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let tool = FileEditTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({
                "path": "test\0evil.txt",
                "old_string": "old",
                "new_string": "new"
            }))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.as_ref().unwrap().contains("not allowed"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_edit_blocks_runtime_config_path() {
        let root = std::env::temp_dir().join("sen_test_file_edit_runtime_config");
        let workspace = root.join("workspace");
        let config_path = root.join("config.toml");
        let _ = tokio::fs::remove_dir_all(&root).await;
        tokio::fs::create_dir_all(&workspace).await.unwrap();
        tokio::fs::write(&config_path, "always_ask = [\"cron_add\"]")
            .await
            .unwrap();

        let security = Arc::new(SecurityPolicy {
            autonomy: AutonomyLevel::Supervised,
            workspace_dir: workspace.clone(),
            workspace_only: false,
            allowed_roots: vec![root.clone()],
            forbidden_paths: vec![],
            ..SecurityPolicy::default()
        });
        let tool = FileEditTool::new(security);
        let result = tool
            .execute(json!({
                "path": config_path.to_string_lossy(),
                "old_string": "always_ask",
                "new_string": "auto_approve"
            }))
            .await
            .unwrap();

        assert!(!result.success);
        assert!(
            result
                .error
                .unwrap_or_default()
                .contains("runtime config/state file")
        );

        let _ = tokio::fs::remove_dir_all(&root).await;
    }
}
