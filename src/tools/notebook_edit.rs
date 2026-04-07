// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::Arc;

/// Edit a Jupyter notebook (.ipynb) by inserting, replacing, or deleting cells.
///
/// Security checks mirror [`super::file_edit::FileEditTool`].
pub struct NotebookEditTool {
    security: Arc<SecurityPolicy>,
}

impl NotebookEditTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditMode {
    Replace,
    Insert,
    Delete,
}

impl EditMode {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "replace" => Some(Self::Replace),
            "insert" => Some(Self::Insert),
            "delete" => Some(Self::Delete),
            _ => None,
        }
    }
}

fn string_to_source_array(s: &str) -> Vec<Value> {
    if s.is_empty() {
        return vec![json!("")];
    }
    s.split_inclusive('\n').map(|piece| json!(piece)).collect()
}

fn notebook_to_string_pretty_one_space(value: &Value) -> anyhow::Result<String> {
    let mut buf = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
    value.serialize(&mut ser)?;
    Ok(String::from_utf8(buf)?)
}

fn apply_cell_type(cell: &mut Value, cell_type: &str) {
    cell["cell_type"] = json!(cell_type);
    match cell_type {
        "code" => {
            if cell.get("metadata").is_none() {
                cell["metadata"] = json!({});
            }
            cell["execution_count"] = Value::Null;
            cell["outputs"] = json!([]);
        }
        "markdown" | "raw" => {
            if let Some(m) = cell.as_object_mut() {
                m.remove("outputs");
                m.remove("execution_count");
            }
            if cell.get("metadata").is_none() {
                cell["metadata"] = json!({});
            }
        }
        _ => {}
    }
}

fn reset_code_execution_state(cell: &mut Value) {
    if cell.get("cell_type").and_then(|v| v.as_str()) == Some("code") {
        cell["execution_count"] = Value::Null;
        cell["outputs"] = json!([]);
    }
}

#[async_trait]
impl Tool for NotebookEditTool {
    fn name(&self) -> &str {
        "notebook_edit"
    }

    fn description(&self) -> &str {
        "Edit a Jupyter notebook by inserting, replacing, or deleting cells"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "notebook_path": {
                    "type": "string",
                    "description": "Path to the .ipynb file. Relative paths resolve from workspace; outside paths require policy allowlist."
                },
                "cell_index": {
                    "type": "integer",
                    "description": "0-based cell index to edit (for insert: position after which to insert; use index equal to cells.len() to append at end; on an empty notebook use 0)"
                },
                "new_source": {
                    "type": "string",
                    "description": "New cell source content (required for replace and insert)"
                },
                "cell_type": {
                    "type": "string",
                    "enum": ["code", "markdown", "raw"],
                    "description": "Cell type (required for insert; optional for replace to change type)"
                },
                "edit_mode": {
                    "type": "string",
                    "enum": ["replace", "insert", "delete"],
                    "default": "replace",
                    "description": "Edit operation (default: replace)"
                }
            },
            "required": ["notebook_path", "cell_index"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        // ── 1. Extract parameters ──────────────────────────────────
        let notebook_path = args
            .get("notebook_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'notebook_path' parameter"))?;

        let cell_index = args
            .get("cell_index")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing or invalid 'cell_index' parameter"))?;
        let cell_index = usize::try_from(cell_index)
            .map_err(|_| anyhow::anyhow!("cell_index is out of range"))?;

