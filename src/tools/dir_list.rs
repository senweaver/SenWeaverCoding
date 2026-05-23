// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::fmt::Write;
use std::path::Path;
use std::sync::Arc;

const MAX_DEPTH: usize = 5;
const MAX_ENTRIES: usize = 500;

pub struct DirListTool {
    security: Arc<SecurityPolicy>,
}

impl DirListTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for DirListTool {
    fn name(&self) -> &str {
        "dir_list"
    }

    fn description(&self) -> &str {
        "List directory contents with tree-like output. Shows files and subdirectories \
         with optional size and modification time. Use depth parameter to control recursion."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path to list. Relative paths resolve from workspace."
                },
                "depth": {
                    "type": "integer",
                    "description": "Maximum recursion depth (default 1, max 5). 1 = immediate children only."
                },
                "show_hidden": {
                    "type": "boolean",
                    "description": "Include hidden files/directories (default false)"
                },
                "show_size": {
                    "type": "boolean",
                    "description": "Show file sizes (default true)"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let path_str = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' parameter"))?;

        let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
        let depth = depth.min(MAX_DEPTH);

        let show_hidden = args
            .get("show_hidden")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let show_size = args
            .get("show_size")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        if !self.security.is_path_allowed(path_str) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Path not allowed by security policy: {path_str}")),
            });
        }

        let full_path = self.security.resolve_tool_path(path_str);
        let full_path = match full_path.canonicalize() {
            Ok(p) => p,
            Err(_) => full_path,
        };
        if !self.security.is_resolved_path_allowed(&full_path) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Resolved path not allowed by security policy: {path_str}"
                )),
            });
        }

        if !full_path.exists() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Directory not found: {path_str}")),
            });
        }

        if !full_path.is_dir() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Not a directory: {path_str}")),
            });
        }

        if depth == 1 && crate::token_saver::is_enabled() {
            let ctx = crate::token_saver::global();
            if !matches!(ctx.level, crate::token_saver::CompactLevel::Conservative) {
                if let Some(flat) = build_flat_listing(&full_path, show_hidden).await {
                    let opts = crate::token_saver::ListOpts {
                        level: ctx.level,
                        group_by_ext: true,
                    };
                    let mut output = String::new();
                    writeln!(output, "{}/", path_str).ok();
                    output.push_str(&crate::token_saver::compact_dir_listing(&flat, &opts));
                    let dir_count = flat.iter().filter(|e| e.is_dir).count();
                    let file_count = flat.iter().filter(|e| !e.is_dir && !e.is_hidden).count();
                    writeln!(output, "\n{} directories, {} files", dir_count, file_count).ok();
                    if flat.len() >= MAX_ENTRIES {
                        writeln!(
                            output,
                            "(output truncated at {} entries, use a deeper path to narrow)",
                            MAX_ENTRIES
                        )
                        .ok();
                    }
                    return Ok(ToolResult {
                        success: true,
                        output,
                        error: None,
                    });
                }
            }
        }

        let mut output = String::new();
        let mut entry_count = 0u32;
        let mut file_count = 0u32;
        let mut dir_count = 0u32;

        writeln!(output, "{}/", path_str).ok();
        list_dir_recursive(
            &full_path,
            "",
            depth,
            show_hidden,
            show_size,
            &mut output,
            &mut entry_count,
            &mut file_count,
            &mut dir_count,
            &self.security,
        )
        .await;

        writeln!(output, "\n{} directories, {} files", dir_count, file_count).ok();

        if entry_count >= MAX_ENTRIES as u32 {
            writeln!(
                output,
                "(output truncated at {} entries, use depth=1 for large directories)",
                MAX_ENTRIES
            )
            .ok();
        }

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}

async fn build_flat_listing(
    dir: &Path,
    show_hidden: bool,
) -> Option<Vec<crate::token_saver::DirEntry>> {
    let mut read_dir = tokio::fs::read_dir(dir).await.ok()?;
    let mut entries: Vec<crate::token_saver::DirEntry> = Vec::new();
    while let Ok(Some(entry)) = read_dir.next_entry().await {
        if entries.len() >= MAX_ENTRIES {
            break;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let is_hidden = name.starts_with('.');
        if !show_hidden && is_hidden {
            continue;
        }
        let is_dir = entry
            .file_type()
            .await
            .map(|ft| ft.is_dir())
            .unwrap_or(false);
        let size = if !is_dir {
            entry.metadata().await.map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };
        entries.push(crate::token_saver::DirEntry {
            name,
            is_dir,
            is_hidden,
            size,
        });
    }
    Some(entries)
}

#[async_trait::async_trait]
trait DirListAsync {}

async fn list_dir_recursive(
    dir: &Path,
    prefix: &str,
    remaining_depth: usize,
    show_hidden: bool,
    show_size: bool,
    output: &mut String,
    entry_count: &mut u32,
    file_count: &mut u32,
    dir_count: &mut u32,
    security: &SecurityPolicy,
) {
    if remaining_depth == 0 || *entry_count >= MAX_ENTRIES as u32 {
        return;
    }

    let mut entries: Vec<(String, bool, u64)> = Vec::new();

    let mut read_dir = match tokio::fs::read_dir(dir).await {
        Ok(rd) => rd,
        Err(_) => return,
    };

    while let Ok(Some(entry)) = read_dir.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();

        if !show_hidden && name.starts_with('.') {
            continue;
        }

        let entry_path = entry.path();
        if let Ok(ft) = tokio::fs::symlink_metadata(&entry_path).await {
            if ft.file_type().is_symlink() {
                if let Ok(resolved) = tokio::fs::canonicalize(&entry_path).await {
                    if !security.is_resolved_path_allowed(&resolved) {
                        continue;
                    }
                } else {
                    continue;
                }
            }
        }

        let is_dir = entry
            .file_type()
            .await
            .map(|ft| ft.is_dir())
            .unwrap_or(false);

        let size = if !is_dir {
            entry.metadata().await.map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };

        entries.push((name, is_dir, size));
    }

    entries.sort_by(|a, b| match (a.1, b.1) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.0.to_lowercase().cmp(&b.0.to_lowercase()),
    });

    let total = entries.len();
    for (i, (name, is_dir, size)) in entries.into_iter().enumerate() {
        if *entry_count >= MAX_ENTRIES as u32 {
            break;
        }

        let is_last = i == total - 1;
        let connector = if is_last { "└── " } else { "├── " };
        let child_prefix = if is_last {
            format!("{prefix}    ")
        } else {
            format!("{prefix}│   ")
        };

        *entry_count += 1;

        if is_dir {
            *dir_count += 1;
            writeln!(output, "{prefix}{connector}{name}/").ok();

            let sub_path = dir.join(&name);
            Box::pin(list_dir_recursive(
                &sub_path,
                &child_prefix,
                remaining_depth - 1,
                show_hidden,
                show_size,
                output,
                entry_count,
                file_count,
                dir_count,
                security,
            ))
            .await;
        } else {
            *file_count += 1;
            if show_size {
                writeln!(output, "{prefix}{connector}{name} ({})", format_size(size)).ok();
            } else {
                writeln!(output, "{prefix}{connector}{name}").ok();
            }
        }
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
