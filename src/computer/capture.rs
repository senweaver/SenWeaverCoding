// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use std::io::Cursor;
use std::sync::Arc;

use super::coordinates::MonitorRect;

const MAX_TRANSPORT_DIM: u32 = 1536;
const PREVIEW_MAX_DIM: u32 = 640;
const PREVIEW_JPEG_QUALITY: u8 = 60;
const DISPLAY_JPEG_QUALITY: u8 = 72;

#[derive(Debug, Clone)]
pub struct CapturedScreen {
    pub width: u32,
    pub height: u32,
    pub png_base64: String,
    pub display_jpeg_base64: String,
    pub monitor: MonitorRect,
}

impl CapturedScreen {
    pub fn data_uri(&self) -> String {
        format!("data:image/png;base64,{}", self.png_base64)
    }
}

#[derive(Debug, Clone)]
pub struct RecorderFrame {
    pub width: u32,
    pub height: u32,
    pub transport_width: u32,
    pub transport_height: u32,
    pub phash: u64,
    pub shot_jpeg_bytes: Arc<Vec<u8>>,
    pub preview_jpeg_base64: Arc<str>,
    pub monitor: MonitorRect,
}

#[derive(Debug, Clone)]
pub struct RawFrame {
    pub image: Arc<image::RgbaImage>,
    pub monitor: MonitorRect,
}

pub async fn capture_raw(monitor_id: Option<u32>) -> Result<RawFrame> {
    tokio::task::spawn_blocking(move || {
        super::dpi::ensure_dpi_awareness();
        let selector = match monitor_id {
            Some(id) => MonitorSelector::Id(id),
            None => MonitorSelector::Primary,
        };
        let (transport, _, _, monitor) = grab_frame(selector)?;
        Ok(RawFrame {
            image: Arc::new(transport),
            monitor,
        })
    })
    .await
    .map_err(|e| anyhow!("screen capture task failed to join: {e}"))?
}

pub async fn encode_frame_for_vision(frame: &RawFrame) -> Result<(String, String)> {
    let image = Arc::clone(&frame.image);
    tokio::task::spawn_blocking(move || {
        let png_bytes = encode_png(&image)?;
        let png_base64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
        let display_jpeg = encode_display_jpeg_base64(&image)?;
        Ok((format!("data:image/png;base64,{png_base64}"), display_jpeg))
    })
    .await
    .map_err(|e| anyhow!("frame encode task failed to join: {e}"))?
}

pub async fn encode_frame_display_jpeg(frame: &RawFrame) -> Result<String> {
    let image = Arc::clone(&frame.image);
    tokio::task::spawn_blocking(move || encode_display_jpeg_base64(&image))
        .await
        .map_err(|e| anyhow!("frame encode task failed to join: {e}"))?
}

fn monitor_rect(monitor: &xcap::Monitor) -> MonitorRect {
    MonitorRect {
        id: monitor.id().unwrap_or(0),
        x: monitor.x().unwrap_or(0),
        y: monitor.y().unwrap_or(0),
        width: monitor.width().unwrap_or(0) as i32,
        height: monitor.height().unwrap_or(0) as i32,
    }
}

pub async fn list_monitors() -> Vec<MonitorRect> {
    tokio::task::spawn_blocking(|| {
        super::dpi::ensure_dpi_awareness();
        match xcap::Monitor::all() {
            Ok(monitors) => monitors.iter().map(monitor_rect).collect(),
            Err(_) => Vec::new(),
        }
    })
    .await
    .unwrap_or_default()
}

pub async fn capture_primary() -> Result<CapturedScreen> {
    tokio::task::spawn_blocking(|| {
        super::dpi::ensure_dpi_awareness();
        capture_screen_blocking(MonitorSelector::Primary)
    })
    .await
    .map_err(|e| anyhow!("screen capture task failed to join: {e}"))?
}

pub async fn capture_monitor(id: u32) -> Result<CapturedScreen> {
    tokio::task::spawn_blocking(move || {
        super::dpi::ensure_dpi_awareness();
        capture_screen_blocking(MonitorSelector::Id(id))
    })
    .await
    .map_err(|e| anyhow!("screen capture task failed to join: {e}"))?
}

pub async fn capture_recorder_frame() -> Result<RecorderFrame> {
    tokio::task::spawn_blocking(|| {
        super::dpi::ensure_dpi_awareness();
        let selector = match cursor_point() {
            Some((x, y)) => MonitorSelector::Point { x, y },
            None => MonitorSelector::Primary,
        };
        capture_recorder_frame_blocking(selector)
    })
    .await
    .map_err(|e| anyhow!("screen capture task failed to join: {e}"))?
}

#[cfg(windows)]
fn cursor_point() -> Option<(i32, i32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut point = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut point) } != 0 {
        Some((point.x, point.y))
    } else {
        None
    }
}

#[cfg(not(windows))]
fn cursor_point() -> Option<(i32, i32)> {
    None
}

