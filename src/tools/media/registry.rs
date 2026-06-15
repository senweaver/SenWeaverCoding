// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaSurface {
    Image,
    Video,
    Audio,
}

impl MediaSurface {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "image" => Some(Self::Image),
            "video" => Some(Self::Video),
            "audio" => Some(Self::Audio),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
        }
    }

    pub fn extension(&self, audio_kind: Option<&str>) -> &'static str {
        match self {
            Self::Image => "png",
            Self::Video => "mp4",
            Self::Audio => match audio_kind {
                Some("sfx") | Some("music") => "mp3",
                _ => "mp3",
            },
        }
    }
}

pub fn aspect_to_openai_size(aspect: &str) -> &'static str {
    match aspect {
        "16:9" | "4:3" => "1536x1024",
        "9:16" | "3:4" => "1024x1536",
        _ => "1024x1024",
    }
}

pub fn aspect_to_fal_size(aspect: &str) -> &'static str {
    match aspect {
        "16:9" => "landscape_16_9",
        "4:3" => "landscape_4_3",
        "9:16" => "portrait_16_9",
        "3:4" => "portrait_4_3",
        _ => "square_hd",
    }
}

pub fn aspect_to_pixels(aspect: &str, resolution: &str) -> Option<(u32, u32)> {
    let long: u32 = match resolution {
        "2k" => 2048,
        "4k" => 4096,
        _ => return None,
    };
    let (w_ratio, h_ratio): (u32, u32) = match aspect {
        "16:9" => (16, 9),
        "9:16" => (9, 16),
        "4:3" => (4, 3),
        "3:4" => (3, 4),
        _ => (1, 1),
    };
    if w_ratio >= h_ratio {
        Some((long, (long * h_ratio / w_ratio / 8) * 8))
    } else {
        Some(((long * w_ratio / h_ratio / 8) * 8, long))
    }
}

pub fn is_fal_model(model: &str) -> bool {
    model.starts_with("fal-ai/")
}

pub fn is_gemini_image_model(model: &str) -> bool {
    model.trim().to_ascii_lowercase().starts_with("gemini")
}

pub fn is_hyperframes_model(model: &str) -> bool {
    model.eq_ignore_ascii_case("hyperframes-html") || model.eq_ignore_ascii_case("hyperframes")
}

pub fn default_models(surface: MediaSurface) -> serde_json::Value {
    match surface {
        MediaSurface::Image => json!([
            { "id": "gpt-image-1", "label": "GPT Image", "provider": "openai" },
            { "id": "dall-e-3", "label": "DALL-E 3", "provider": "openai" },
            { "id": "gemini-2.5-flash-image", "label": "Gemini 2.5 Flash Image", "provider": "gemini" },
            { "id": "fal-ai/flux/schnell", "label": "FLUX schnell (fal)", "provider": "fal" },
            { "id": "fal-ai/flux-pro/v1.1-ultra", "label": "FLUX 1.1 Ultra (fal)", "provider": "fal" },
        ]),
        MediaSurface::Video => json!([
            { "id": "fal-ai/sora", "label": "Sora (fal)", "provider": "fal" },
            { "id": "fal-ai/veo3", "label": "Veo 3 (fal)", "provider": "fal" },
            { "id": "fal-ai/bytedance/seedance/v1/pro", "label": "Seedance Pro (fal)", "provider": "fal" },
            { "id": "hyperframes-html", "label": "HyperFrames (local HTML→MP4)", "provider": "hyperframes" },
        ]),
        MediaSurface::Audio => json!([
            { "id": "gpt-4o-mini-tts", "label": "OpenAI TTS", "provider": "openai" },
            { "id": "elevenlabs-tts", "label": "ElevenLabs TTS", "provider": "elevenlabs" },
            { "id": "elevenlabs-sfx", "label": "ElevenLabs SFX", "provider": "elevenlabs" },
        ]),
    }
}
