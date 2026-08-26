// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{anyhow, Result};
use std::path::Path;

use super::{
    load_audio_manifest, load_transcript, save_transcript, NarrationSegment, NarrationTranscript,
    AUDIO_DIR,
};
use crate::config::Config;

const BOILERPLATE: [&str; 12] = [
    "thank you",
    "thanks for watching",
    "thank you for watching",
    "please subscribe",
    "subtitles by the amara org community",
    "grazie",
    "merci",
    "gracias",
    "obrigado",
    "danke",
    "谢谢",
    "谢谢观看",
];

fn is_meaningful_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.chars().count() < 2 {
        return false;
    }
    if !trimmed.chars().any(char::is_alphanumeric) {
        return false;
    }
    let normalized: String = trimmed
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    !BOILERPLATE.contains(&normalized.as_str())
}

pub fn transcription_configured(config: &Config) -> bool {
    let t = &config.transcription;
    match crate::config::schema::normalize_transcription_provider(&t.default_provider)
        .unwrap_or(t.default_provider.trim())
    {
        "groq" => {
            t.api_key.as_deref().is_some_and(|k| !k.trim().is_empty())
                || std::env::var("GROQ_API_KEY").is_ok_and(|k| !k.trim().is_empty())
        }
        "openai" => t.openai.is_some(),
        "deepgram" => t.deepgram.is_some(),
        "" => false,
        _ => true,
    }
}

pub fn has_pending_transcription(dir: &Path) -> bool {
    let Some(manifest) = load_audio_manifest(dir) else {
        return false;
    };
    if manifest.segments.is_empty() {
        return false;
    }
    load_transcript(dir).is_none()
}

pub async fn transcribe_recording(
    config: &Config,
    dir: &Path,
    session_started_epoch: i64,
) -> Result<NarrationTranscript> {
    let manifest = load_audio_manifest(dir)
        .ok_or_else(|| anyhow!("this recording has no narration audio"))?;
    if manifest.segments.is_empty() {
        return Err(anyhow!("this recording has no narration audio"));
    }
    if !transcription_configured(config) {
        return Err(anyhow!(
            "no transcription provider is configured; set one up in [transcription] settings"
        ));
    }

    let mut segments: Vec<NarrationSegment> = Vec::new();
    for segment in &manifest.segments {
        let path = dir.join(AUDIO_DIR).join(&segment.file);
        let data = tokio::fs::read(&path)
            .await
            .map_err(|e| anyhow!("failed to read audio segment {}: {e}", segment.file))?;
        let text = match crate::channels::pipeline::transcription::transcribe_audio(
            data,
            &segment.file,
            &config.transcription,
        )
        .await
        {
            Ok(text) => text,
            Err(e) => {
                return Err(anyhow!(
                    "transcription failed on segment {}: {e}",
                    segment.file
                ));
            }
        };
        let trimmed = text.trim();
        if trimmed.is_empty() || !is_meaningful_text(trimmed) {
            continue;
        }
        segments.push(NarrationSegment {
            at_ms: (segment.start_epoch - session_started_epoch).max(0),
            end_ms: (segment.stop_epoch - session_started_epoch).max(0),
            text: trimmed.to_string(),
        });
    }
    segments.sort_by_key(|s| s.at_ms);

    let transcript = NarrationTranscript {
        provider: config.transcription.default_provider.clone(),
        language: manifest.narration_language.clone(),
        transcribed_at: chrono::Utc::now().timestamp_millis(),
        segments,
    };
    save_transcript(dir, &transcript)?;
    Ok(transcript)
}
