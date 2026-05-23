// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Tool, ToolResult};
use crate::services::lsp::{self, LspService};
use crate::services::lsp_pool;
use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;

pub struct LspSymbolsTool {
    lsp: LspService,
    workspace_dir: PathBuf,
}

impl LspSymbolsTool {
    pub fn new(workspace_dir: PathBuf) -> Self {
        let lsp = crate::services::try_get_services()
            .map(|svc| svc.lsp.clone())
            .unwrap_or_else(LspService::new);
        Self { lsp, workspace_dir }
    }

    fn resolve_path(&self, p: &str) -> PathBuf {
        let candidate = PathBuf::from(p);
        if candidate.is_absolute() {
            candidate
        } else {
            self.workspace_dir.join(candidate)
        }
    }
}

impl Default for LspSymbolsTool {
    fn default() -> Self {
        Self::new(std::env::current_dir().unwrap_or_default())
    }
}

fn language_for_path(path: &std::path::Path) -> Option<&'static str> {
    let ext = path.extension().and_then(|s| s.to_str())?;
    match ext.to_ascii_lowercase().as_str() {
        "rs" => Some("rust"),
        "py" | "pyi" => Some("python"),
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" => Some("typescript"),
        "go" => Some("go"),
        "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => Some("cpp"),
        _ => None,
    }
}

#[async_trait]
impl Tool for LspSymbolsTool {
    fn name(&self) -> &str {
        "lsp_symbols"
    }

    fn description(&self) -> &str {
        "Return the document outline (symbols) for a source file via LSP. \
         Auto-detects language from the file extension and restricts itself \
         to the tier-1 pool (Rust, Python, TypeScript, Go, C/C++)."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Source file path (absolute or workspace-relative)."
                },
                "language": {
                    "type": "string",
                    "description": "Optional explicit language id \
                                    (rust, python, typescript, go, cpp). \
                                    When omitted, inferred from the extension."
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let path = match args.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("missing 'path' parameter".into()),
                });
            }
        };

        let resolved = self.resolve_path(path);
        let language = args
            .get("language")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| language_for_path(&resolved).map(|s| s.to_string()));
        let language = match language {
            Some(l) => l,
            None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "cannot infer language for '{}'; supply `language` explicitly",
                        resolved.display()
                    )),
                });
            }
        };

        if lsp_pool::find(&language).is_none() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "language '{language}' is not in the tier-1 LSP pool"
                )),
            });
        }

        let uri = lsp::path_to_uri(&resolved);
        let params = json!({ "textDocument": { "uri": uri } });
        match self
            .lsp
            .request(
                &language,
                &self.workspace_dir,
                Some(&resolved),
                "textDocument/documentSymbol",
                params,
            )
            .await
        {
            Ok(result) => Ok(ToolResult {
                success: true,
                output: lsp::format_document_symbols(&result),
                error: None,
            }),
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("lsp document_symbols failed: {e}")),
            }),
        }
    }
}
