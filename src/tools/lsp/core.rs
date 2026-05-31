// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::super::traits::{Tool, ToolResult};
use crate::services::lsp::{self, LspService, ServerInfo};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct LspTool {
    lsp: LspService,
    workspace_root: Arc<RwLock<PathBuf>>,
}

impl LspTool {
    pub fn new(workspace_root: Arc<RwLock<PathBuf>>) -> Self {
        let lsp = crate::services::try_get_services()
            .map(|svc| svc.lsp.clone())
            .unwrap_or_else(LspService::new);
        Self {
            lsp,
            workspace_root,
        }
    }

    fn workspace_snapshot(&self) -> PathBuf {
        self.workspace_root.read().clone()
    }

    fn resolve_file_path(workspace_root: &Path, file_path: &str) -> PathBuf {
        let p = PathBuf::from(file_path);
        if p.is_absolute() {
            p
        } else {
            workspace_root.join(p)
        }
    }
}

impl Default for LspTool {
    fn default() -> Self {
        Self::new(Arc::new(RwLock::new(
            std::env::current_dir().unwrap_or_default(),
        )))
    }
}

#[async_trait]
impl Tool for LspTool {
    fn name(&self) -> &str {
        "lsp"
    }

    fn description(&self) -> &str {
        "Perform Language Server Protocol operations. Supports go-to-definition, find-references, hover, document-symbols, workspace-symbols, diagnostics, call-hierarchy, list-servers, shutdown-server, restart-server, and server-status."
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
                        "call_hierarchy",
                        "list_servers",
                        "shutdown_server",
                        "restart_server",
                        "server_status"
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
                },
                "workspace": {
                    "type": "string",
                    "description": "Workspace root directory for the server. Defaults to the tool's configured workspace."
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

        let workspace_dir = args
            .get("workspace")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.workspace_snapshot());

        let file_abs = file_path_str
            .map(|fp| Self::resolve_file_path(&workspace_dir, fp));

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
                    &workspace_dir,
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
                        &workspace_dir,
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
                    &workspace_dir,
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
                        &workspace_dir,
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
                        &workspace_dir,
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
                        &workspace_dir,
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
                        &workspace_dir,
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
                                    &workspace_dir,
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
            "list_servers" => Ok(self
                .lsp
                .list_servers()
                .await
                .into_iter()
                .map(|info| {
                    format!(
                        "Server: language={}, workspace={}, status={}, open_files={}",
                        info.language_id,
                        info.workspace_root.display(),
                        info.status,
                        info.open_files
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")),
            "shutdown_server" => {
                self.lsp
                    .shutdown_server(&language, &workspace_dir)
                    .await;
                Ok(format!(
                    "Server for '{}' at '{}' has been shut down.",
                    language,
                    workspace_dir.display()
                ))
            }
            "restart_server" => {
                self.lsp
                    .restart_server(&language, &workspace_dir)
                    .await?;
                Ok(format!(
                    "Server for '{}' at '{}' has been restarted.",
                    language,
                    workspace_dir.display()
                ))
            }
            "server_status" => {
                let servers = self.lsp.list_servers().await;
                if servers.is_empty() {
                    Ok("No LSP servers currently running.".to_string())
                } else {
                    let mut out = format!("Running servers ({}):\n", servers.len());
                    for ServerInfo {
                        language_id,
                        workspace_root,
                        status,
                        open_files,
                    } in servers
                    {
                        out.push_str(&format!(
                            "  - {} @ {} [status={}, files={}]\n",
                            language_id,
                            workspace_root.display(),
                            status,
                            open_files
                        ));
                    }
                    Ok(out)
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

    async fn exec_position_op(
        &self,
        workspace_dir: &Path,
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
            .request(language, workspace_dir, Some(fp), lsp_method, params)
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

fn require_file<'a>(file_abs: Option<&'a Path>, op: &str) -> anyhow::Result<&'a Path> {
    file_abs.ok_or_else(|| anyhow::anyhow!("'file_path' is required for {op}"))
}

fn require_position(
    line: Option<u64>,
    character: Option<u64>,
    op: &str,
) -> anyhow::Result<(u64, u64)> {
    let ln = line.ok_or_else(|| anyhow::anyhow!("'line' is required for {op}"))?;
    let ch = character.ok_or_else(|| anyhow::anyhow!("'character' is required for {op}"))?;
    Ok((ln.saturating_sub(1), ch.saturating_sub(1)))
}