pub async fn capture_preview_jpeg() -> Result<String> {
    tokio::task::spawn_blocking(|| {
        super::dpi::ensure_dpi_awareness();
        let (transport, _, _, _) = grab_frame(MonitorSelector::Primary)?;
        encode_preview_jpeg_base64(&transport)
    })
    .await
    .map_err(|e| anyhow!("screen capture task failed to join: {e}"))?
}

#[derive(Debug, Clone, Copy)]
enum MonitorSelector {
    Primary,
    Id(u32),
    Point { x: i32, y: i32 },
}

fn select_monitor(selector: MonitorSelector) -> Result<xcap::Monitor> {
    let monitors = xcap::Monitor::all().map_err(|e| anyhow!("enumerate monitors failed: {e}"))?;
    if monitors.is_empty() {
        return Err(anyhow!("no monitors available for capture"));
    }
    let chosen = match selector {
        MonitorSelector::Id(id) => monitors
            .iter()
            .find(|m| m.id().map(|mid| mid == id).unwrap_or(false))
            .cloned(),
        MonitorSelector::Point { x, y } => xcap::Monitor::from_point(x, y)
            .ok()
            .or_else(|| {
                monitors
                    .iter()
                    .find(|m| monitor_rect(m).contains(x, y))
                    .cloned()
            }),
        MonitorSelector::Primary => None,
    };
    if let Some(monitor) = chosen {
        return Ok(monitor);
    }
    let primary = monitors
        .iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .cloned();
    Ok(primary.unwrap_or_else(|| monitors[0].clone()))
}

pub fn register_overlay_hwnd(hwnd: isize) {
    pin_overlay_hwnd(hwnd);
}

pub fn pin_overlay_hwnd(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        };
        let handle = hwnd as HWND;
        unsafe {
            let _ = SetWindowPos(
                handle,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
}

fn grab_frame(selector: MonitorSelector) -> Result<(image::RgbaImage, u32, u32, MonitorRect)> {
    let monitor = select_monitor(selector)?;
    let rect = monitor_rect(&monitor);

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

    Ok((transport, width, height, rect))
}

fn encode_png(image: &image::RgbaImage) -> Result<Vec<u8>> {
    let mut png_bytes: Vec<u8> = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png_bytes), image::ImageFormat::Png)
        .context("encode screenshot to PNG")?;
    Ok(png_bytes)
}

fn encode_display_jpeg_bytes(image: &image::RgbaImage) -> Result<Vec<u8>> {
    let rgb = image::DynamicImage::ImageRgba8(image.clone()).to_rgb8();
    let mut jpeg_bytes: Vec<u8> = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
        &mut jpeg_bytes,
        DISPLAY_JPEG_QUALITY,
    );
    encoder
        .encode_image(&rgb)
        .context("encode screenshot to display JPEG")?;
    Ok(jpeg_bytes)
}

fn encode_display_jpeg_base64(image: &image::RgbaImage) -> Result<String> {
    let jpeg_bytes = encode_display_jpeg_bytes(image)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(&jpeg_bytes))
}

pub(crate) fn encode_preview_jpeg_base64(image: &image::RgbaImage) -> Result<String> {
    let longest = image.width().max(image.height());
    let preview = if longest > PREVIEW_MAX_DIM {
        let factor = f64::from(PREVIEW_MAX_DIM) / f64::from(longest);
        let new_width = ((f64::from(image.width()) * factor).round() as u32).max(1);
        let new_height = ((f64::from(image.height()) * factor).round() as u32).max(1);
        image::imageops::resize(
            image,
            new_width,
            new_height,
            image::imageops::FilterType::Triangle,
        )
    } else {
        image.clone()
    };
    let rgb = image::DynamicImage::ImageRgba8(preview).to_rgb8();
    let mut jpeg_bytes: Vec<u8> = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
        &mut jpeg_bytes,
        PREVIEW_JPEG_QUALITY,
    );
    encoder
        .encode_image(&rgb)
        .context("encode screenshot preview to JPEG")?;
    Ok(base64::engine::general_purpose::STANDARD.encode(&jpeg_bytes))
}

fn capture_screen_blocking(selector: MonitorSelector) -> Result<CapturedScreen> {
    let (transport, width, height, monitor) = grab_frame(selector)?;
    let png_bytes = encode_png(&transport)?;
    let png_base64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
    let display_jpeg_base64 = encode_display_jpeg_base64(&transport)?;

    Ok(CapturedScreen {
        width,
        height,
        png_base64,
        display_jpeg_base64,
        monitor,
    })
}

fn capture_recorder_frame_blocking(selector: MonitorSelector) -> Result<RecorderFrame> {
    let (transport, width, height, monitor) = grab_frame(selector)?;
    let shot_jpeg_bytes = encode_display_jpeg_bytes(&transport)?;
    let preview = encode_preview_jpeg_base64(&transport)?;
    let phash = super::frames::hash::dhash64(&transport);

    Ok(RecorderFrame {
        width,
        height,
        transport_width: transport.width(),
        transport_height: transport.height(),
        phash,
        shot_jpeg_bytes: Arc::new(shot_jpeg_bytes),
        preview_jpeg_base64: Arc::from(preview),
        monitor,
    })
}
