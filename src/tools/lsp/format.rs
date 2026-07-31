// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

pub struct LspFormatTool {
    security: Arc<SecurityPolicy>,
}

impl LspFormatTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
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
         (textDocument/formatting). Applies the returned text edits in place."
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
        let file_uri = format!(
            "file://{}",
            file_path.display().to_string().replace('\\', "/")
        );

        let params = json!({
            "textDocument": { "uri": file_uri },
            "options": {
                "tabSize": tab_size,
                "insertSpaces": insert_spaces,
            }
        });

        let resp = svc
            .lsp
            .request(
                lang,
                &self.security.workspace_dir(),
                Some(&file_path),
                "textDocument/formatting",
                params,
            )
            .await
            .map_err(|e| anyhow::anyhow!("LSP formatting failed: {e}"))?;

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: action budget exhausted".into()),
            });
        }

        let _write_guard = match crate::session::acquire_file_write_guard(&file_path).await {
            Ok(guard) => guard,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("{e}")),
                });
            }
        };

        let file_for_apply = file_path.clone();
        let security = self.security.clone();
        let edits_applied =
            tokio::task::spawn_blocking(move || apply_text_edits(&security, &file_for_apply, &resp))
                .await
                .unwrap_or(Ok(0))
                .map_err(|e| anyhow::anyhow!("Failed to apply formatting edits: {e}"))?;

        if edits_applied > 0 {
            crate::session::record_write_for_current_session(&file_path);
        }

        Ok(ToolResult {
            success: true,
            output: format!(
                "Formatted {} ({} edit(s) applied via language server)",
                file_path.display(),
                edits_applied
            ),
            error: None,
        })
    }
}

fn apply_text_edits(
    security: &SecurityPolicy,
    file_path: &PathBuf,
    resp: &serde_json::Value,
) -> std::io::Result<usize> {
    let edits = match resp.as_array() {
        Some(arr) if !arr.is_empty() => arr,
        _ => return Ok(0),
    };

    if let Ok(meta) = std::fs::symlink_metadata(file_path) {
        if meta.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("Refusing to write through symlink: {}", file_path.display()),
            ));
        }
    }
    let resolved = std::fs::canonicalize(file_path)?;
    if !security.is_resolved_path_allowed(&resolved) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            security.resolved_path_violation_message(&resolved),
        ));
    }

    let content = std::fs::read_to_string(&resolved)?;
    let mut lines: Vec<String> = content.lines().map(String::from).collect();

    let mut sorted: Vec<(usize, usize, usize, usize, String)> = edits
        .iter()
        .filter_map(|e| {
            let sl = e.pointer("/range/start/line")?.as_u64()? as usize;
            let sc = e.pointer("/range/start/character")?.as_u64()? as usize;
            let el = e.pointer("/range/end/line")?.as_u64()? as usize;
            let ec = e.pointer("/range/end/character")?.as_u64()? as usize;
            let new_text = e.get("newText")?.as_str()?.to_string();
            Some((sl, sc, el, ec, new_text))
        })
        .collect();

    if sorted.is_empty() {
        return Ok(0);
    }

    sorted.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));

    let mut applied = 0;
    for (sl, sc, el, ec, new_text) in sorted {
        if sl >= lines.len() {
            continue;
        }
        if sl == el {
            if let Some(line) = lines.get_mut(sl) {
                let chars: Vec<char> = line.chars().collect();
                let sc = sc.min(chars.len());
                let ec = ec.min(chars.len());
                let before: String = chars[..sc].iter().collect();
                let after: String = chars[ec..].iter().collect();
                *line = format!("{before}{new_text}{after}");
                applied += 1;
            }
        } else {
            let el = el.min(lines.len().saturating_sub(1));
            if el < sl {
                continue;
            }
            let start_chars: Vec<char> = lines[sl].chars().collect();
            let sc = sc.min(start_chars.len());
            let before: String = start_chars[..sc].iter().collect();
            let end_chars: Vec<char> = lines[el].chars().collect();
            let ec = ec.min(end_chars.len());
            let after: String = end_chars[ec..].iter().collect();
            let replacement = format!("{before}{new_text}{after}");
            let new_block: Vec<String> = replacement.split('\n').map(String::from).collect();
            lines.splice(sl..=el, new_block);
            applied += 1;
        }
    }

    let mut output = lines.join("\n");
    if content.ends_with('\n') && !output.ends_with('\n') {
        output.push('\n');
    }
    crate::util::atomic_write(&resolved, output.as_bytes())?;
    Ok(applied)
}
