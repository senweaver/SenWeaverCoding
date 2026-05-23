// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub const MAX_INLINE_MEDIA_BYTES: usize = 10 * 1024 * 1024;

pub fn ext_from_mime(mime: &str) -> &'static str {
    match mime.split(';').next().unwrap_or("").trim() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/ogg" => "ogg",
        "audio/wav" | "audio/wave" => "wav",
        "video/mp4" => "mp4",
        "application/pdf" => "pdf",
        "application/json" => "json",
        _ => "bin",
    }
}
