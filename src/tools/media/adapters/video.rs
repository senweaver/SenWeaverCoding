// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::MediaJob;
use crate::tools::media::registry;
use crate::tools::media::tasks::{download_bytes, first_string, poll_until};
use anyhow::{anyhow, Context};
use serde_json::json;
use std::time::Duration;

pub async fn generate(job: &MediaJob) -> anyhow::Result<Vec<u8>> {
    if registry::is_fal_model(&job.model) {
        return fal_video(job).await;
    }
    openai_video(job).await
}

async fn fal_video(job: &MediaJob) -> anyhow::Result<Vec<u8>> {
    let key = job.require_key()?;
    let submit_url = format!("https://queue.fal.run/{}", job.model);
    let body = json!({
        "prompt": job.prompt,
        "aspect_ratio": job.aspect,
        "duration": job.seconds,
    });
    let resp = job
        .client
        .post(&submit_url)
        .header("Authorization", format!("Key {key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("fal video submit failed")?;
    let status = resp.status();
    let submit: serde_json::Value = resp.json().await.context("failed to parse fal submit")?;
    if !status.is_success() {
        return Err(anyhow!("fal video submit error ({status}): {submit}"));
    }
    let status_url = first_string(&submit, &["/status_url"])
        .ok_or_else(|| anyhow!("fal submit missing status_url: {submit}"))?
        .to_string();
    let response_url = first_string(&submit, &["/response_url"])
        .ok_or_else(|| anyhow!("fal submit missing response_url: {submit}"))?
        .to_string();

    poll_until(
        &job.client,
        &status_url,
        Some(("Authorization", format!("Key {key}"))),
        Duration::from_secs(5),
        Duration::from_secs(600),
        |body| {
            let state = body.get("status").and_then(|v| v.as_str()).unwrap_or("");
            match state {
                "COMPLETED" => Some(Ok(body.clone())),
                "FAILED" | "ERROR" => Some(Err(anyhow!("fal video job failed: {body}"))),
                _ => None,
            }
        },
    )
    .await?;

    let result_resp = job
        .client
        .get(&response_url)
        .header("Authorization", format!("Key {key}"))
        .send()
        .await
        .context("fal video result fetch failed")?;
    let result: serde_json::Value = result_resp
        .json()
        .await
        .context("failed to parse fal video result")?;
    let video_url = first_string(
        &result,
        &["/video/url", "/videos/0/url", "/output/url", "/url"],
    )
    .ok_or_else(|| anyhow!("no video url in fal result: {result}"))?;
    download_bytes(&job.client, video_url).await
}

async fn openai_video(job: &MediaJob) -> anyhow::Result<Vec<u8>> {
    let key = job.require_key()?;
    let base = job.provider.base_url.trim_end_matches('/').to_string();
    let submit_url = format!("{base}/videos");
    let body = json!({
        "model": job.model,
        "prompt": job.prompt,
        "seconds": job.seconds.to_string(),
        "size": registry::aspect_to_openai_size(&job.aspect),
    });
    let resp = job
        .client
        .post(&submit_url)
        .header("Authorization", format!("Bearer {key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("video submit failed")?;
    let status = resp.status();
    let submit: serde_json::Value = resp.json().await.context("failed to parse video submit")?;
    if !status.is_success() {
        return Err(anyhow!("video submit error ({status}): {submit}"));
    }
    let job_id = first_string(&submit, &["/id"])
        .ok_or_else(|| anyhow!("video submit missing id: {submit}"))?
        .to_string();

    let status_url = format!("{base}/videos/{job_id}");
    poll_until(
        &job.client,
        &status_url,
        Some(("Authorization", format!("Bearer {key}"))),
        Duration::from_secs(6),
        Duration::from_secs(600),
        |body| {
            let state = body.get("status").and_then(|v| v.as_str()).unwrap_or("");
            match state {
                "completed" | "succeeded" => Some(Ok(body.clone())),
                "failed" | "error" | "cancelled" => {
                    Some(Err(anyhow!("video job failed: {body}")))
                }
                _ => None,
            }
        },
    )
    .await?;

    let content_url = format!("{base}/videos/{job_id}/content");
    let content_resp = job
        .client
        .get(&content_url)
        .header("Authorization", format!("Bearer {key}"))
        .send()
        .await
        .context("video content fetch failed")?;
    if content_resp.status().is_success() {
        return Ok(content_resp
            .bytes()
            .await
            .context("failed to read video bytes")?
            .to_vec());
    }
    Err(anyhow!(
        "could not download video content ({})",
        content_resp.status()
    ))
}
