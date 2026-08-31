// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::traits::{Tool, ToolResult};
use super::text_edit::{adapt_edit_newtext_eols, apply_edits_to_content, secure_resolve_target};
use crate::apply_model::{EditBatch, EditOp, EditOrigin, OpsApplier};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const LSP_FORMAT_TIMEOUT: Duration = Duration::from_secs(10);

pub struct LspFormatTool {
    security: Arc<SecurityPolicy>,
    ops_applier: Arc<OpsApplier>,
}

impl LspFormatTool {
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

    fn resolve_path(&self, file_path: &str) -> PathBuf {
        let p = PathBuf::from(file_path);
        if p.is_absolute() {
            p
        } else {
            self.security.workspace_dir().join(p)
        }
    }
}

#[async_trait]
impl Tool for LspFormatTool {
    fn name(&self) -> &str {
        "lsp_format"
    }

    fn description(&self) -> &str {
        "Format a source file using the language server's document formatting provider \
         (textDocument/formatting). Applies the returned text edits in place while preserving \
         the file's original line endings."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file to format (absolute or workspace-relative)."
                },
                "tab_size": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 16,
                    "description": "Indentation size in spaces (default 4)."
                },
                "insert_spaces": {
                    "type": "boolean",
                    "description": "Use spaces instead of tabs for indentation (default true)."
                }
            },
            "required": ["file_path"]
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

        let file_path_str = args
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'file_path' parameter"))?;

        if !self.security.is_path_allowed(file_path_str) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Path not allowed by security policy: {file_path_str}")),
            });
        }

        let tab_size = args.get("tab_size").and_then(|v| v.as_u64()).unwrap_or(4);
        let insert_spaces = args
            .get("insert_spaces")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let file_path = self.resolve_path(file_path_str);
        if !file_path.is_file() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("File not found: {}", file_path.display())),
            });
        }

        let svc = crate::services::try_get_services()
            .ok_or_else(|| anyhow::anyhow!("Services not initialized"))?;

        let lang = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let file_uri = crate::services::lsp::core::path_to_uri(&file_path);

        let params = json!({
            "textDocument": { "uri": file_uri },
            "options": {
                "tabSize": tab_size,
                "insertSpaces": insert_spaces,
            }
        });

        let workspace_dir = self.security.workspace_dir();
        let request_fut = svc.lsp.request(
            lang,
            &workspace_dir,
            Some(&file_path),
            "textDocument/formatting",
            params,
        );
        let resp = match tokio::time::timeout(LSP_FORMAT_TIMEOUT, request_fut).await {
            Ok(Ok(resp)) => resp,
            Ok(Err(e)) => return Err(anyhow::anyhow!("LSP formatting failed: {e}")),
            Err(_) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "LSP formatting request timed out after {}s",
                        LSP_FORMAT_TIMEOUT.as_secs()
                    )),
                });
            }
        };

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: action budget exhausted".into()),
            });
        }

        let edits = match resp.as_array() {
            Some(arr) if !arr.is_empty() => arr.clone(),
            _ => {
                return Ok(ToolResult {
                    success: true,
                    output: format!(
                        "No formatting edits returned for {}",
                        file_path.display()
                    ),
                    error: None,
                });
            }
        };

        let security = self.security.clone();
        let probe = file_path.clone();
        let resolved =
            match tokio::task::spawn_blocking(move || secure_resolve_target(&security, &probe))
                .await
                .map_err(|e| anyhow::anyhow!("Path resolution task failed: {e}"))?
            {
                Ok(p) => p,
                Err(e) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(e),
                    });
                }
            };

        let _write_guard = match crate::session::acquire_file_write_guard(&resolved).await {
            Ok(guard) => guard,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("{e}")),
                });
            }
        };

        let content = match tokio::fs::read_to_string(&resolved).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Failed to read {} for formatting: {e}",
                        resolved.display()
                    )),
                });
            }
        };

        let dominant = crate::tools::file::eol::dominant_eol(&content);
        let adapted_edits = adapt_edit_newtext_eols(&edits, dominant);
        let (new_content, applied, edit_errors) =
            apply_edits_to_content(&content, &adapted_edits);

        if new_content == content {
            return Ok(ToolResult {
                success: true,
                output: format!(
                    "{} already formatted (no changes needed)",
                    file_path.display()
                ),
                error: None,
            });
        }

        let op = EditOp::Replace {
            path: resolved.clone(),
            byte_range: 0..content.len(),
            old_text: content,
            new_text: new_content,
            anchor: None,
        };
        let batch = EditBatch::new(EditOrigin::LspFormatTool).with_op(op);
        if let Err(e) = self.ops_applier.apply_batch(batch).await {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to apply formatting edits: {e}")),
            });
        }
        crate::session::record_write_for_current_session(&resolved);

        let mut output = format!(
            "Formatted {} ({applied} edit(s) applied via language server)",
            file_path.display()
        );
        if !edit_errors.is_empty() {
            output.push_str(&format!(
                "\nSkipped {} malformed edit(s) returned by the server",
                edit_errors.len()
            ));
        }
        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}
