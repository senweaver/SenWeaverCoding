// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Tool, ToolResult};
use crate::apply_model::{EditBatch, EditOp, EditOrigin, NotebookCellOp, OpsApplier};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::Arc;

pub struct NotebookEditTool {
    security: Arc<SecurityPolicy>,
    ops_applier: Arc<OpsApplier>,
}

impl NotebookEditTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        let ops_applier = Arc::new(
            OpsApplier::default_for_shared_workspace(security.workspace_root_handle())
                .with_allowed_roots(security.allowed_roots.clone()),
        );
        Self {
            security,
            ops_applier,
        }
    }

    #[must_use]
    pub fn with_ops_applier(mut self, ops_applier: Arc<OpsApplier>) -> Self {
        self.ops_applier = ops_applier;
        self
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

pub(crate) fn string_to_source_array(s: &str) -> Vec<Value> {
    if s.is_empty() {
        return vec![json!("")];
    }
    s.split_inclusive('\n').map(|piece| json!(piece)).collect()
}

pub(crate) fn notebook_to_string_pretty_one_space(value: &Value) -> anyhow::Result<String> {
    let mut buf = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
    value.serialize(&mut ser)?;
    let mut out = String::from_utf8(buf)?;
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

pub(crate) fn apply_cell_type(cell: &mut Value, cell_type: &str) {
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

pub(crate) fn reset_code_execution_state(cell: &mut Value) {
    if cell.get("cell_type").and_then(|v| v.as_str()) == Some("code") {
        cell["execution_count"] = Value::Null;
        cell["outputs"] = json!([]);
    }
}

fn new_cell_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

pub(crate) fn apply_notebook_cell_op(
    nb: &mut Value,
    op: &crate::apply_model::NotebookCellOp,
) -> anyhow::Result<()> {
    let cells = nb
        .get_mut("cells")
        .and_then(|c| c.as_array_mut())
        .ok_or_else(|| anyhow::anyhow!("Invalid notebook: missing top-level \"cells\" array"))?;

    match op {
        crate::apply_model::NotebookCellOp::Replace {
            cell_index,
            new_source,
            cell_type,
        } => {
            if *cell_index >= cells.len() {
                anyhow::bail!(
                    "cell_index {cell_index} out of range (notebook has {} cells)",
                    cells.len()
                );
            }
            let cell = &mut cells[*cell_index];
            cell["source"] = Value::Array(string_to_source_array(new_source));
            if let Some(ct) = cell_type {
                apply_cell_type(cell, ct);
            } else {
                reset_code_execution_state(cell);
            }
        }
        crate::apply_model::NotebookCellOp::Insert {
            cell_index,
            new_source,
            cell_type,
            insert_before,
        } => {
            if *cell_index > cells.len() {
                anyhow::bail!(
                    "cell_index {cell_index} out of range for insert (notebook has {} cells)",
                    cells.len()
                );
            }
            let insert_pos = if *insert_before {
                (*cell_index).min(cells.len())
            } else if *cell_index == cells.len() {
                cells.len()
            } else {
                *cell_index + 1
            };

            let source_arr = string_to_source_array(new_source);
            let cell_id = new_cell_id();
            let new_cell = match cell_type.as_str() {
                "code" => json!({
                    "cell_type": "code",
                    "id": cell_id,
                    "execution_count": null,
                    "metadata": {},
                    "outputs": [],
                    "source": source_arr,
                }),
                "markdown" => json!({
                    "cell_type": "markdown",
                    "id": cell_id,
                    "metadata": {},
                    "source": source_arr,
                }),
                "raw" => json!({
                    "cell_type": "raw",
                    "id": cell_id,
                    "metadata": {},
                    "source": source_arr,
                }),
                other => anyhow::bail!(
                    "Invalid cell_type: {other} (expected code, markdown, or raw)"
                ),
            };
            cells.insert(insert_pos, new_cell);
        }
        crate::apply_model::NotebookCellOp::Delete { cell_index } => {
            if *cell_index >= cells.len() {
                anyhow::bail!(
                    "cell_index {cell_index} out of range (notebook has {} cells)",
                    cells.len()
                );
            }
            cells.remove(*cell_index);
        }
    }

    Ok(())
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
                    "description": "0-based cell index to edit (for insert with default position=after: the cell after which to insert; use index equal to cells.len() to append at end; on an empty notebook use 0)"
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
                },
                "position": {
                    "type": "string",
                    "enum": ["after", "before"],
                    "default": "after",
                    "description": "For insert mode: whether the new cell goes after (default) or before cell_index. Use position=before with cell_index=0 to insert at the very top."
                }
            },
            "required": ["notebook_path", "cell_index"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {

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

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: action budget exhausted".into()),
            });
        }

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

        let nb: Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Invalid JSON in notebook: {e}")),
                });
            }
        };

        let Some(cells_view) = nb.get("cells").and_then(|c| c.as_array()) else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Invalid notebook: missing top-level \"cells\" array".into()),
            });
        };

        let new_source_owned = new_source.unwrap_or("").to_string();
        let cell_op = match edit_mode {
            EditMode::Replace => {
                if cell_index >= cells_view.len() {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "cell_index {cell_index} out of range (notebook has {} cells)",
                            cells_view.len()
                        )),
                    });
                }
                NotebookCellOp::Replace {
                    cell_index,
                    new_source: new_source_owned,
                    cell_type: cell_type.map(|s| s.to_string()),
                }
            }
            EditMode::Insert => {
                let Some(ct) = cell_type.map(|s| s.to_string()) else {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some("cell_type is required for insert mode".into()),
                    });
                };
                if cell_index > cells_view.len() {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "cell_index {cell_index} out of range for insert (notebook has {} cells)",
                            cells_view.len()
                        )),
                    });
                }
                NotebookCellOp::Insert {
                    cell_index,
                    new_source: new_source_owned,
                    cell_type: ct,
                    insert_before: args
                        .get("position")
                        .and_then(|v| v.as_str())
                        .is_some_and(|p| p.eq_ignore_ascii_case("before")),
                }
            }
            EditMode::Delete => {
                if cell_index >= cells_view.len() {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "cell_index {cell_index} out of range (notebook has {} cells)",
                            cells_view.len()
                        )),
                    });
                }
                NotebookCellOp::Delete { cell_index }
            }
        };

        let batch = EditBatch::new(EditOrigin::NotebookEditTool).with_op(EditOp::NotebookCell {
            path: resolved_target.clone(),
            cell: cell_op,
        });
        let batch_id = batch.batch_id.clone();
        let before_bytes = tokio::fs::read(&resolved_target).await.ok();

        match self.ops_applier.apply_batch(batch).await {
            Ok(_) => {
                let after_bytes = tokio::fs::read(&resolved_target).await.ok();
                let out_len = after_bytes
                    .as_ref()
                    .map(|b| b.len() as u64)
                    .unwrap_or(0);
                if let Some(after) = after_bytes.as_deref() {
                    crate::agent::file_edit_emitter::emit_file_edit(
                        &resolved_target,
                        before_bytes.as_deref(),
                        Some(after),
                        Some(batch_id),
                    )
                    .await;
                }
                Ok(ToolResult {
                    success: true,
                    output: format!(
                        "Updated notebook {notebook_path}: {edit_mode_str} at cell index {cell_index} ({} bytes written)",
                        out_len
                    ),
                    error: None,
                })
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to apply notebook edit: {e}")),
            }),
        }
    }
}
