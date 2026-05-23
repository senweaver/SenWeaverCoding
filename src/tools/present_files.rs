// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::fmt::Write;
use std::sync::Arc;

const MAX_INLINE_SIZE: u64 = 50_000;

pub struct PresentFilesTool {
    security: Arc<SecurityPolicy>,
}

impl PresentFilesTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for PresentFilesTool {
    fn name(&self) -> &str {
        "present_files"
    }

    fn description(&self) -> &str {
        "Present one or more output files to the user. Use this when you have generated, \
         modified, or downloaded files that the user should see. Provides file metadata \
         and contents for text files."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "files": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Path to the file"
                            },
                            "description": {
                                "type": "string",
                                "description": "Brief description of the file content/purpose"
                            }
                        },
                        "required": ["path"]
                    },
                    "description": "List of files to present"
                }
            },
            "required": ["files"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let files = args
            .get("files")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing 'files' parameter"))?;

        if files.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("No files specified".into()),
            });
        }

        if files.len() > 100 {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Too many files specified (maximum 100)".into()),
            });
        }

        let mut output = String::new();
        let mut success_count = 0u32;
        let mut error_count = 0u32;

        for item in files {
            let path_str = match item.get("path").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => {
                    error_count += 1;
                    writeln!(output, "--- Error: missing path in file entry ---").ok();
                    continue;
                }
            };

            let description = item
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if !self.security.is_path_allowed(path_str) {
                error_count += 1;
                writeln!(
                    output,
                    "--- {path_str}: Access denied by security policy ---"
                )
                .ok();
                continue;
            }

            let full_path = self.security.resolve_tool_path(path_str);
            let full_path = match full_path.canonicalize() {
                Ok(p) => p,
                Err(_) => full_path,
            };
            if !self.security.is_resolved_path_allowed(&full_path) {
                error_count += 1;
                writeln!(
                    output,
                    "--- {path_str}: Resolved path blocked by security policy ---"
                )
                .ok();
                continue;
            }

            if !full_path.exists() {
                error_count += 1;
                writeln!(output, "--- {path_str}: File not found ---").ok();
                continue;
            }

            let metadata = match tokio::fs::metadata(&full_path).await {
                Ok(m) => m,
                Err(e) => {
                    error_count += 1;
                    writeln!(output, "--- {path_str}: Cannot read metadata: {e} ---").ok();
                    continue;
                }
            };

            success_count += 1;

            let size = metadata.len();
            let modified = metadata
                .modified()
                .ok()
                .map(|t| {
                    let dt: chrono::DateTime<chrono::Utc> = t.into();
                    dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
                })
                .unwrap_or_else(|| "unknown".into());

            let ext = full_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            let file_type = match ext.as_str() {
                "rs" | "py" | "js" | "ts" | "go" | "java" | "c" | "cpp" | "h" | "cs" => {
                    "source code"
                }
                "md" | "txt" | "log" | "csv" | "toml" | "yaml" | "yml" | "json" | "xml" => "text",
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" => "image",
                "pdf" => "pdf",
                "zip" | "tar" | "gz" | "7z" | "rar" => "archive",
                "html" | "css" => "web",
                _ => "file",
            };

            writeln!(output, "=== File: {path_str} ===").ok();
            if !description.is_empty() {
                writeln!(output, "Description: {description}").ok();
            }
            writeln!(
                output,
                "Type: {file_type} | Size: {} | Modified: {modified}",
                format_size(size)
            )
            .ok();

            let is_text = matches!(file_type, "source code" | "text" | "web");
            if is_text && size <= MAX_INLINE_SIZE {
                match tokio::fs::read_to_string(&full_path).await {
                    Ok(content) => {
                        let lang = match ext.as_str() {
                            "rs" => "rust",
                            "py" => "python",
                            "js" => "javascript",
                            "ts" => "typescript",
                            "go" => "go",
                            "java" => "java",
                            "c" | "cpp" | "h" => "c",
                            "cs" => "csharp",
                            "md" => "markdown",
                            "json" => "json",
                            "yaml" | "yml" => "yaml",
                            "toml" => "toml",
                            "xml" => "xml",
                            "html" => "html",
                            "css" => "css",
                            _ => "",
                        };
                        writeln!(output, "```{lang}").ok();
                        output.push_str(&content);
                        if !content.ends_with('\n') {
                            output.push('\n');
                        }
                        writeln!(output, "```").ok();
                    }
                    Err(_) => {
                        writeln!(output, "(binary or non-UTF8 content, not displayed inline)").ok();
                    }
                }
            } else if is_text {
                writeln!(
                    output,
                    "(file too large for inline display: {})",
                    format_size(size)
                )
                .ok();
            } else if file_type == "image" {
                writeln!(output, "(image file — use view_image tool to inspect)").ok();
            }

            writeln!(output).ok();
        }

        writeln!(
            output,
            "Presented {success_count} file(s), {error_count} error(s)."
        )
        .ok();

        Ok(ToolResult {
            success: error_count == 0,
            output,
            error: if error_count > 0 {
                Some(format!("{error_count} file(s) had errors"))
            } else {
                None
            },
        })
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
