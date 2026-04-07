// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use crate::services::lsp::{self, LspService};
use async_trait::async_trait;
use serde_json::json;
use std::path::{Path, PathBuf};

/// Language Server Protocol integration tool.
///
/// Provides access to LSP operations like go-to-definition, find-references,
/// hover info, document symbols, workspace symbols, diagnostics, and call hierarchy.
pub struct LspTool {
    lsp: LspService,
    workspace_dir: PathBuf,
}

impl LspTool {
    pub fn new(workspace_dir: PathBuf) -> Self {
        let lsp = std::panic::catch_unwind(crate::services::get_services)
            .ok()
            .map(|svc| svc.lsp.clone())
            .unwrap_or_else(LspService::new);
        Self { lsp, workspace_dir }
    }

    fn resolve_path(&self, file_path: &str) -> PathBuf {
        let p = PathBuf::from(file_path);
        if p.is_absolute() {
            p
        } else {
            self.workspace_dir.join(p)
        }
    }
}

impl Default for LspTool {
    fn default() -> Self {
        Self::new(std::env::current_dir().unwrap_or_default())
    }
}

#[async_trait]
impl Tool for LspTool {
    fn name(&self) -> &str {
        "lsp"
    }

    fn description(&self) -> &str {
        "Perform Language Server Protocol operations. Supports go-to-definition, find-references, hover, document-symbols, workspace-symbols, diagnostics, and call-hierarchy."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "description": "The LSP operation to perform",
                    "enum": [
                        "goto_definition",
                        "find_references",
                        "hover",
                        "document_symbols",
                        "workspace_symbols",
                        "diagnostics",
                        "call_hierarchy"
                    ]
                },
                "file_path": {
                    "type": "string",
                    "description": "Path to the file (required for most operations)"
                },
                "line": {
                    "type": "integer",
                    "description": "1-based line number for position-based operations"
                },
                "character": {
                    "type": "integer",
                    "description": "1-based character offset for position-based operations"
                },
                "query": {
                    "type": "string",
                    "description": "Search query for workspace_symbols operation"
                },
                "language": {
                    "type": "string",
                    "description": "Language identifier (e.g., 'rust', 'python', 'typescript'). Auto-detected from file extension when not provided."
                }
            },
            "required": ["operation"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let operation = args
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'operation' parameter"))?;
        let file_path_str = args.get("file_path").and_then(|v| v.as_str());
        let line = args.get("line").and_then(|v| v.as_u64());
        let character = args.get("character").and_then(|v| v.as_u64());
        let query = args.get("query").and_then(|v| v.as_str());
        let language_override = args.get("language").and_then(|v| v.as_str());

        let file_abs = file_path_str.map(|fp| self.resolve_path(fp));

        let language = match language_override {
            Some(lang) => lang.to_string(),
            None => match file_abs.as_ref() {
                Some(fp) => lsp::detect_language(fp)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Cannot detect language for '{}'. Provide a 'language' parameter.",
                            fp.display()
                        )
                    })?
                    .to_string(),
                None => {
                    return Err(anyhow::anyhow!(
                        "Either 'file_path' or 'language' must be provided \
                         so the correct language server can be selected."
                    ));
                }
            },
        };

        let result = match operation {
            "goto_definition" => {
                self.exec_position_op(
                    &language,
                    file_abs.as_deref(),
                    file_path_str,
                    line,
                    character,
                    "textDocument/definition",
                    "goto_definition",
                )
                .await
            }
            "find_references" => {
                let fp = require_file(file_abs.as_deref(), "find_references")?;
                let (ln, ch) = require_position(line, character, "find_references")?;
                let uri = lsp::path_to_uri(fp);
                let params = json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": ln, "character": ch },
                    "context": { "includeDeclaration": true }
                });
                match self
                    .lsp
                    .request(
                        &language,
                        &self.workspace_dir,
                        Some(fp),
                        "textDocument/references",
                        params,
                    )
                    .await
                {
                    Ok(resp) => Ok(lsp::format_locations(&resp, "References")),
                    Err(e) => Err(e),
                }
            }
            "hover" => {
                self.exec_position_op(
                    &language,
                    file_abs.as_deref(),
                    file_path_str,
                    line,
                    character,
                    "textDocument/hover",
                    "hover",
                )
                .await
            }
            "document_symbols" => {
                let fp = require_file(file_abs.as_deref(), "document_symbols")?;
                let uri = lsp::path_to_uri(fp);
                let params = json!({ "textDocument": { "uri": uri } });
                match self
                    .lsp
                    .request(
                        &language,
                        &self.workspace_dir,
                        Some(fp),
                        "textDocument/documentSymbol",
                        params,
                    )
                    .await
                {
                    Ok(resp) => Ok(lsp::format_document_symbols(&resp)),
                    Err(e) => Err(e),
                }
            }
            "workspace_symbols" => {
                let q = query.unwrap_or("");
                let params = json!({ "query": q });
                match self
                    .lsp
                    .request(
                        &language,
                        &self.workspace_dir,
                        None,
                        "workspace/symbol",
                        params,
                    )
                    .await
                {
                    Ok(resp) => Ok(lsp::format_workspace_symbols(&resp)),
                    Err(e) => Err(e),
                }
            }
            "diagnostics" => {
                let fp = require_file(file_abs.as_deref(), "diagnostics")?;
                let display = file_path_str.unwrap_or("?");
                let uri = lsp::path_to_uri(fp);
                let params = json!({ "textDocument": { "uri": uri } });
                match self
                    .lsp
                    .request(
                        &language,
                        &self.workspace_dir,
                        Some(fp),
                        "textDocument/diagnostic",
                        params,
                    )
                    .await
                {
                    Ok(resp) => Ok(lsp::format_diagnostics(&resp, display)),
                    Err(e) => Err(e),
                }
            }
            "call_hierarchy" => {
                let fp = require_file(file_abs.as_deref(), "call_hierarchy")?;
                let (ln, ch) = require_position(line, character, "call_hierarchy")?;
                let uri = lsp::path_to_uri(fp);

                let prepare_params = json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": ln, "character": ch }
                });

                match self
                    .lsp
                    .request(
                        &language,
                        &self.workspace_dir,
                        Some(fp),
                        "textDocument/prepareCallHierarchy",
                        prepare_params,
                    )
                    .await
                {
                    Ok(items) => {
                        let first_item = items
                            .as_array()
                            .and_then(|a| a.first())
                            .cloned()
                            .unwrap_or(json!(null));

                        if first_item.is_null() {
                            Ok(lsp::format_call_hierarchy(&items, &json!(null), "Incoming"))
                        } else {
                            let calls_params = json!({ "item": first_item });
                            match self
                                .lsp
                                .request(
                                    &language,
                                    &self.workspace_dir,
                                    None,
                                    "callHierarchy/incomingCalls",
                                    calls_params,
                                )
                                .await
                            {
                                Ok(calls) => {
                                    Ok(lsp::format_call_hierarchy(&items, &calls, "Incoming"))
                                }
                                Err(e) => Err(e),
                            }
                        }
                    }
                    Err(e) => Err(e),
                }
            }
            other => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Unknown LSP operation: '{other}'")),
                });
            }
        };

        match result {
            Ok(output) => Ok(ToolResult {
                success: true,
                output,
                error: None,
            }),
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("{e:#}")),
            }),
        }
    }
}

