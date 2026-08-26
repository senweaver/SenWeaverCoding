// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Result;

#[derive(Debug, Clone)]
pub struct OcrWord {
    pub text: String,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone)]
pub struct OcrLine {
    pub words: Vec<OcrWord>,
}

#[cfg(windows)]
pub fn ocr_available() -> bool {
    imp::engine_available()
}

#[cfg(not(windows))]
pub fn ocr_available() -> bool {
    false
}

#[cfg(windows)]
pub fn recognize(image: &image::RgbaImage) -> Result<Vec<OcrLine>> {
    imp::recognize(image)
}

#[cfg(not(windows))]
pub fn recognize(_image: &image::RgbaImage) -> Result<Vec<OcrLine>> {
    anyhow::bail!("on-device OCR is only available on Windows")
}

#[cfg(windows)]
mod imp {
    use super::{OcrLine, OcrWord};
    use anyhow::{anyhow, Result};
    use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::DataWriter;

    pub fn engine_available() -> bool {
        OcrEngine::TryCreateFromUserProfileLanguages().is_ok()
    }

    pub fn recognize(image: &image::RgbaImage) -> Result<Vec<OcrLine>> {
        let engine = OcrEngine::TryCreateFromUserProfileLanguages()
            .map_err(|e| anyhow!("OCR engine unavailable: {e}"))?;
        let max_dim = OcrEngine::MaxImageDimension().unwrap_or(2600);

        let (width, height) = (image.width(), image.height());
        let longest = width.max(height);
        let (bitmap_image, scale): (std::borrow::Cow<image::RgbaImage>, f64) =
            if longest > max_dim {
                let factor = f64::from(max_dim) / f64::from(longest);
                let new_w = ((f64::from(width) * factor).round() as u32).max(1);
                let new_h = ((f64::from(height) * factor).round() as u32).max(1);
                (
                    std::borrow::Cow::Owned(image::imageops::resize(
                        image,
                        new_w,
                        new_h,
                        image::imageops::FilterType::Triangle,
                    )),
                    f64::from(width) / f64::from(new_w),
                )
            } else {
                (std::borrow::Cow::Borrowed(image), 1.0)
            };

        let mut bgra = Vec::with_capacity(
            (bitmap_image.width() * bitmap_image.height() * 4) as usize,
        );
        for pixel in bitmap_image.pixels() {
            let [r, g, b, a] = pixel.0;
            bgra.extend_from_slice(&[b, g, r, a]);
        }

        let writer = DataWriter::new().map_err(|e| anyhow!("OCR buffer init failed: {e}"))?;
        writer
            .WriteBytes(&bgra)
            .map_err(|e| anyhow!("OCR buffer write failed: {e}"))?;
        let buffer = writer
            .DetachBuffer()
            .map_err(|e| anyhow!("OCR buffer detach failed: {e}"))?;
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
            &buffer,
            BitmapPixelFormat::Bgra8,
            bitmap_image.width() as i32,
            bitmap_image.height() as i32,
        )
        .map_err(|e| anyhow!("OCR bitmap creation failed: {e}"))?;

        let result = engine
            .RecognizeAsync(&bitmap)
            .map_err(|e| anyhow!("OCR recognize dispatch failed: {e}"))?
            .get()
            .map_err(|e| anyhow!("OCR recognize failed: {e}"))?;

        let mut lines = Vec::new();
        for line in result.Lines().map_err(|e| anyhow!("OCR lines failed: {e}"))? {
            let mut words = Vec::new();
            for word in line.Words().map_err(|e| anyhow!("OCR words failed: {e}"))? {
                let text = word
                    .Text()
                    .map_err(|e| anyhow!("OCR word text failed: {e}"))?
                    .to_string();
                let rect = word
                    .BoundingRect()
                    .map_err(|e| anyhow!("OCR word rect failed: {e}"))?;
                let x = (f64::from(rect.X) * scale).floor().max(0.0) as u32;
                let y = (f64::from(rect.Y) * scale).floor().max(0.0) as u32;
                let w = (f64::from(rect.Width) * scale).ceil().max(1.0) as u32;
                let h = (f64::from(rect.Height) * scale).ceil().max(1.0) as u32;
                if text.trim().is_empty() {
                    continue;
                }
                words.push(OcrWord { text, x, y, w, h });
            }
            if !words.is_empty() {
                lines.push(OcrLine { words });
            }
        }
        Ok(lines)
    }
}
