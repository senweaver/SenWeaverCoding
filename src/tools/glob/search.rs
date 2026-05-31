// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

const MAX_RESULTS: usize = 1000;

pub struct GlobSearchTool {
    security: Arc<SecurityPolicy>,
}

impl GlobSearchTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for GlobSearchTool {
    fn name(&self) -> &str {
        "glob_search"
    }

    fn description(&self) -> &str {
        "Search for files matching a glob pattern within the workspace. \
         Returns a sorted list of matching file paths relative to the workspace root. \
         Examples: '**/*.rs' (all Rust files), 'src/**/mod.rs' (all mod.rs in src)."
    }

    fn mcp_safe(&self) -> bool {

        true
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match files, e.g. '**/*.rs', 'src/**/mod.rs'"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'pattern' parameter"))?;

        if self.security.is_rate_limited() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: too many actions in the last hour".into()),
            });
        }

        if self.security.is_command_policy_enabled() {
            if (pattern.starts_with('/') || pattern.starts_with('\\'))
                && !self.security.is_under_allowed_root(pattern)
            {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(
                        "Absolute paths are not allowed. Use a relative glob pattern.".into(),
                    ),
                });
            }

            if pattern.contains("../") || pattern.contains("..\\") || pattern == ".." {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("Path traversal ('..') is not allowed in glob patterns.".into()),
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

        let full_pattern = self
            .security
            .resolve_tool_path(pattern)
            .to_string_lossy()
            .to_string();

        enum WalkOutcome {
            Ok { results: Vec<String>, truncated: bool },
            InvalidPattern(String),
            BadWorkspace(String),
        }

        let security_arc = Arc::clone(&self.security);
        let workspace = self.security.workspace_dir().to_path_buf();
        let walk = tokio::task::spawn_blocking(move || -> WalkOutcome {
            let entries = match glob::glob(&full_pattern) {
                Ok(paths) => paths,
                Err(e) => return WalkOutcome::InvalidPattern(e.to_string()),
            };
            let workspace_canon = match std::fs::canonicalize(&workspace) {
                Ok(p) => p,
                Err(e) => return WalkOutcome::BadWorkspace(e.to_string()),
            };
            let mut results = Vec::new();
            let mut truncated = false;
            for entry in entries {
                let path = match entry {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                let resolved = match std::fs::canonicalize(&path) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                if !security_arc.is_resolved_path_allowed(&resolved) {
                    continue;
                }
                if resolved.is_dir() {
                    continue;
                }
                if let Ok(rel) = resolved.strip_prefix(&workspace_canon) {
                    results.push(rel.to_string_lossy().to_string());
                }
                if results.len() >= MAX_RESULTS {
                    truncated = true;
                    break;
                }
            }
            results.sort();
            WalkOutcome::Ok { results, truncated }
        })
        .await
        .map_err(|e| anyhow::anyhow!("glob_search join error: {e}"))?;

        let (mut results, truncated) = match walk {
            WalkOutcome::Ok { results, truncated } => (results, truncated),
            WalkOutcome::InvalidPattern(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Invalid glob pattern: {e}")),
                });
            }
            WalkOutcome::BadWorkspace(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Cannot resolve workspace directory: {e}")),
                });
            }
        };
        let _ = &mut results;

        let output = if results.is_empty() {
            format!("No files matching pattern '{pattern}' found in workspace.")
        } else {
            use std::fmt::Write;

            let rendered = if crate::token_saver::is_enabled() {
                let ctx = crate::token_saver::global();
                if matches!(ctx.level, crate::token_saver::CompactLevel::Conservative) {
                    results.join("\n")
                } else {
                    let entries: Vec<crate::token_saver::DirEntry> = results
                        .iter()
                        .map(|p| crate::token_saver::DirEntry {
                            name: p.clone(),
                            is_dir: false,
                            is_hidden: false,
                            size: 0,
                        })
                        .collect();
                    let opts = crate::token_saver::ListOpts {
                        level: ctx.level,
                        group_by_ext: true,
                    };
                    let mut compacted =
                        crate::token_saver::compact_dir_listing(&entries, &opts);
                    if compacted.ends_with('\n') {
                        compacted.pop();
                    }
                    compacted
                }
            } else {
                results.join("\n")
            };
            let mut buf = rendered;
            if truncated {
                let _ = write!(
                    buf,
                    "\n\n[Results truncated: showing first {MAX_RESULTS} of more matches]"
                );
            }
            let _ = write!(buf, "\n\nTotal: {} files", results.len());
            buf
        };

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}
