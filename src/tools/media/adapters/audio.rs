// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::MediaJob;
use anyhow::{anyhow, Context};
use serde_json::json;

fn is_elevenlabs(job: &MediaJob) -> bool {
    job.provider.provider_id.eq_ignore_ascii_case("elevenlabs")
        || job.model.to_ascii_lowercase().contains("elevenlabs")
}

pub async fn generate(job: &MediaJob) -> anyhow::Result<Vec<u8>> {
    if is_elevenlabs(job) {
        return elevenlabs_audio(job).await;
    }
    openai_speech(job).await
}

async fn openai_speech(job: &MediaJob) -> anyhow::Result<Vec<u8>> {
    let key = job.require_key()?;
    let url = format!("{}/audio/speech", job.provider.base_url.trim_end_matches('/'));
    let voice = job.voice.clone().unwrap_or_else(|| "alloy".to_string());
    let body = json!({
        "model": job.model,
        "input": job.prompt,
        "voice": voice,
        "response_format": "mp3",
    });
    let resp = job
        .client
        .post(&url)
        .header("Authorization", format!("Bearer {key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("speech request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("speech API error ({status}): {text}"));
    }
    Ok(resp.bytes().await.context("failed to read audio bytes")?.to_vec())
}

async fn elevenlabs_audio(job: &MediaJob) -> anyhow::Result<Vec<u8>> {
    let key = job.require_key()?;
    let base = job.provider.base_url.trim_end_matches('/');
    let (url, body) = if job.audio_kind == "sfx" || job.audio_kind == "music" {
        (
            format!("{base}/v1/sound-generation"),
            json!({
                "text": job.prompt,
                "duration_seconds": job.seconds.clamp(1, 22),
            }),
        )
    } else {
        let voice = job
            .voice
            .clone()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "21m00Tcm4TlvDq8ikWAM".to_string());
        (
            format!("{base}/v1/text-to-speech/{voice}"),
            json!({
                "text": job.prompt,
                "model_id": "eleven_multilingual_v2",
            }),
        )
    };
    let resp = job
        .client
        .post(&url)
        .header("xi-api-key", key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("elevenlabs request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("elevenlabs API error ({status}): {text}"));
    }
    Ok(resp.bytes().await.context("failed to read audio bytes")?.to_vec())
}
