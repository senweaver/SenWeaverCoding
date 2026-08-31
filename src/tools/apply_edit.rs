// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use super::edit_history::EditHistory;
use super::file::write::FileWriteTool;
use super::traits::{Tool, ToolResult};
use crate::apply_model::OpsApplier;
use crate::security::SecurityPolicy;

pub struct ApplyEditTool {
    security: Arc<SecurityPolicy>,
    writer: FileWriteTool,
}

impl ApplyEditTool {
    pub fn new(
        security: Arc<SecurityPolicy>,
        ops_applier: Arc<OpsApplier>,
        edit_history: Option<Arc<EditHistory>>,
    ) -> Self {
        let mut writer = FileWriteTool::new(security.clone()).with_ops_applier(ops_applier);
        if let Some(history) = edit_history {
            writer = writer.with_edit_history(history);
        }
        Self { security, writer }
    }

    fn err(msg: impl Into<String>) -> ToolResult {
        ToolResult {
            success: false,
            output: String::new(),
            error: Some(msg.into()),
        }
    }
}

#[async_trait]
impl Tool for ApplyEditTool {
    fn name(&self) -> &str {
        "apply_edit"
    }

    fn description(&self) -> &str {
        "Apply a lazy edit snippet to an EXISTING file using a fast apply model. Provide only the \
         changed lines and use `// ... existing code ...` markers to represent every unchanged \
         region; the apply model merges the snippet into the current file, validates the result \
         (tree-sitter + shrink guard), and writes it atomically. Use this for multi-region or \
         structural edits where an exact old_string match is awkward. To create a brand-new file \
         use file_write; for a single precise substring change file_edit is cheaper."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path (relative to the workspace) of the existing file to edit."
                },
                "code_edit": {
                    "type": "string",
                    "description": "Only the changed lines. Use `// ... existing code ...` on its own line for every unchanged region (omitting it will delete that region). Preserve exact indentation."
                },
                "instructions": {
                    "type": "string",
                    "description": "A first-person, single-sentence description of the change (e.g. 'I am adding error handling to the auth handler'). Helps the apply model disambiguate."
                }
            },
            "required": ["file_path", "code_edit", "instructions"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let file_path = args
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'file_path' parameter"))?;
        let code_edit = args
            .get("code_edit")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'code_edit' parameter"))?;
        let instructions = args
            .get("instructions")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if !self.security.can_act() {
            return Ok(Self::err("Action blocked: autonomy is read-only"));
        }
        if !self.security.is_path_allowed(file_path) {
            return Ok(Self::err(format!(
                "Path not allowed by security policy: {file_path}"
            )));
        }

        let full_path = self.security.resolve_tool_path(file_path);

        let raw = match tokio::fs::read(&full_path).await {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::err(format!(
                    "apply_edit edits existing files; '{file_path}' does not exist (use file_write to create it)"
                )));
            }
            Err(e) => {
                return Ok(Self::err(format!("cannot read '{file_path}': {e}")));
            }
        };
        const MAX_APPLY_FILE_BYTES: usize = 10 * 1024 * 1024;
        if raw.len() > MAX_APPLY_FILE_BYTES {
            return Ok(Self::err(format!(
                "'{file_path}' is {} bytes, exceeding the {MAX_APPLY_FILE_BYTES} byte apply limit; edit a smaller region",
                raw.len()
            )));
        }
        if crate::tools::file::encoding::is_probably_binary(&raw) {
            return Ok(Self::err(format!("refusing to edit binary file '{file_path}'")));
        }
        let (source, _encoding_label) = match crate::tools::file::encoding::decode_for_edit(&raw) {
            Ok(decoded) => decoded,
            Err(e) => {
                return Ok(Self::err(format!("cannot decode '{file_path}' safely: {e}")));
            }
        };

        crate::session::record_read_for_current_session(&full_path);

        let Some(refiner) = crate::apply_model::fast_apply::runtime_ladder_refiner() else {
            return Ok(Self::err(
                "apply_edit requires a fast apply model. Configure agent_runtime.fast_apply_model \
                 (or a model_routes entry with hint=\"fast\") and ensure agent_runtime.apply_ladder_enabled is on.",
            ));
        };

        let merged = match refiner
            .merge_lazy_snippet(&source, code_edit, Some(instructions), Some(full_path.as_path()))
            .await
        {
            Ok(m) => m,
            Err(e) => {
                return Ok(Self::err(format!(
                    "apply_edit merge failed for '{file_path}': {e}. Retry with a more specific code_edit/instructions, or fall back to file_edit."
                )));
            }
        };

        if merged == source {
            return Ok(ToolResult {
                success: true,
                output: format!("No changes: merged content is identical to '{file_path}'."),
                error: None,
            });
        }

        self.writer
            .execute(json!({ "path": file_path, "content": merged }))
            .await
    }
}
