// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{anyhow, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;

use super::detect::{resolve_overlaps, shannon_entropy, SensitiveCategory, SensitiveMatch, SensitiveSeverity};
use super::ocr::{self, OcrLine};
use super::secrets::scan_text;

const REDACT_JPEG_QUALITY: u8 = 85;
const BOX_PAD: u32 = 4;
const KNOWN_VALUE_RANK: u32 = 100;
const ENTROPY_MIN_LEN: usize = 20;
const ENTROPY_THRESHOLD: f64 = 3.2;
const ALNUM_RATIO_MIN: f64 = 0.75;

static FRAME_CREDENTIAL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(?:api[_\-]?key|secret|token|password|passwd|pwd|bearer|credential|auth)\s*[:=]\s*(\S{6,})",
    )
    .expect("frame credential regex")
});

#[derive(Debug, Clone, Copy)]
pub enum FramePolicy {
    Inactive,
    Withhold,
    Redact,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RedactStats {
    pub frames_blurred: u32,
    pub regions_blurred: u32,
}

enum CachedFrame {
    Clean(std::sync::Arc<Vec<u8>>),
    Redacted(std::sync::Arc<Vec<u8>>),
    Failed(String),
}

pub struct FrameRedactor {
    known_values: Vec<String>,
    cache: HashMap<String, CachedFrame>,
    stats: RedactStats,
}

impl FrameRedactor {
    pub fn new(known_values: Vec<String>) -> Self {
        Self {
            known_values,
            cache: HashMap::new(),
            stats: RedactStats::default(),
        }
    }

    pub fn stats(&self) -> RedactStats {
        self.stats
    }

    pub fn redact_frame_bytes(&mut self, cache_key: &str, jpeg: &[u8]) -> Result<std::sync::Arc<Vec<u8>>> {
        if let Some(cached) = self.cache.get(cache_key) {
            return match cached {
                CachedFrame::Clean(bytes) | CachedFrame::Redacted(bytes) => {
                    Ok(std::sync::Arc::clone(bytes))
                }
                CachedFrame::Failed(message) => Err(anyhow!("{message}")),
            };
        }
        match redact_jpeg(jpeg, &self.known_values) {
            Ok((bytes, regions)) => {
                let bytes = std::sync::Arc::new(bytes);
                if regions > 0 {
                    self.stats.frames_blurred += 1;
                    self.stats.regions_blurred += regions;
                    self.cache.insert(
                        cache_key.to_string(),
                        CachedFrame::Redacted(std::sync::Arc::clone(&bytes)),
                    );
                } else {
                    self.cache.insert(
                        cache_key.to_string(),
                        CachedFrame::Clean(std::sync::Arc::clone(&bytes)),
                    );
                }
                Ok(bytes)
            }
            Err(e) => {
                let message = format!("frame redaction failed: {e}");
                self.cache
                    .insert(cache_key.to_string(), CachedFrame::Failed(message.clone()));
                Err(anyhow!("{message}"))
            }
        }
    }

    pub fn redact_frame_file(&mut self, dir: &Path, file: &str) -> Result<std::sync::Arc<Vec<u8>>> {
        if let Some(cached) = self.cache.get(file) {
            return match cached {
                CachedFrame::Clean(bytes) | CachedFrame::Redacted(bytes) => {
                    Ok(std::sync::Arc::clone(bytes))
                }
                CachedFrame::Failed(message) => Err(anyhow!("{message}")),
            };
        }
        let path = dir.join("frames").join(file);
        let jpeg = std::fs::read(&path).map_err(|e| anyhow!("failed to read frame {file}: {e}"))?;
        self.redact_frame_bytes(file, &jpeg)
    }
}

fn frame_heuristic_matches(text: &str) -> Vec<SensitiveMatch> {
    let mut out = Vec::new();
    for caps in FRAME_CREDENTIAL_RE.captures_iter(text) {
        if let Some(m) = caps.get(1) {
            out.push(SensitiveMatch {
                category: SensitiveCategory::ApiKey,
                label: "Credential",
                severity: SensitiveSeverity::High,
                value: m.as_str().to_string(),
                start: m.start(),
                end: m.end(),
                rank: 60,
            });
        }
    }
    let mut offset = 0usize;
    for token in text.split_whitespace() {
        let start = match text[offset..].find(token) {
            Some(rel) => offset + rel,
            None => continue,
        };
        offset = start + token.len();
        if token.chars().count() < ENTROPY_MIN_LEN {
            continue;
        }
        let alnum = token.chars().filter(|c| c.is_ascii_alphanumeric()).count();
        let ratio = alnum as f64 / token.chars().count() as f64;
        if ratio < ALNUM_RATIO_MIN {
            continue;
        }
        let has_digit = token.chars().any(|c| c.is_ascii_digit());
        let has_alpha = token.chars().any(|c| c.is_ascii_alphabetic());
        if !has_digit || !has_alpha {
            continue;
        }
        let is_pure_hex = token
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '-');
        if is_pure_hex {
            continue;
        }
        if shannon_entropy(token) < ENTROPY_THRESHOLD {
            continue;
        }
        out.push(SensitiveMatch {
            category: SensitiveCategory::ApiKey,
            label: "High-entropy token",
            severity: SensitiveSeverity::High,
            value: token.to_string(),
            start,
            end: start + token.len(),
            rank: 50,
        });
    }
    out
}

