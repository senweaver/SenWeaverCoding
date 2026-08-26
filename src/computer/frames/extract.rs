// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{anyhow, Result};
use std::path::Path;

use super::log::{load_manifest, FrameRecord};

pub const MAX_WINDOW_FRAMES: usize = 24;
pub const MAX_TOOL_IMAGES: usize = 6;
const CROP_JPEG_QUALITY: u8 = 88;

#[derive(Debug, Clone, Copy, serde::Deserialize)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

pub struct ExtractedFrame {
    pub record: FrameRecord,
    pub jpeg: Vec<u8>,
}

pub fn list_frames(dir: &Path) -> Vec<FrameRecord> {
    load_manifest(dir).map(|m| m.frames).unwrap_or_default()
}

pub fn extract_window(
    dir: &Path,
    from_offset_ms: u64,
    to_offset_ms: u64,
    fps: f64,
    max_frames: usize,
    crop: Option<CropRect>,
) -> Result<Vec<ExtractedFrame>> {
    let frames = list_frames(dir);
    if frames.is_empty() {
        return Ok(Vec::new());
    }
    let to_offset_ms = to_offset_ms.max(from_offset_ms);
    let max_frames = max_frames.clamp(1, MAX_WINDOW_FRAMES);
    let fps = if fps.is_finite() && fps > 0.0 {
        fps.min(1.0)
    } else {
        1.0
    };
    let step_ms = ((1000.0 / fps).round() as u64).max(200);

    let mut selected: Vec<&FrameRecord> = Vec::new();
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    let mut cursor = from_offset_ms;
    while selected.len() < max_frames {
        if let Some(frame) = frames.iter().min_by_key(|f| f.offset_ms.abs_diff(cursor)) {
            if frame.offset_ms.abs_diff(cursor) <= step_ms && seen.insert(frame.file.as_str()) {
                selected.push(frame);
            }
        }
        if cursor >= to_offset_ms {
            break;
        }
        cursor = cursor.saturating_add(step_ms).min(to_offset_ms);
    }
    selected.sort_by_key(|f| f.offset_ms);

    let mut out = Vec::with_capacity(selected.len());
    for record in selected {
        let path = dir.join("frames").join(&record.file);
        let bytes = std::fs::read(&path)
            .map_err(|e| anyhow!("failed to read frame {}: {e}", record.file))?;
        let jpeg = match crop {
            Some(rect) => crop_jpeg(&bytes, rect)?,
            None => bytes,
        };
        out.push(ExtractedFrame {
            record: record.clone(),
            jpeg,
        });
    }
    Ok(out)
}

fn crop_jpeg(bytes: &[u8], rect: CropRect) -> Result<Vec<u8>> {
    let img = image::load_from_memory(bytes).map_err(|e| anyhow!("decode frame failed: {e}"))?;
    let (width, height) = (img.width(), img.height());
    if rect.w == 0 || rect.h == 0 || rect.x >= width || rect.y >= height {
        return Err(anyhow!(
            "crop {{x:{},y:{},w:{},h:{}}} is outside the {width}x{height} frame",
            rect.x,
            rect.y,
            rect.w,
            rect.h
        ));
    }
    let w = rect.w.min(width - rect.x);
    let h = rect.h.min(height - rect.y);
    let cropped = img.crop_imm(rect.x, rect.y, w, h).to_rgb8();
    let mut out = Vec::new();
    let mut encoder =
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, CROP_JPEG_QUALITY);
    encoder
        .encode_image(&cropped)
        .map_err(|e| anyhow!("encode cropped frame failed: {e}"))?;
    Ok(out)
}
