// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use reqwest::header::{HeaderValue, CONTENT_TYPE};
use std::path::Path;

use super::{CloudPushTarget, PushOutcome, truncate_excerpt};
use crate::evolution::types::{CloudTarget, ExportRecord};

pub struct HuggingfaceTarget;

#[async_trait]
impl CloudPushTarget for HuggingfaceTarget {
    async fn push(
        &self,
        target: &CloudTarget,
        secret: Option<&str>,
        export: &ExportRecord,
        file_path: &Path,
    ) -> Result<PushOutcome> {
        let token = secret.ok_or_else(|| anyhow!("missing_hf_token"))?;
        if target.endpoint.is_empty() {
            return Err(anyhow!("missing_endpoint"));
        }
        let bytes = tokio::fs::read(file_path)
            .await
            .with_context(|| format!("read export {}", file_path.display()))?;
        let file_name = file_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("export.jsonl");
        let upload_url = if target.endpoint.contains("{filename}") {
            target.endpoint.replace("{filename}", file_name)
        } else if target.endpoint.ends_with('/') {
            format!("{}{}", target.endpoint, file_name)
        } else {
            format!("{}/{}", target.endpoint, file_name)
        };
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(180))
            .build()?;
        let _ = export;
        let resp = client
            .put(&upload_url)
            .bearer_auth(token)
            .header(CONTENT_TYPE, HeaderValue::from_static("application/x-ndjson"))
            .body(bytes)
            .send()
            .await
            .context("huggingface push failed")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(
                "huggingface returned {}: {}",
                status,
                truncate_excerpt(&text, 240).unwrap_or_default()
            ));
        }
        Ok(PushOutcome {
            status: status.as_u16().to_string(),
            response_excerpt: truncate_excerpt(&text, 240),
        })
    }
}