fn known_value_matches(text: &str, known_values: &[String]) -> Vec<SensitiveMatch> {
    let mut out = Vec::new();
    for value in known_values {
        if value.chars().count() < 3 {
            continue;
        }
        let mut from = 0usize;
        while let Some(rel) = text[from..].find(value.as_str()) {
            let start = from + rel;
            out.push(SensitiveMatch {
                category: SensitiveCategory::ApiKey,
                label: "Detected sensitive value",
                severity: SensitiveSeverity::High,
                value: value.clone(),
                start,
                end: start + value.len(),
                rank: KNOWN_VALUE_RANK,
            });
            from = start + value.len();
        }
    }
    out
}

struct LineLayout {
    text: String,
    spans: Vec<(usize, usize, u32, u32, u32, u32)>,
}

fn layout_line(line: &OcrLine) -> LineLayout {
    let mut text = String::new();
    let mut spans = Vec::new();
    for (i, word) in line.words.iter().enumerate() {
        if i > 0 {
            text.push(' ');
        }
        let start = text.len();
        text.push_str(&word.text);
        spans.push((start, text.len(), word.x, word.y, word.w, word.h));
    }
    LineLayout { text, spans }
}

fn redact_jpeg(jpeg: &[u8], known_values: &[String]) -> Result<(Vec<u8>, u32)> {
    let decoded = image::load_from_memory(jpeg)
        .map_err(|e| anyhow!("decode frame failed: {e}"))?
        .to_rgba8();
    let lines = ocr::recognize(&decoded)?;

    let mut boxes: Vec<(u32, u32, u32, u32)> = Vec::new();
    for line in &lines {
        let layout = layout_line(line);
        if layout.text.trim().is_empty() {
            continue;
        }
        let mut matches = scan_text(&layout.text);
        matches.extend(frame_heuristic_matches(&layout.text));
        matches.extend(known_value_matches(&layout.text, known_values));
        let matches = resolve_overlaps(matches);
        for m in &matches {
            for (start, end, x, y, w, h) in &layout.spans {
                if m.start < *end && *start < m.end {
                    boxes.push((*x, *y, *w, *h));
                }
            }
        }
    }

    let regions = boxes.len() as u32;
    let mut canvas = decoded;
    for (x, y, w, h) in boxes {
        let x0 = x.saturating_sub(BOX_PAD);
        let y0 = y.saturating_sub(BOX_PAD);
        let x1 = (x + w + BOX_PAD).min(canvas.width());
        let y1 = (y + h + BOX_PAD).min(canvas.height());
        for py in y0..y1 {
            for px in x0..x1 {
                canvas.put_pixel(px, py, image::Rgba([17, 17, 17, 255]));
            }
        }
    }

    let rgb = image::DynamicImage::ImageRgba8(canvas).to_rgb8();
    let mut out = Vec::new();
    let mut encoder =
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, REDACT_JPEG_QUALITY);
    encoder
        .encode_image(&rgb)
        .map_err(|e| anyhow!("encode redacted frame failed: {e}"))?;
    Ok((out, regions))
}