        let edit_mode_str = args
            .get("edit_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("replace");
        let Some(edit_mode) = EditMode::parse(edit_mode_str) else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Invalid edit_mode: {edit_mode_str} (expected replace, insert, or delete)"
                )),
            });
        };

        let new_source = args.get("new_source").and_then(|v| v.as_str());
        let cell_type = args.get("cell_type").and_then(|v| v.as_str());

        if matches!(edit_mode, EditMode::Replace | EditMode::Insert) && new_source.is_none() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("new_source is required for replace and insert modes".into()),
            });
        }

        if matches!(edit_mode, EditMode::Insert) && cell_type.is_none() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("cell_type is required for insert mode".into()),
            });
        }

        if let Some(ct) = cell_type {
            if !matches!(ct, "code" | "markdown" | "raw") {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Invalid cell_type: {ct} (expected code, markdown, or raw)"
                    )),
                });
            }
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
        if !self.security.is_path_allowed(notebook_path) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Path not allowed by security policy: {notebook_path}"
                )),
            });
        }

        let full_path = self.security.resolve_tool_path(notebook_path);

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

        if !file_name
            .to_string_lossy()
            .rsplit_once('.')
            .map(|(_, ext)| ext.eq_ignore_ascii_case("ipynb"))
            .unwrap_or(false)
        {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Only .ipynb notebook files are supported".into()),
            });
        }

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

        // ── 10. Read → edit → write ────────────────────────────────
        let raw = match tokio::fs::read_to_string(&resolved_target).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to read file: {e}")),
                });
            }
        };

        let mut nb: Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Invalid JSON in notebook: {e}")),
                });
            }
        };

        let Some(cells) = nb.get_mut("cells").and_then(|c| c.as_array_mut()) else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Invalid notebook: missing top-level \"cells\" array".into()),
            });
        };

        let new_source = new_source.unwrap_or("");
        let source_arr = string_to_source_array(new_source);

        match edit_mode {
            EditMode::Replace => {
                if cell_index >= cells.len() {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "cell_index {cell_index} out of range (notebook has {} cells)",
                            cells.len()
                        )),
                    });
                }
                let cell = &mut cells[cell_index];
                cell["source"] = Value::Array(source_arr);
                if let Some(ct) = cell_type {
                    apply_cell_type(cell, ct);
                } else {
                    reset_code_execution_state(cell);
                }
            }
            EditMode::Insert => {
                let ct = cell_type.expect("validated above");
                if cell_index > cells.len() {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "cell_index {cell_index} out of range for insert (notebook has {} cells)",
                            cells.len()
                        )),
                    });
                }
                let insert_pos = if cell_index == cells.len() {
                    cells.len()
                } else {
                    cell_index + 1
                };

                let new_cell = match ct {
                    "code" => json!({
                        "cell_type": "code",
                        "execution_count": null,
                        "metadata": {},
                        "outputs": [],
                        "source": source_arr
                    }),
                    "markdown" => json!({
                        "cell_type": "markdown",
                        "metadata": {},
                        "source": source_arr
                    }),
                    "raw" => json!({
                        "cell_type": "raw",
                        "metadata": {},
                        "source": source_arr
                    }),
                    _ => unreachable!(),
                };

                cells.insert(insert_pos, new_cell);
            }
            EditMode::Delete => {
                if cell_index >= cells.len() {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "cell_index {cell_index} out of range (notebook has {} cells)",
                            cells.len()
                        )),
                    });
                }
                cells.remove(cell_index);
            }
        }

        let out = match notebook_to_string_pretty_one_space(&nb) {
            Ok(s) => s,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to serialize notebook: {e}")),
                });
            }
        };

        match tokio::fs::write(&resolved_target, out.as_bytes()).await {
            Ok(()) => Ok(ToolResult {
                success: true,
                output: format!(
                    "Updated notebook {notebook_path}: {edit_mode_str} at cell index {cell_index} ({} bytes written)",
                    out.len()
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

    fn minimal_nb_json() -> String {
        r##"{
 "nbformat": 4,
 "nbformat_minor": 5,
 "metadata": {},
 "cells": [
  {
   "cell_type": "code",
   "execution_count": 1,
   "metadata": {},
   "outputs": [{"output_type": "stream", "name": "stdout", "text": ["hi\n"]}],
   "source": ["print(1)\n"]
  },
  {
   "cell_type": "markdown",
   "metadata": {},
   "source": ["# Title\n"]
  }
 ]
}"##
        .to_string()
    }

    #[test]
    fn notebook_edit_name() {
        let tool = NotebookEditTool::new(test_security(std::env::temp_dir()));
        assert_eq!(tool.name(), "notebook_edit");
    }

    #[test]
    fn notebook_edit_schema_has_required_params() {
        let tool = NotebookEditTool::new(test_security(std::env::temp_dir()));
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["notebook_path"].is_object());
        assert!(schema["properties"]["cell_index"].is_object());
        assert!(schema["properties"]["new_source"].is_object());
        assert!(schema["properties"]["cell_type"].is_object());
        assert!(schema["properties"]["edit_mode"].is_object());
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("notebook_path")));
        assert!(required.contains(&json!("cell_index")));
    }

    #[tokio::test]
    async fn notebook_edit_replaces_cell() {
        let dir = std::env::temp_dir().join("sen_test_notebook_edit_replace");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("test.ipynb"), minimal_nb_json())
            .await
            .unwrap();

        let tool = NotebookEditTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({
                "notebook_path": "test.ipynb",
                "cell_index": 0,
                "new_source": "print(99)",
                "edit_mode": "replace"
            }))
            .await
            .unwrap();

        assert!(result.success, "replace should succeed: {:?}", result.error);

        let raw = tokio::fs::read_to_string(dir.join("test.ipynb"))
            .await
            .unwrap();
        let nb: Value = serde_json::from_str(&raw).unwrap();
        let cells = nb["cells"].as_array().unwrap();
        assert_eq!(cells[0]["source"], json!(["print(99)"]));
        assert_eq!(cells[0]["execution_count"], Value::Null);
        assert_eq!(cells[0]["outputs"], json!([]));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn notebook_edit_inserts_cell() {
        let dir = std::env::temp_dir().join("sen_test_notebook_edit_insert");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("test.ipynb"), minimal_nb_json())
            .await
            .unwrap();

        let tool = NotebookEditTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({
                "notebook_path": "test.ipynb",
                "cell_index": 0,
                "new_source": "# New",
                "cell_type": "markdown",
                "edit_mode": "insert"
            }))
            .await
            .unwrap();

        assert!(result.success, "insert should succeed: {:?}", result.error);

        let raw = tokio::fs::read_to_string(dir.join("test.ipynb"))
            .await
            .unwrap();
        let nb: Value = serde_json::from_str(&raw).unwrap();
        let cells = nb["cells"].as_array().unwrap();
        assert_eq!(cells.len(), 3);
        assert_eq!(cells[1]["cell_type"], json!("markdown"));
        assert_eq!(cells[1]["source"], json!(["# New"]));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn notebook_edit_deletes_cell() {
        let dir = std::env::temp_dir().join("sen_test_notebook_edit_delete");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("test.ipynb"), minimal_nb_json())
            .await
            .unwrap();

        let tool = NotebookEditTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({
                "notebook_path": "test.ipynb",
                "cell_index": 0,
                "edit_mode": "delete"
            }))
            .await
            .unwrap();

        assert!(result.success, "delete should succeed: {:?}", result.error);

        let raw = tokio::fs::read_to_string(dir.join("test.ipynb"))
            .await
            .unwrap();
        let nb: Value = serde_json::from_str(&raw).unwrap();
        let cells = nb["cells"].as_array().unwrap();
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0]["cell_type"], json!("markdown"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn notebook_edit_rejects_non_ipynb() {
        let dir = std::env::temp_dir().join("sen_test_notebook_edit_ext");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("x.json"), "{}").await.unwrap();

        let tool = NotebookEditTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({
                "notebook_path": "x.json",
                "cell_index": 0,
                "new_source": "x",
                "edit_mode": "replace"
            }))
            .await
            .unwrap();

        assert!(!result.success);
        assert!(
            result
                .error
                .as_deref()
                .unwrap_or("")
                .contains("Only .ipynb")
        );

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn notebook_edit_invalid_json() {
        let dir = std::env::temp_dir().join("sen_test_notebook_edit_badjson");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("bad.ipynb"), "not json {{{")
            .await
            .unwrap();

        let tool = NotebookEditTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({
                "notebook_path": "bad.ipynb",
                "cell_index": 0,
                "new_source": "x",
                "edit_mode": "replace"
            }))
            .await
            .unwrap();

        assert!(!result.success);
        assert!(
            result
                .error
                .as_deref()
                .unwrap_or("")
                .contains("Invalid JSON")
        );

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
}
