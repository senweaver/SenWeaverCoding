// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Image viewing tool for vision-capable models.
//!
//! Reads an image file, validates it, and returns base64-encoded data
//! along with metadata so vision-capable LLMs can actually "see" images.
//! Mirrors DeerFlow's `view_image` tool.

use super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

const MAX_IMAGE_BYTES: u64 = 10_485_760;

const SUPPORTED_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp", "bmp", "svg"];

pub struct ViewImageTool {
    security: Arc<SecurityPolicy>,
}

impl ViewImageTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }

    fn detect_mime(ext: &str) -> &'static str {
        match ext {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "bmp" => "image/bmp",
            "svg" => "image/svg+xml",
            _ => "application/octet-stream",
        }
    }

    fn detect_dimensions(bytes: &[u8], ext: &str) -> Option<(u32, u32)> {
        match ext {
            "png" => {
                if bytes.len() >= 24 && bytes.starts_with(b"\x89PNG") {
                    let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
                    let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
                    Some((w, h))
                } else {
                    None
                }
            }
            "gif" => {
                if bytes.len() >= 10 && bytes.starts_with(b"GIF8") {
                    let w = u16::from_le_bytes([bytes[6], bytes[7]]) as u32;
                    let h = u16::from_le_bytes([bytes[8], bytes[9]]) as u32;
                    Some((w, h))
                } else {
                    None
                }
            }
            "bmp" => {
                if bytes.len() >= 26 && bytes.starts_with(b"BM") {
                    let w = u32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]);
                    let h = u32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]);
                    Some((w, h))
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

#[async_trait]
impl Tool for ViewImageTool {
    fn name(&self) -> &str {
        "view_image"
    }

    fn description(&self) -> &str {
        "View an image file by returning its base64-encoded content and metadata. \
         Use this tool when you need to examine, analyze, or describe the contents \
         of an image file. Returns base64 data suitable for vision model processing."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the image file"
                },
                "include_base64": {
                    "type": "boolean",
                    "description": "Include base64-encoded image data (default true). Set false for metadata only."
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

        let include_base64 = args
            .get("include_base64")
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

        if !full_path.exists() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("File not found: {path_str}")),
            });
        }

        let ext = full_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        if !SUPPORTED_EXTENSIONS.contains(&ext.as_str()) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Unsupported image format: .{ext}. Supported: {}",
                    SUPPORTED_EXTENSIONS.join(", ")
                )),
            });
        }

        let metadata = match tokio::fs::metadata(&full_path).await {
            Ok(m) => m,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Cannot read file metadata: {e}")),
                });
            }
        };

        if metadata.len() > MAX_IMAGE_BYTES {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Image too large: {} bytes (max {} MB)",
                    metadata.len(),
                    MAX_IMAGE_BYTES / 1_048_576
                )),
            });
        }

        let bytes = match tokio::fs::read(&full_path).await {
            Ok(b) => b,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to read image: {e}")),
                });
            }
        };

        let mime = Self::detect_mime(&ext);
        let dimensions = Self::detect_dimensions(&bytes, &ext);

        let mut result = json!({
            "path": path_str,
            "format": ext,
            "mime_type": mime,
            "size_bytes": bytes.len(),
        });

        if let Some((w, h)) = dimensions {
            result["width"] = json!(w);
            result["height"] = json!(h);
        }

        if include_base64 {
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            result["base64"] = json!(encoded);
            result["data_uri"] = json!(format!(
                "data:{mime};base64,{}",
                &encoded[..64.min(encoded.len())]
            ));
            result["data_uri_note"] =
                json!("data_uri is truncated; use base64 field for full data");
        }

        let output = serde_json::to_string_pretty(&result).unwrap_or_default();

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}
