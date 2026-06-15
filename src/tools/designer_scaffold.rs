// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use serde_json::{json, Value};

use super::traits::{Tool, ToolResult};
use crate::agent::designer::scaffold;

const MAX_OUTPUT: usize = 200_000;

pub struct DesignerScaffoldTool;

impl DesignerScaffoldTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DesignerScaffoldTool {
    fn default() -> Self {
        Self::new()
    }
}

fn resolve_dest(rel_path: &str) -> Result<(std::path::PathBuf, String), String> {
    let rel = rel_path.trim().trim_start_matches('/');
    if rel.is_empty() {
        return Err("`dest` must be a non-empty workspace-relative path.".to_string());
    }
    if rel.contains("..") {
        return Err("Path traversal is not allowed in `dest`.".to_string());
    }
    let session = crate::session::current_session_context()
        .ok_or_else(|| "No active session workspace.".to_string())?;
    let workspace = std::path::PathBuf::from(&session.workspace_dir);
    let candidate = workspace.join(rel);
    Ok((candidate, rel.replace('\\', "/")))
}

#[async_trait]
impl Tool for DesignerScaffoldTool {
    fn name(&self) -> &str {
        "designer_scaffold"
    }

    fn description(&self) -> &str {
        "Drop a bundled design scaffold into the workspace or read it (Designer mode). Scaffolds are curated building blocks: background/surface CSS treatments, UI primitives (command palette, kanban, toast, drawer, skeleton, empty states, stepper, file tree), app shells, landing hero, device/browser frames, and the DESIGN.md design-system starter. Call without arguments to list every scaffold. Provide `id` to read one. Provide `id` + `dest` (workspace-relative file path) to write it to disk in one step. CSS scaffolds paste into your artifact's <style>; JSX scaffolds are reference patterns to adapt into self-contained HTML/CSS — never ship them with a React dependency."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Scaffold id (e.g. aurora-mesh-bg, cmdk-palette, design-system-starter). Omit to list all scaffolds."
                },
                "dest": {
                    "type": "string",
                    "description": "Optional workspace-relative destination path. When set, the scaffold content is written to this file."
                }
            }
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let id = args
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .trim()
            .to_string();

        if id.is_empty() {
            return Ok(ToolResult {
                success: true,
                output: format!("Available design scaffolds:\n\n{}", scaffold::listing()),
                error: None,
            });
        }

        let Some(content) = scaffold::read(&id) else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Unknown scaffold id `{id}`. Call without arguments to list available scaffolds."
                )),
            });
        };

        let dest = args
            .get("dest")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());

        match dest {
            Some(rel_dest) => {
                let (abs, rel) = match resolve_dest(rel_dest) {
                    Ok(v) => v,
                    Err(e) => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(e),
                        });
                    }
                };
                if let Some(parent) = abs.parent() {
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("Could not create directory for `{rel}`: {e}")),
                        });
                    }
                }
                if let Err(e) = std::fs::write(&abs, content) {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Could not write `{rel}`: {e}")),
                    });
                }
                crate::agent::designer::record_artifact_if_designer(&abs);
                Ok(ToolResult {
                    success: true,
                    output: format!(
                        "Scaffold `{id}` written to `{rel}` ({} bytes). Adapt it to the brief and \
                         the active design baton before shipping.",
                        content.len()
                    ),
                    error: None,
                })
            }
            None => {
                let mut body = content.to_string();
                let truncated = body.len() > MAX_OUTPUT;
                if truncated {
                    body.truncate(MAX_OUTPUT);
                }
                let suffix = if truncated { "\n\n[truncated]" } else { "" };
                Ok(ToolResult {
                    success: true,
                    output: format!("# scaffold {id}\n\n{body}{suffix}"),
                    error: None,
                })
            }
        }
    }
}
