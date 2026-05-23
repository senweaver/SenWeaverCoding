// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Tool, ToolResult};
use crate::code_intel;
use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;

pub struct CodeOutlineTool {
    workspace_dir: PathBuf,
}

impl CodeOutlineTool {
    pub fn new(workspace_dir: PathBuf) -> Self {
        Self { workspace_dir }
    }

    fn resolve(&self, p: &str) -> PathBuf {
        let c = PathBuf::from(p);
        if c.is_absolute() {
            c
        } else {
            self.workspace_dir.join(c)
        }
    }
}

impl Default for CodeOutlineTool {
    fn default() -> Self {
        Self::new(std::env::current_dir().unwrap_or_default())
    }
}

#[async_trait]
impl Tool for CodeOutlineTool {
    fn name(&self) -> &str {
        "code_outline"
    }

    fn description(&self) -> &str {
        "Return the structural outline (functions/classes/structs) of a source file. \
         Uses tree-sitter when compiled with `code-intel`, otherwise a heuristic scan."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the source file (absolute or workspace-relative)."
                },
                "language": {
                    "type": "string",
                    "description": "Optional explicit language id (rust, python, typescript, go, cpp)."
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
        let resolved = self.resolve(path);
        let language = args.get("language").and_then(|v| v.as_str()).map(String::from);

        let extract_outcome = {
            let resolved_owned = resolved.clone();
            let language_owned = language.clone();
            tokio::task::spawn_blocking(move || {
                code_intel::extract_outline(&resolved_owned, language_owned.as_deref())
            })
            .await
            .map_err(|e| anyhow::anyhow!("code_outline join error: {e}"))?
        };
        match extract_outcome {
            Ok(entries) if entries.is_empty() => Ok(ToolResult {
                success: true,
                output: "(no outline entries found)".to_string(),
                error: None,
            }),
            Ok(entries) => {
                let mut buf = format!("Outline ({} entries)\n", entries.len());
                for e in entries {
                    buf.push_str(&format!(
                        "  {kind}: {name} (line {line})\n",
                        kind = e.kind,
                        name = e.name,
                        line = e.line
                    ));
                }
                Ok(ToolResult {
                    success: true,
                    output: buf,
                    error: None,
                })
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("code_outline failed: {e}")),
            }),
        }
    }
}
