// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{anyhow, Context};
use serde_json::Value;
use std::time::{Duration, Instant};

pub async fn download_bytes(client: &reqwest::Client, url: &str) -> anyhow::Result<Vec<u8>> {
    if let Some(rest) = url.strip_prefix("data:") {
        if let Some(idx) = rest.find("base64,") {
            let b64 = &rest[idx + "base64,".len()..];
            use base64::Engine;
            return base64::engine::general_purpose::STANDARD
                .decode(b64.trim())
                .context("failed to decode data: URL base64");
        }
        return Err(anyhow!("unsupported data: URL (not base64)"));
    }
    let resp = client
        .get(url)
        .send()
        .await
        .context("failed to download generated media")?;
    if !resp.status().is_success() {
        return Err(anyhow!(
            "download failed ({}) for {url}",
            resp.status()
        ));
    }
    Ok(resp.bytes().await.context("failed to read media bytes")?.to_vec())
}

pub fn first_string<'a>(value: &'a Value, pointers: &[&str]) -> Option<&'a str> {
    for p in pointers {
        if let Some(s) = value.pointer(p).and_then(|v| v.as_str()) {
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    None
}

pub async fn poll_until<F>(
    client: &reqwest::Client,
    status_url: &str,
    auth_header: Option<(&str, String)>,
    interval: Duration,
    max: Duration,
    mut done: F,
) -> anyhow::Result<Value>
where
    F: FnMut(&Value) -> Option<anyhow::Result<Value>>,
{
    let start = Instant::now();
    loop {
        if start.elapsed() > max {
            return Err(anyhow!("media job timed out after {:?}", max));
        }
        let mut req = client.get(status_url);
        if let Some((name, value)) = auth_header.as_ref() {
            req = req.header(*name, value.clone());
        }
        let resp = req.send().await.context("media poll request failed")?;
        let status = resp.status();
        let body: Value = resp
            .json()
            .await
            .unwrap_or_else(|_| Value::Null);
        if !status.is_success() {
            return Err(anyhow!("media poll error ({status}): {body}"));
        }
        if let Some(result) = done(&body) {
            return result;
        }
        tokio::time::sleep(interval).await;
    }
}
