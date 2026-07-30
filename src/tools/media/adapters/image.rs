// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::MediaJob;
use crate::tools::media::registry;
use crate::tools::media::tasks::{download_bytes, first_string};
use anyhow::{anyhow, Context};
use base64::Engine;
use serde_json::json;

pub async fn generate(job: &MediaJob) -> anyhow::Result<Vec<u8>> {
    if registry::is_fal_model(&job.model) {
        return fal_image(job).await;
    }
    match job.provider.provider_id.to_ascii_lowercase().as_str() {
        "openrouter" => openrouter_image(job).await,
        "gemini" | "google" => gemini_image(job).await,
        _ if registry::is_gemini_image_model(&job.model)
            && job.provider.base_url.contains("googleapis.com") =>
        {
            gemini_image(job).await
        }
        _ if job.source_image.is_some() => openai_image_edit(job).await,
        _ => openai_image(job).await,
    }
}

async fn image_bytes_from_url(client: &reqwest::Client, url: &str) -> anyhow::Result<Vec<u8>> {
    if let Some(rest) = url.strip_prefix("data:") {
        let b64 = rest
            .split_once("base64,")
            .map(|(_, d)| d)
            .ok_or_else(|| anyhow!("unsupported non-base64 data url in image response"))?;
        return base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .context("failed to decode image data url");
    }
    download_bytes(client, url).await
}

async fn openrouter_image(job: &MediaJob) -> anyhow::Result<Vec<u8>> {
    let key = job.require_key()?;
    let url = format!(
        "{}/chat/completions",
        job.provider.base_url.trim_end_matches('/')
    );

    let mut prompt_text = job.prompt.clone();
    if job.aspect != "1:1" {
        prompt_text.push_str(&format!("\n\nAspect ratio: {}", job.aspect));
    }
    let mut user_content: Vec<serde_json::Value> = Vec::new();
    if let Some(source_path) = job.source_image.as_deref() {
        let source_bytes = read_job_image(source_path, "source image").await?;
        if job.mask.is_some() {
            prompt_text = format!(
                "Edit the first image. The second image is a mask: WHITE areas mark the region to \
                 change, BLACK areas must stay identical to the original. Apply this edit ONLY \
                 inside the white mask region, blending edges naturally: {prompt_text}"
            );
        } else {
            prompt_text = format!(
                "Edit the provided image according to this instruction, preserving everything \
                 not mentioned: {prompt_text}"
            );
        }
        user_content.push(json!({ "type": "text", "text": prompt_text }));
        user_content.push(json!({
            "type": "image_url",
            "image_url": { "url": data_uri(&source_bytes) }
        }));
        if let Some(mask_path) = job.mask.as_deref() {
            let mask_bytes = read_job_image(mask_path, "mask").await?;
            user_content.push(json!({
                "type": "image_url",
                "image_url": { "url": data_uri(&mask_bytes) }
            }));
        }
    } else {
        user_content.push(json!({ "type": "text", "text": prompt_text }));
    }

    let body = json!({
        "model": job.model,
        "messages": [{ "role": "user", "content": user_content }],
        "modalities": ["image", "text"],
    });
    let resp = job
        .client
        .post(&url)
        .header("Authorization", format!("Bearer {key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("openrouter image request failed")?;
    let status = resp.status();
    let value: serde_json::Value = resp
        .json()
        .await
        .context("failed to parse openrouter image response")?;
    if !status.is_success() {
        return Err(anyhow!("openrouter image error ({status}): {value}"));
    }
    let img_url = value
        .pointer("/choices/0/message/images/0/image_url/url")
        .and_then(|v| v.as_str())
        .or_else(|| {
            value
                .pointer("/choices/0/message/images/0/url")
                .and_then(|v| v.as_str())
        })
        .ok_or_else(|| {
            anyhow!(
                "no image data in openrouter response (model '{}' may not support image \
                 output): {value}",
                job.model
            )
        })?;
    image_bytes_from_url(&job.client, img_url).await
}

fn sniff_mime(bytes: &[u8]) -> &'static str {
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
        "image/jpeg"
    } else if bytes.len() >= 12 && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "image/png"
    }
}

async fn read_job_image(path: &std::path::Path, what: &str) -> anyhow::Result<Vec<u8>> {
    tokio::fs::read(path)
        .await
        .with_context(|| format!("cannot read {what} file `{}`", path.display()))
}

