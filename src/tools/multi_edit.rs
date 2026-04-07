// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

/// Apply edits to multiple files atomically. If any edit fails validation,
/// no files are modified. This enables safe cross-file refactoring.
pub struct MultiEditTool {
    security: Arc<SecurityPolicy>,
}

impl MultiEditTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for MultiEditTool {
    fn name(&self) -> &str {
        "multi_edit"
    }

    fn description(&self) -> &str {
        "Apply edits to multiple files atomically. All edits succeed or none are applied. \
         Each edit specifies a file path and either old_string/new_string replacement \
         or full content to write."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "edits": {
                    "type": "array",
                    "description": "Array of file edits to apply atomically",
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Path to the file to edit"
                            },
                            "old_string": {
                                "type": "string",
                                "description": "Text to find and replace (if omitted, writes full content)"
                            },
                            "new_string": {
                                "type": "string",
                                "description": "Replacement text (or full file content if old_string is omitted)"
                            }
                        },
                        "required": ["path", "new_string"]
                    }
                }
            },
            "required": ["edits"]
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

        if self.security.is_rate_limited() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: too many actions in the last hour".into()),
            });
        }

        let edits = args
            .get("edits")
            .and_then(|e| e.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing 'edits' array"))?;

        if edits.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("No edits provided".into()),
            });
        }

        // Phase 1: Validate all edits and prepare changes
        let mut prepared: Vec<(std::path::PathBuf, String, Option<u64>)> = Vec::new();
        let mut backup: Vec<(std::path::PathBuf, Option<String>)> = Vec::new();

        for (i, edit) in edits.iter().enumerate() {
            let path_str = edit
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Edit {i}: missing 'path'"))?;
            let new_string = edit
                .get("new_string")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Edit {i}: missing 'new_string'"))?;
            let old_string = edit.get("old_string").and_then(|v| v.as_str());
            let expected_mtime_ms = edit
                .get("expected_mtime_ms")
                .and_then(|v| v.as_i64())
                .map(|v| v as u64);

            if !self.security.is_path_allowed(path_str) {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Edit {i}: security policy blocked path '{}'",
                        path_str
                    )),
                });
            }

            let path = std::path::PathBuf::from(path_str);
            let new_content = if let Some(old) = old_string {
                let content = match tokio::fs::read_to_string(&path).await {
                    Ok(c) => c,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!(
                                "Edit {i}: file '{}' does not exist",
                                path.display()
                            )),
                        });
                    }
                    Ok(c) => c,
                    Err(e) => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!(
                                "Edit {i}: cannot read '{}': {e}",
                                path.display()
                            )),
                        });
                    }
                };
                let count = content.matches(old).count();
                if count == 0 {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Edit {i}: old_string not found in '{}'",
                            path.display()
                        )),
                    });
                }
                if count > 1 {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Edit {i}: old_string matches {count} times in '{}' (use exact string to disambiguate)",
                            path.display()
                        )),
                    });
                }
                content.replacen(old, new_string, 1)
            } else {
                new_string.to_string()
            };

            // Capture mtime for optimistic concurrency check before writing
            let actual_mtime = tokio::fs::metadata(&path)
                .await
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64);

            if let Some(expected) = expected_mtime_ms {
                if let Some(actual) = actual_mtime {
                    if actual != expected {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!(
                                "Edit {i}: file '{}' was modified externally (expected mtime {}, found {}). \
                                 Re-read the file and retry with the updated content.",
                                path.display(),
                                expected,
                                actual
                            )),
                        });
                    }
                }
            }

            // Refuse to follow symlinks
            if tokio::fs::symlink_metadata(&path).await.map_or(false, |m| m.file_type().is_symlink()) {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Edit {i}: refusing to edit through symlink '{}'",
                        path.display()
                    )),
                });
            }

            let existing = tokio::fs::read_to_string(&path).await.ok();
            backup.push((path.clone(), existing));
            prepared.push((path, new_content, actual_mtime));
        }

        // Phase 2: Apply all edits atomically
        let mut applied = Vec::new();
        for (path, content, expected_mtime) in &prepared {
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            // Re-check mtime before write to detect concurrent modification (TOCTOU mitigation)
            if let Some(expected) = expected_mtime {
                if let Ok(current_meta) = tokio::fs::metadata(path).await {
                    if let Ok(current_mtime) = current_meta.modified() {
                        if let Ok(current) = current_mtime.duration_since(std::time::UNIX_EPOCH) {
                            if current.as_millis() as u64 != *expected {
                                // Rollback already-applied edits
                                for (bp, original) in &backup {
                                    if applied.contains(bp) {
                                        if let Some(orig) = original {
                                            let _ = tokio::fs::write(bp, orig).await;
                                        } else {
                                            let _ = tokio::fs::remove_file(bp).await;
                                        }
                                    }
                                }
                                return Ok(ToolResult {
                                    success: false,
                                    output: String::new(),
                                    error: Some(format!(
                                        "File '{}' was modified between validation and write (TOCTOU race detected). \
                                         All edits rolled back. Re-read files and retry.",
                                        path.display()
                                    )),
                                });
                            }
                        }
                    }
                }
            }
            match tokio::fs::write(path, content).await {
                Ok(()) => applied.push(path.clone()),
                Err(e) => {
                    // Rollback all applied edits
                    for (bp, original) in &backup {
                        if applied.contains(bp) {
                            if let Some(orig) = original {
                                let _ = tokio::fs::write(bp, orig).await;
                            } else {
                                let _ = tokio::fs::remove_file(bp).await;
                            }
                        }
                    }
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Write failed for '{}': {e}. All edits rolled back.",
                            path.display()
                        )),
                    });
                }
            }
        }

        let summary: Vec<String> = prepared
            .iter()
            .map(|(p, _, _)| format!("  \u{2713} {}", p.display()))
            .collect();

        Ok(ToolResult {
            success: true,
            output: format!(
                "Applied {} edit(s) atomically:\n{}",
                prepared.len(),
                summary.join("\n")
            ),
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_name() {
        let security = Arc::new(SecurityPolicy::default());
        let tool = MultiEditTool::new(security);
        assert_eq!(tool.name(), "multi_edit");
    }

    #[test]
    fn schema_has_edits() {
        let security = Arc::new(SecurityPolicy::default());
        let tool = MultiEditTool::new(security);
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["edits"].is_object());
    }
}
