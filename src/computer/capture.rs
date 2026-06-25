// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use std::io::Cursor;

const MAX_TRANSPORT_DIM: u32 = 1536;

#[derive(Debug, Clone)]
pub struct CapturedScreen {
    pub width: u32,
    pub height: u32,
    pub png_base64: String,
}

impl CapturedScreen {
    pub fn data_uri(&self) -> String {
        format!("data:image/png;base64,{}", self.png_base64)
    }
}

pub async fn capture_primary() -> Result<CapturedScreen> {
    tokio::task::spawn_blocking(capture_primary_blocking)
        .await
        .map_err(|e| anyhow!("screen capture task failed to join: {e}"))?
}

fn capture_primary_blocking() -> Result<CapturedScreen> {
    let monitors = xcap::Monitor::all().map_err(|e| anyhow!("enumerate monitors failed: {e}"))?;
    if monitors.is_empty() {
        return Err(anyhow!("no monitors available for capture"));
    }

    let mut primary: Option<xcap::Monitor> = None;
    for monitor in monitors.iter() {
        if monitor.is_primary().unwrap_or(false) {
            primary = Some(monitor.clone());
            break;
        }
    }
    let monitor = primary.unwrap_or_else(|| monitors[0].clone());

    let image = monitor
        .capture_image()
        .map_err(|e| anyhow!("capture image failed: {e}"))?;

    let width = image.width();
    let height = image.height();
    let raw = image.into_raw();

    let buffer: image::RgbaImage = image::RgbaImage::from_raw(width, height, raw)
        .ok_or_else(|| anyhow!("captured frame had unexpected buffer size"))?;

    let longest = width.max(height);
    let transport = if longest > MAX_TRANSPORT_DIM {
        let factor = f64::from(MAX_TRANSPORT_DIM) / f64::from(longest);
        let new_width = ((f64::from(width) * factor).round() as u32).max(1);
        let new_height = ((f64::from(height) * factor).round() as u32).max(1);
        image::imageops::resize(
            &buffer,
            new_width,
            new_height,
            image::imageops::FilterType::Triangle,
        )
    } else {
        buffer
    };

    let mut png_bytes: Vec<u8> = Vec::new();
    image::DynamicImage::ImageRgba8(transport)
        .write_to(&mut Cursor::new(&mut png_bytes), image::ImageFormat::Png)
        .context("encode screenshot to PNG")?;

    let png_base64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);

    Ok(CapturedScreen {
        width,
        height,
        png_base64,
    })
}