fn data_uri(bytes: &[u8]) -> String {
    format!(
        "data:{};base64,{}",
        sniff_mime(bytes),
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

fn openai_mask_rgba(mask_bytes: &[u8], target_w: u32, target_h: u32) -> anyhow::Result<Vec<u8>> {
    let mask = image::load_from_memory(mask_bytes)
        .context("cannot decode mask image")?
        .to_luma8();
    let mask = if mask.width() == target_w && mask.height() == target_h {
        mask
    } else {
        image::imageops::resize(
            &mask,
            target_w,
            target_h,
            image::imageops::FilterType::Nearest,
        )
    };
    let mut rgba = image::RgbaImage::new(target_w, target_h);
    for (x, y, p) in mask.enumerate_pixels() {
        let alpha = 255u8.saturating_sub(p.0[0]);
        rgba.put_pixel(x, y, image::Rgba([0, 0, 0, alpha]));
    }
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(rgba)
        .write_to(&mut buf, image::ImageFormat::Png)
        .context("cannot encode converted mask")?;
    Ok(buf.into_inner())
}

fn parse_openai_image_response(
    status: reqwest::StatusCode,
    value: serde_json::Value,
) -> anyhow::Result<OpenAiImagePayload> {
    if !status.is_success() {
        return Err(anyhow!("image API error ({status}): {value}"));
    }
    if let Some(b64) = value.pointer("/data/0/b64_json").and_then(|v| v.as_str()) {
        return Ok(OpenAiImagePayload::Bytes(
            base64::engine::general_purpose::STANDARD
                .decode(b64)
                .context("failed to decode image base64")?,
        ));
    }
    if let Some(img_url) = first_string(&value, &["/data/0/url"]) {
        return Ok(OpenAiImagePayload::Url(img_url.to_string()));
    }
    Err(anyhow!("no image data in response: {value}"))
}

enum OpenAiImagePayload {
    Bytes(Vec<u8>),
    Url(String),
}

async fn openai_image(job: &MediaJob) -> anyhow::Result<Vec<u8>> {
    let key = job.require_key()?;
    let url = format!("{}/images/generations", job.provider.base_url.trim_end_matches('/'));
    let size = registry::aspect_to_openai_size(&job.aspect);
    let body = json!({
        "model": job.model,
        "prompt": job.prompt,
        "n": 1,
        "size": size,
    });
    let resp = job
        .client
        .post(&url)
        .header("Authorization", format!("Bearer {key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("image generation request failed")?;
    let status = resp.status();
    let value: serde_json::Value = resp
        .json()
        .await
        .context("failed to parse image response")?;
    match parse_openai_image_response(status, value)? {
        OpenAiImagePayload::Bytes(b) => Ok(b),
        OpenAiImagePayload::Url(u) => download_bytes(&job.client, &u).await,
    }
}

async fn openai_image_edit(job: &MediaJob) -> anyhow::Result<Vec<u8>> {
    let key = job.require_key()?;
    let source_path = job
        .source_image
        .as_deref()
        .ok_or_else(|| anyhow!("source_image required for image edit"))?;
    let source_bytes = read_job_image(source_path, "source image").await?;
    let source_mime = sniff_mime(&source_bytes);
    let model = if job.model.eq_ignore_ascii_case("dall-e-3") {
        "gpt-image-1".to_string()
    } else {
        job.model.clone()
    };
    let url = format!("{}/images/edits", job.provider.base_url.trim_end_matches('/'));

    let mut form = reqwest::multipart::Form::new()
        .text("model", model)
        .text("prompt", job.prompt.clone())
        .text("n", "1")
        .part(
            "image",
            reqwest::multipart::Part::bytes(source_bytes.clone())
                .file_name("source.png")
                .mime_str(source_mime)?,
        );
    if let Some(fid) = job
        .fidelity
        .as_deref()
        .map(str::trim)
        .filter(|f| f.eq_ignore_ascii_case("high") || f.eq_ignore_ascii_case("low"))
    {
        form = form.text("input_fidelity", fid.to_ascii_lowercase());
    }
    if let Some(mask_path) = job.mask.as_deref() {
        let mask_bytes = read_job_image(mask_path, "mask").await?;
        let source_for_dims = source_bytes.clone();
        let converted = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<u8>> {
            let (w, h) = image::load_from_memory(&source_for_dims)
                .context("cannot decode source image")
                .map(|img| (img.width(), img.height()))?;
            openai_mask_rgba(&mask_bytes, w, h)
        })
        .await
        .map_err(|e| anyhow!("mask conversion task panicked: {e}"))??;
        form = form.part(
            "mask",
            reqwest::multipart::Part::bytes(converted)
                .file_name("mask.png")
                .mime_str("image/png")?,
        );
    }

    let resp = job
        .client
        .post(&url)
        .header("Authorization", format!("Bearer {key}"))
        .multipart(form)
        .send()
        .await
        .context("image edit request failed")?;
    let status = resp.status();
    let value: serde_json::Value = resp
        .json()
        .await
        .context("failed to parse image edit response")?;
    match parse_openai_image_response(status, value)? {
        OpenAiImagePayload::Bytes(b) => Ok(b),
        OpenAiImagePayload::Url(u) => download_bytes(&job.client, &u).await,
    }
}

fn fal_strength(fidelity: Option<&str>) -> f64 {
    match fidelity.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("high") => 0.45,
        Some("low") => 0.9,
        _ => 0.7,
    }
}

async fn fal_image(job: &MediaJob) -> anyhow::Result<Vec<u8>> {
    let key = job.require_key()?;
    let source = match job.source_image.as_deref() {
        Some(path) => Some(read_job_image(path, "source image").await?),
        None => None,
    };
    let (endpoint_model, body) = if let Some(source_bytes) = &source {
        if let Some(mask_path) = job.mask.as_deref() {
            let mask_bytes = read_job_image(mask_path, "mask").await?;
            (
                "fal-ai/flux-pro/v1/fill".to_string(),
                json!({
                    "prompt": job.prompt,
                    "image_url": data_uri(source_bytes),
                    "mask_url": data_uri(&mask_bytes),
                    "num_images": 1,
                }),
            )
        } else {
            (
                "fal-ai/flux/dev/image-to-image".to_string(),
                json!({
                    "prompt": job.prompt,
                    "image_url": data_uri(source_bytes),
                    "strength": fal_strength(job.fidelity.as_deref()),
                    "num_images": 1,
                }),
            )
        }
    } else {
        let image_size = job
            .resolution
            .as_deref()
            .and_then(|res| registry::aspect_to_pixels(&job.aspect, res))
            .map(|(w, h)| json!({ "width": w, "height": h }))
            .unwrap_or_else(|| json!(registry::aspect_to_fal_size(&job.aspect)));
        (
            job.model.clone(),
            json!({
                "prompt": job.prompt,
                "image_size": image_size,
                "num_images": 1,
            }),
        )
    };
    let url = format!("https://fal.run/{endpoint_model}");
    let resp = job
        .client
        .post(&url)
        .header("Authorization", format!("Key {key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("fal image request failed")?;
    let status = resp.status();
    let value: serde_json::Value = resp.json().await.context("failed to parse fal response")?;
    if !status.is_success() {
        return Err(anyhow!("fal image error ({status}): {value}"));
    }
    let img_url = first_string(&value, &["/images/0/url", "/image/url"])
        .ok_or_else(|| anyhow!("no image url in fal response: {value}"))?;
    download_bytes(&job.client, img_url).await
}

async fn gemini_image(job: &MediaJob) -> anyhow::Result<Vec<u8>> {
    let key = job.require_key()?;
    let base = job.provider.base_url.trim_end_matches('/');
    let url = format!("{base}/models/{}:generateContent", job.model);

    let mut parts: Vec<serde_json::Value> = Vec::new();
    let mut prompt_text = job.prompt.clone();
    if let Some(source_path) = job.source_image.as_deref() {
        let source_bytes = read_job_image(source_path, "source image").await?;
        if job.mask.is_some() {
            prompt_text = format!(
                "Edit the first image. The second image is a mask: WHITE areas mark the region to \
                 change, BLACK areas must stay pixel-identical to the original. Apply this edit \
                 ONLY inside the white mask region, blending edges naturally: {}",
                job.prompt
            );
        } else {
            prompt_text = format!(
                "Edit the provided image according to this instruction, preserving everything \
                 not mentioned: {}",
                job.prompt
            );
        }
        parts.push(json!({ "text": prompt_text }));
        parts.push(json!({
            "inline_data": {
                "mime_type": sniff_mime(&source_bytes),
                "data": base64::engine::general_purpose::STANDARD.encode(&source_bytes),
            }
        }));
        if let Some(mask_path) = job.mask.as_deref() {
            let mask_bytes = read_job_image(mask_path, "mask").await?;
            parts.push(json!({
                "inline_data": {
                    "mime_type": sniff_mime(&mask_bytes),
                    "data": base64::engine::general_purpose::STANDARD.encode(&mask_bytes),
                }
            }));
        }
    } else {
        parts.push(json!({ "text": prompt_text }));
    }

    let body = json!({
        "contents": [{ "parts": parts }],
        "generationConfig": { "responseModalities": ["TEXT", "IMAGE"] },
    });
    let resp = job
        .client
        .post(&url)
        .header("x-goog-api-key", key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("gemini image request failed")?;
    let status = resp.status();
    let value: serde_json::Value = resp
        .json()
        .await
        .context("failed to parse gemini response")?;
    if !status.is_success() {
        return Err(anyhow!("gemini image error ({status}): {value}"));
    }
    let inline = value
        .pointer("/candidates/0/content/parts")
        .and_then(|v| v.as_array())
        .and_then(|parts| {
            parts.iter().find_map(|p| {
                p.pointer("/inlineData/data")
                    .or_else(|| p.pointer("/inline_data/data"))
                    .and_then(|d| d.as_str())
            })
        })
        .ok_or_else(|| anyhow!("no image data in gemini response: {value}"))?;
    base64::engine::general_purpose::STANDARD
        .decode(inline)
        .context("failed to decode gemini image base64")
}
