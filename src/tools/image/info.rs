// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::fmt::Write;
use std::sync::Arc;

const MAX_IMAGE_BYTES: u64 = 5_242_880;

pub struct ImageInfoTool {
    security: Arc<SecurityPolicy>,
}

impl ImageInfoTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }

    fn detect_format(bytes: &[u8]) -> &'static str {
        if bytes.len() < 4 {
            return "unknown";
        }
        if bytes.starts_with(b"\x89PNG") {
            "png"
        } else if bytes.starts_with(b"\xFF\xD8\xFF") {
            "jpeg"
        } else if bytes.starts_with(b"GIF8") {
            "gif"
        } else if bytes.starts_with(b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == b"WEBP" {
            "webp"
        } else if bytes.starts_with(b"BM") {
            "bmp"
        } else {
            "unknown"
        }
    }

    fn extract_dimensions(bytes: &[u8], format: &str) -> Option<(u32, u32)> {
        match format {
            "png" => {

                if bytes.len() >= 24 {
                    let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
                    let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
                    Some((w, h))
                } else {
                    None
                }
            }
            "gif" => {

                if bytes.len() >= 10 {
                    let w = u32::from(u16::from_le_bytes([bytes[6], bytes[7]]));
                    let h = u32::from(u16::from_le_bytes([bytes[8], bytes[9]]));
                    Some((w, h))
                } else {
                    None
                }
            }
            "bmp" => {

                if bytes.len() >= 26 {
                    let w = u32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]);
                    let h_raw = i32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]);
                    let h = h_raw.unsigned_abs();
                    Some((w, h))
                } else {
                    None
                }
            }
            "jpeg" => Self::jpeg_dimensions(bytes),
            _ => None,
        }
    }

    fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
        let mut i = 2;
        while i + 1 < bytes.len() {
            if bytes[i] != 0xFF {
                return None;
            }
            let marker = bytes[i + 1];
            i += 2;

            if (0xC0..=0xC3).contains(&marker) {
                if i + 7 <= bytes.len() {
                    let h = u32::from(u16::from_be_bytes([bytes[i + 3], bytes[i + 4]]));
                    let w = u32::from(u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]));
                    return Some((w, h));
                }
                return None;
            }

            if i + 1 < bytes.len() {
                let seg_len = u16::from_be_bytes([bytes[i], bytes[i + 1]]) as usize;
                if seg_len < 2 {
                    return None;
                }
                i += seg_len;
            } else {
                return None;
            }
        }
        None
    }
}

#[async_trait]
impl Tool for ImageInfoTool {
    fn name(&self) -> &str {
        "image_info"
    }

    fn description(&self) -> &str {
        "Read image file metadata (format, dimensions, size) and optionally return base64-encoded data."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the image file (absolute or relative to workspace)"
                },
                "include_base64": {
                    "type": "boolean",
                    "description": "Include base64-encoded image data in output (default: false)"
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
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        if !self.security.is_path_allowed(path_str) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Path not allowed: {path_str} (must be within workspace)"
                )),
            });
        }

        let full_path = self.security.resolve_tool_path(path_str);
        let path = full_path.as_path();

        if !path.exists() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("File not found: {path_str}")),
            });
        }

        let metadata = tokio::fs::metadata(path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read file metadata: {e}"))?;

        let file_size = metadata.len();

        if file_size > MAX_IMAGE_BYTES {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Image too large: {file_size} bytes (max {MAX_IMAGE_BYTES} bytes)"
                )),
            });
        }

        let bytes = tokio::fs::read(path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read image file: {e}"))?;

        let format = Self::detect_format(&bytes);
        let dimensions = Self::extract_dimensions(&bytes, format);

        let mut output = format!("File: {path_str}\nFormat: {format}\nSize: {file_size} bytes");

        if let Some((w, h)) = dimensions {
            let _ = write!(output, "\nDimensions: {w}x{h}");
        }

        if include_base64 {
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            let mime = match format {
                "png" => "image/png",
                "jpeg" => "image/jpeg",
                "gif" => "image/gif",
                "webp" => "image/webp",
                "bmp" => "image/bmp",
                _ => "application/octet-stream",
            };
            let _ = write!(output, "\ndata:{mime};base64,{encoded}");
        }

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}
