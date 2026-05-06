// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

const MAX_FILE_SIZE_BYTES: u64 = 10 * 1024 * 1024;

pub struct FileReadTool {
    security: Arc<SecurityPolicy>,
}

impl FileReadTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        "file_read"
    }

    fn description(&self) -> &str {
        "Read file contents with line numbers. Supports partial reading via offset and limit. Extracts text from PDF; other binary files are read with lossy UTF-8 conversion."
    }

    fn mcp_safe(&self) -> bool {

        true
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file. Relative paths resolve from workspace; outside paths require policy allowlist."
                },
                "offset": {
                    "type": "integer",
                    "description": "Starting line number (1-based, default: 1)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to return (default: all)"
                },
                "level": {
                    "type": "string",
                    "enum": ["default", "smart", "signatures"],
                    "description": "Token-saver compaction level for the file body. 'default' (the default) returns the file as-is with line numbers; 'smart' keeps the first 80 + last 40 lines for files longer than that with an elision marker; 'signatures' returns only function/struct/class declarations for supported source files (.rs/.ts/.py/.go/.java/.c/.cpp/.js)."
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' parameter"))?;

        if self.security.is_rate_limited() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: too many actions in the last hour".into()),
            });
        }

        if !self.security.is_path_allowed(path) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Path not allowed by security policy: {path}")),
            });
        }

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: action budget exhausted".into()),
            });
        }

        let full_path = self.security.resolve_tool_path(path);

        let resolved_path = match tokio::fs::canonicalize(&full_path).await {
            Ok(p) => p,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to resolve file path: {e}")),
                });
            }
        };

        if !self.security.is_resolved_path_allowed(&resolved_path) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    self.security
                        .resolved_path_violation_message(&resolved_path),
                ),
            });
        }

        match tokio::fs::metadata(&resolved_path).await {
            Ok(meta) => {
                if meta.len() > MAX_FILE_SIZE_BYTES {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "File too large: {} bytes (limit: {MAX_FILE_SIZE_BYTES} bytes)",
                            meta.len()
                        )),
                    });
                }
            }
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to read file metadata: {e}")),
                });
            }
        }

        let explicit_level = args.get("level").and_then(|v| v.as_str()).is_some();
        let mut level = args
            .get("level")
            .and_then(|v| v.as_str())
            .map(crate::token_saver::ReadLevel::parse)
            .unwrap_or(crate::token_saver::ReadLevel::Default);

        let has_offset = args.get("offset").and_then(|v| v.as_u64()).is_some();
        let has_limit = args.get("limit").and_then(|v| v.as_u64()).is_some();

        match tokio::fs::read_to_string(&resolved_path).await {
            Ok(mut contents) => {

                const AUTO_SMART_LINE_THRESHOLD: usize = 1500;
                const AUTO_SMART_BYTE_THRESHOLD: usize = 128 * 1024;
                if !explicit_level
                    && !has_offset
                    && !has_limit
                    && crate::token_saver::is_enabled()
                    && level == crate::token_saver::ReadLevel::Default
                    && (contents.len() >= AUTO_SMART_BYTE_THRESHOLD
                        || contents.lines().count() >= AUTO_SMART_LINE_THRESHOLD)
                {
                    level = crate::token_saver::ReadLevel::Smart;
                }

                if level != crate::token_saver::ReadLevel::Default
                    && crate::token_saver::is_enabled()
                {
                    contents = crate::token_saver::compact_file_content(path, &contents, level);
                    return Ok(ToolResult {
                        success: true,
                        output: contents,
                        error: None,
                    });
                }
                let lines: Vec<&str> = contents.lines().collect();
                let total = lines.len();

                if total == 0 {
                    return Ok(ToolResult {
                        success: true,
                        output: String::new(),
                        error: None,
                    });
                }

                let offset = args
                    .get("offset")
                    .and_then(|v| v.as_u64())
                    .map(|v| {
                        usize::try_from(v.max(1))
                            .unwrap_or(usize::MAX)
                            .saturating_sub(1)
                    })
                    .unwrap_or(0);
                let start = offset.min(total);

                let end = match args.get("limit").and_then(|v| v.as_u64()) {
                    Some(l) => {
                        let limit = usize::try_from(l).unwrap_or(usize::MAX);
                        (start.saturating_add(limit)).min(total)
                    }
                    None => total,
                };

                if start >= end {
                    return Ok(ToolResult {
                        success: true,
                        output: format!("[No lines in range, file has {total} lines]"),
                        error: None,
                    });
                }

                let numbered: String = lines[start..end]
                    .iter()
                    .enumerate()
                    .map(|(i, line)| format!("{}: {}", start + i + 1, line))
                    .collect::<Vec<_>>()
                    .join("\n");

                let partial = start > 0 || end < total;
                let summary = if partial {
                    format!("\n[Lines {}-{} of {total}]", start + 1, end)
                } else {
                    format!("\n[{total} lines total]")
                };

                Ok(ToolResult {
                    success: true,
                    output: format!("{numbered}{summary}"),
                    error: None,
                })
            }
            Err(_) => {

                let bytes = tokio::fs::read(&resolved_path)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to read file: {e}"))?;

                if let Some(text) = try_extract_pdf_text(&bytes) {
                    return Ok(ToolResult {
                        success: true,
                        output: text,
                        error: None,
                    });
                }

                let lossy = String::from_utf8_lossy(&bytes).into_owned();
                Ok(ToolResult {
                    success: true,
                    output: lossy,
                    error: None,
                })
            }
        }
    }
}

#[cfg(feature = "rag-pdf")]
fn try_extract_pdf_text(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 5 || &bytes[..5] != b"%PDF-" {
        return None;
    }
    let text = pdf_extract::extract_text_from_mem(bytes).ok()?;
    if text.trim().is_empty() {
        return None;
    }
    Some(text)
}

#[cfg(not(feature = "rag-pdf"))]
fn try_extract_pdf_text(_bytes: &[u8]) -> Option<String> {
    None
}
