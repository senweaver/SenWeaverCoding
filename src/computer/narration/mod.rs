// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod transcribe;

use serde::{Deserialize, Serialize};
use std::path::Path;

pub const AUDIO_DIR: &str = "audio";
pub const AUDIO_MANIFEST_FILE: &str = "audio.json";
pub const NARRATION_FILE: &str = "narration.json";
pub const MAX_SEGMENT_BYTES: usize = 24 * 1024 * 1024;
pub const MAX_SEGMENTS: usize = 240;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioSegment {
    pub file: String,
    pub start_epoch: i64,
    pub stop_epoch: i64,
    pub duration_ms: i64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioManifest {
    pub version: u32,
    pub narration_language: String,
    pub segments: Vec<AudioSegment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NarrationSegment {
    pub at_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NarrationTranscript {
    pub provider: String,
    pub language: String,
    pub transcribed_at: i64,
    pub segments: Vec<NarrationSegment>,
}

pub fn load_audio_manifest(dir: &Path) -> Option<AudioManifest> {
    let content = std::fs::read_to_string(dir.join(AUDIO_MANIFEST_FILE)).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn save_audio_manifest(dir: &Path, manifest: &AudioManifest) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(manifest)?;
    std::fs::write(dir.join(AUDIO_MANIFEST_FILE), bytes)?;
    Ok(())
}

pub fn load_transcript(dir: &Path) -> Option<NarrationTranscript> {
    let content = std::fs::read_to_string(dir.join(NARRATION_FILE)).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn save_transcript(dir: &Path, transcript: &NarrationTranscript) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(transcript)?;
    std::fs::write(dir.join(NARRATION_FILE), bytes)?;
    Ok(())
}

pub fn transcript_updated_at(dir: &Path) -> Option<i64> {
    load_transcript(dir).map(|t| t.transcribed_at)
}

pub fn append_segment(
    dir: &Path,
    language: &str,
    start_epoch: i64,
    stop_epoch: i64,
    data: &[u8],
) -> anyhow::Result<AudioSegment> {
    if data.is_empty() {
        anyhow::bail!("audio segment is empty");
    }
    if data.len() > MAX_SEGMENT_BYTES {
        anyhow::bail!("audio segment exceeds {MAX_SEGMENT_BYTES} bytes");
    }
    let mut manifest = load_audio_manifest(dir).unwrap_or(AudioManifest {
        version: 2,
        narration_language: language.to_string(),
        segments: Vec::new(),
    });
    if manifest.segments.len() >= MAX_SEGMENTS {
        anyhow::bail!("audio segment limit reached ({MAX_SEGMENTS})");
    }
    if !language.trim().is_empty() {
        manifest.narration_language = language.trim().to_string();
    }
    let index = manifest.segments.len() + 1;
    let file = format!("segment-{index:04}.webm");
    let audio_dir = dir.join(AUDIO_DIR);
    std::fs::create_dir_all(&audio_dir)?;
    std::fs::write(audio_dir.join(&file), data)?;
    let segment = AudioSegment {
        file,
        start_epoch,
        stop_epoch,
        duration_ms: (stop_epoch - start_epoch).max(0),
        bytes: data.len() as u64,
    };
    manifest.segments.push(segment.clone());
    save_audio_manifest(dir, &manifest)?;
    Ok(segment)
}
