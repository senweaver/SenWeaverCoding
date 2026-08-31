// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

const MAX_RESULTS: usize = 1000;
const HARD_COLLECT_CAP: usize = 50_000;

use super::{GLOB_WALK_TIMEOUT_SECS as WALK_TIMEOUT_SECS, crosses_skip_dir};

pub struct GlobSearchTool {
    security: Arc<SecurityPolicy>,
}

enum WalkOutcome {
    Ok {
        results: Vec<String>,
        truncated: bool,
        timed_out: bool,
    },
    InvalidPattern(String),
    BadWorkspace(String),
}

fn legacy_absolute_glob(
    security: &SecurityPolicy,
    full_pattern: &str,
    workspace: &std::path::Path,
) -> WalkOutcome {
    let entries = match glob::glob(full_pattern) {
        Ok(paths) => paths,
        Err(e) => return WalkOutcome::InvalidPattern(e.to_string()),
    };
    let workspace_canon = match std::fs::canonicalize(workspace) {
        Ok(p) => p,
        Err(e) => return WalkOutcome::BadWorkspace(e.to_string()),
    };
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_secs(WALK_TIMEOUT_SECS);
    let mut scored: Vec<(String, std::time::SystemTime)> = Vec::new();
    let mut truncated = false;
    let mut timed_out = false;
    for entry in entries {
        if std::time::Instant::now() >= deadline {
            timed_out = true;
            break;
        }
        let path = match entry {
            Ok(p) => p,
            Err(_) => continue,
        };
        if crosses_skip_dir(&path) {
            continue;
        }
        let resolved = match std::fs::canonicalize(&path) {
            Ok(p) => p,
            Err(_) => continue,
        };
        if !security.is_resolved_path_allowed(&resolved) {
            continue;
        }
        let meta = std::fs::metadata(&resolved).ok();
        if meta.as_ref().map(|m| m.is_dir()).unwrap_or(false) {
            continue;
        }
        let mtime = meta
            .and_then(|m| m.modified().ok())
            .unwrap_or(std::time::UNIX_EPOCH);
        let rendered = match resolved.strip_prefix(&workspace_canon) {
            Ok(rel) => rel.to_string_lossy().to_string(),
            Err(_) => resolved.to_string_lossy().to_string(),
        };
        scored.push((rendered, mtime));
        if scored.len() >= HARD_COLLECT_CAP {
            truncated = true;
            break;
        }
    }
    scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    if scored.len() > MAX_RESULTS {
        truncated = true;
        scored.truncate(MAX_RESULTS);
    }
    let results = scored.into_iter().map(|(rel, _)| rel).collect::<Vec<_>>();
    WalkOutcome::Ok {
        results,
        truncated,
        timed_out,
    }
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
         Respects .gitignore and skips heavy build/vendor directories. \
         Returns matching file paths relative to the workspace root, ordered by \
         most-recently-modified first (ties broken alphabetically). \
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

        let full_pattern = {
            let expanded = std::path::Path::new(pattern);
            if expanded.is_absolute() || pattern.contains(':') || pattern.starts_with('~') {
                self.security
                    .resolve_tool_path(pattern)
                    .to_string_lossy()
                    .to_string()
            } else {
                format!(
                    "{}/{}",
                    glob::Pattern::escape(
                        &self.security.workspace_dir().display().to_string()
                    ),
                    pattern
                )
            }
        };

        let security_arc = Arc::clone(&self.security);
        let workspace = self.security.workspace_dir().to_path_buf();
        let pattern_owned = pattern.to_string();
        let escapes_workspace = std::path::Path::new(pattern).is_absolute()
            || pattern.contains(':')
            || pattern.starts_with('~');
        let walk = tokio::task::spawn_blocking(move || -> WalkOutcome {
            if escapes_workspace {
                return legacy_absolute_glob(&security_arc, &full_pattern, &workspace);
            }
            let matcher = match globset::GlobBuilder::new(&pattern_owned)
                .literal_separator(true)
                .build()
            {
                Ok(g) => g.compile_matcher(),
                Err(e) => return WalkOutcome::InvalidPattern(e.to_string()),
            };
            let workspace_canon = match std::fs::canonicalize(&workspace) {
                Ok(p) => p,
                Err(e) => return WalkOutcome::BadWorkspace(e.to_string()),
            };
            let deadline = std::time::Instant::now()
                + std::time::Duration::from_secs(WALK_TIMEOUT_SECS);
            let mut scored: Vec<(String, std::time::SystemTime)> = Vec::new();
            let mut truncated = false;
            let mut timed_out = false;
            let walker = ignore::WalkBuilder::new(&workspace_canon)
                .hidden(false)
                .git_global(false)
                .follow_links(false)
                .filter_entry(|entry| {
                    entry.depth() == 0
                        || entry
                            .file_name()
                            .to_str()
                            .map(|name| !super::SKIP_DIRS.contains(&name))
                            .unwrap_or(true)
                })
                .build();
            for entry in walker {
                if std::time::Instant::now() >= deadline {
                    timed_out = true;
                    break;
                }
                if scored.len() >= HARD_COLLECT_CAP {
                    truncated = true;
                    break;
                }
                let Ok(entry) = entry else {
                    continue;
                };
                if !entry.file_type().is_some_and(|t| t.is_file()) {
                    continue;
                }
                let Ok(rel) = entry.path().strip_prefix(&workspace_canon) else {
                    continue;
                };
                if !matcher.is_match(rel) {
                    continue;
                }
                let mtime = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .unwrap_or(std::time::UNIX_EPOCH);
                scored.push((rel.to_string_lossy().to_string(), mtime));
            }
            scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            if scored.len() > MAX_RESULTS {
                truncated = true;
                scored.truncate(MAX_RESULTS);
            }
            let results = scored.into_iter().map(|(rel, _)| rel).collect::<Vec<_>>();
            WalkOutcome::Ok {
                results,
                truncated,
                timed_out,
            }
        })
        .await
        .map_err(|e| anyhow::anyhow!("glob_search join error: {e}"))?;

        let (mut results, truncated, timed_out) = match walk {
            WalkOutcome::Ok {
                results,
                truncated,
                timed_out,
            } => (results, truncated, timed_out),
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
            if timed_out {
                format!(
                    "Search for pattern '{pattern}' timed out after {WALK_TIMEOUT_SECS}s \
                     before finding any match. Narrow the pattern and retry."
                )
            } else {
                format!("No files matching pattern '{pattern}' found in workspace.")
            }
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
            if timed_out {
                let _ = write!(
                    buf,
                    "\n\n[Search stopped after {WALK_TIMEOUT_SECS}s; results may be incomplete. \
                     Narrow the pattern for a full listing.]"
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
