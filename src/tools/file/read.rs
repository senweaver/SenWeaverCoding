// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::super::traits::{Tool, ToolResult};
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
        "Read file contents with line numbers. Supports partial reading via offset and limit. The trailing summary line includes the file's mtime_ms, which can be passed as expected_mtime_ms to file_write/file_edit/multi_edit for conflict detection. Extracts text from office documents (Word .docx, Excel .xlsx, PowerPoint .pptx) and PDF; image files are reported with their metadata (inspect them with view_image), and other binary files return a notice instead of garbled text."
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

        let mtime_ms: Option<u64> = match tokio::fs::metadata(&resolved_path).await {
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
                meta.modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64)
            }
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to read file metadata: {e}")),
                });
            }
        };
        let mtime_suffix = mtime_ms
            .map(|m| format!(", mtime_ms: {m}"))
            .unwrap_or_default();

        {
            let ext = resolved_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let image_mime = match ext.as_str() {
                "png" => Some("image/png"),
                "jpg" | "jpeg" => Some("image/jpeg"),
                "gif" => Some("image/gif"),
                "webp" => Some("image/webp"),
                "bmp" => Some("image/bmp"),
                "ico" => Some("image/x-icon"),
                _ => None,
            };
            if let Some(mime) = image_mime {
                let size = tokio::fs::metadata(&resolved_path)
                    .await
                    .map(|m| m.len())
                    .unwrap_or(0);
                #[cfg(feature = "tool-image")]
                let hint = "Use the view_image tool to inspect its visual contents.";
                #[cfg(not(feature = "tool-image"))]
                let hint = "Binary image content is not rendered as text.";
                crate::session::record_observed_for_current_session(&resolved_path);
                return Ok(ToolResult {
                    success: true,
                    output: format!("[Image file: {path} ({mime}, {size} bytes). {hint}]"),
                    error: None,
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

        let mut contents: String =
            if let Some(kind) = crate::tools::file::office::detect_office_kind_by_ext(path) {
                let bytes = match tokio::fs::read(&resolved_path).await {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("Failed to read file: {e}")),
                        });
                    }
                };
                let extracted = tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
                    match crate::tools::file::office::extract_office_text(kind, &bytes) {
                        Ok(Some(text)) => Ok(text),
                        Ok(None) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
                        Err(e) => Err(e),
                    }
                })
                .await;
                match extracted {
                    Ok(Ok(text)) => text,
                    Ok(Err(e)) => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("Failed to extract document text: {e}")),
                        });
                    }
                    Err(e) => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("Document extraction task failed: {e}")),
                        });
                    }
                }
            } else {
                match tokio::fs::read_to_string(&resolved_path).await {
                    Ok(text) => text,
                    Err(_) => {
                        let bytes = tokio::fs::read(&resolved_path)
                            .await
                            .map_err(|e| anyhow::anyhow!("Failed to read file: {e}"))?;
                        let byte_count = bytes.len();
                        let decoded = tokio::task::spawn_blocking(move || {
                            match crate::tools::file::office::extract_pdf_text_if_pdf(&bytes) {
                                Some(text) => Some(text),
                                None => {
                                    if crate::tools::file::encoding::is_probably_binary(&bytes) {
                                        None
                                    } else {
                                        let (text, _label) =
                                            crate::tools::file::encoding::decode_best_effort(
                                                &bytes,
                                            );
                                        Some(text)
                                    }
                                }
                            }
                        })
                        .await
                        .map_err(|e| anyhow::anyhow!("File decode task failed: {e}"))?;
                        match decoded {
                            Some(text) => text,
                            None => {
                                crate::session::record_observed_for_current_session(
                                    &resolved_path,
                                );
                                return Ok(ToolResult {
                                    success: true,
                                    output: format!(
                                        "[Binary file: {path} ({byte_count} bytes). Its raw \
                                         bytes are not displayable as text; use a tool suited \
                                         to this file type instead.]"
                                    ),
                                    error: None,
                                });
                            }
                        }
                    }
                }
            };

        if contents.starts_with('\u{feff}') {
            contents = contents.trim_start_matches('\u{feff}').to_string();
        }

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
            level = crate::token_saver::ReadLevel::Signatures;
        }

        if level != crate::token_saver::ReadLevel::Default && crate::token_saver::is_enabled() {
            let path_owned = path.to_string();
            let body = std::mem::take(&mut contents);
            let total_lines = body.lines().count();
            let level_for_compact = level;
            let compacted = tokio::task::spawn_blocking(move || {
                crate::token_saver::compact_file_content(&path_owned, &body, level_for_compact)
            })
            .await
            .map_err(|e| anyhow::anyhow!("file compact task failed: {e}"))?;
            let footer = format!(
                "\n[Compacted view ({}) - {total_lines} lines total{mtime_suffix}; \
                 use level=default for full content. Note: a compacted view does NOT satisfy \
                 the read-before-edit requirement; before editing this file, re-read it with \
                 level=default (add offset/limit to page through large files)]",
                level.as_str()
            );
            crate::session::record_observed_for_current_session(&resolved_path);
            return Ok(ToolResult {
                success: true,
                output: format!("{compacted}{footer}"),
                error: None,
            });
        }

        let lines: Vec<&str> = contents.lines().collect();
        let total = lines.len();

        if total == 0 {
            crate::session::record_read_for_current_session(&resolved_path);
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

        const MAX_READ_OUTPUT_BYTES: usize = 384 * 1024;
        const MAX_LINE_OUTPUT_BYTES: usize = 4 * 1024;
        let mut numbered = String::new();
        let mut emitted_bytes = 0usize;
        let mut clipped_end = start;
        let mut truncated_lines = 0usize;
        for (i, line) in lines[start..end].iter().enumerate() {
            let over_long = line.len() > MAX_LINE_OUTPUT_BYTES;
            let entry_len = if over_long {
                MAX_LINE_OUTPUT_BYTES + 64
            } else {
                line.len() + 16
            };
            if i > 0 && emitted_bytes + entry_len > MAX_READ_OUTPUT_BYTES {
                break;
            }
            if i > 0 {
                numbered.push('\n');
            }
            if over_long {
                let cut = crate::util::floor_char_boundary(line, MAX_LINE_OUTPUT_BYTES);
                numbered.push_str(&format!(
                    "{}: {} [line truncated: {} bytes total]",
                    start + i + 1,
                    &line[..cut],
                    line.len()
                ));
                truncated_lines += 1;
            } else {
                numbered.push_str(&format!("{}: {}", start + i + 1, line));
            }
            emitted_bytes += entry_len;
            clipped_end = start + i + 1;
        }
        let byte_clipped = clipped_end < end;
        let end = clipped_end;

        crate::session::record_read_for_current_session(&resolved_path);

        let truncation_note = if truncated_lines > 0 {
            format!(
                "; {truncated_lines} line(s) longer than {} KB were truncated",
                MAX_LINE_OUTPUT_BYTES / 1024
            )
        } else {
            String::new()
        };
        let partial = start > 0 || end < total;
        let summary = if byte_clipped {
            format!(
                "\n[Lines {}-{} of {total}{mtime_suffix}{truncation_note}; output clipped at {} KB - use offset={} with a smaller limit to continue]",
                start + 1,
                end,
                MAX_READ_OUTPUT_BYTES / 1024,
                end + 1
            )
        } else if partial {
            format!(
                "\n[Lines {}-{} of {total}{mtime_suffix}{truncation_note}]",
                start + 1,
                end
            )
        } else {
            format!("\n[{total} lines total{mtime_suffix}{truncation_note}]")
        };

        Ok(ToolResult {
            success: true,
            output: format!("{numbered}{summary}"),
            error: None,
        })
    }
}