impl LspTool {
    /// Shared helper for position-based operations (definition, hover).
    async fn exec_position_op(
        &self,
        language: &str,
        file_abs: Option<&Path>,
        file_path_str: Option<&str>,
        line: Option<u64>,
        character: Option<u64>,
        lsp_method: &str,
        op_name: &str,
    ) -> Result<String, anyhow::Error> {
        let fp = require_file(file_abs, op_name)?;
        let (ln, ch) = require_position(line, character, op_name)?;
        let uri = lsp::path_to_uri(fp);
        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": ln, "character": ch }
        });

        let resp = self
            .lsp
            .request(language, &self.workspace_dir, Some(fp), lsp_method, params)
            .await?;

        if lsp_method == "textDocument/hover" {
            Ok(lsp::format_hover(&resp))
        } else {
            let label = match op_name {
                "goto_definition" => "Definition",
                "find_references" => "References",
                _ => op_name,
            };
            Ok(lsp::format_locations(
                &resp,
                &format!(
                    "{label} at {}:{}:{}",
                    file_path_str.unwrap_or("?"),
                    ln + 1,
                    ch + 1
                ),
            ))
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn require_file<'a>(file_abs: Option<&'a Path>, op: &str) -> anyhow::Result<&'a Path> {
    file_abs.ok_or_else(|| anyhow::anyhow!("'file_path' is required for {op}"))
}

/// Convert 1-based user input to 0-based LSP positions.
fn require_position(
    line: Option<u64>,
    character: Option<u64>,
    op: &str,
) -> anyhow::Result<(u64, u64)> {
    let ln = line.ok_or_else(|| anyhow::anyhow!("'line' is required for {op}"))?;
    let ch = character.ok_or_else(|| anyhow::anyhow!("'character' is required for {op}"))?;
    Ok((ln.saturating_sub(1), ch.saturating_sub(1)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn name_matches() {
        assert_eq!(LspTool::default().name(), "lsp");
    }

    #[test]
    fn schema_has_operation_enum() {
        let tool = LspTool::default();
        let schema = tool.parameters_schema();
        let ops = schema["properties"]["operation"]["enum"]
            .as_array()
            .unwrap();
        assert!(ops.contains(&json!("goto_definition")));
        assert!(ops.contains(&json!("hover")));
    }

    #[tokio::test]
    async fn hover_requires_file_and_position() {
        let tool = LspTool::default();
        let result = tool.execute(json!({"operation": "hover"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn workspace_symbols_requires_language() {
        let tool = LspTool::default();
        let result = tool
            .execute(json!({"operation": "workspace_symbols", "query": "Foo"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn unknown_operation() {
        let tool = LspTool::default();
        let result = tool.execute(json!({"operation": "invalid"})).await.unwrap();
        assert!(!result.success);
    }
}
