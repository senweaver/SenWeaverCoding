// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use reqwest::multipart::{Form, Part};
use std::path::Path;

use super::{CloudPushTarget, PushOutcome, truncate_excerpt};
use crate::evolution::types::{CloudTarget, ExportRecord};

pub struct RlDatasetServerTarget;

#[async_trait]
impl CloudPushTarget for RlDatasetServerTarget {
    async fn push(
        &self,
        target: &CloudTarget,
        secret: Option<&str>,
        export: &ExportRecord,
        file_path: &Path,
    ) -> Result<PushOutcome> {
        if target.endpoint.is_empty() {
            return Err(anyhow!("missing_endpoint"));
        }
        let bytes = tokio::fs::read(file_path)
            .await
            .with_context(|| format!("read export {}", file_path.display()))?;
        let file_name = file_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("export.jsonl")
            .to_string();
        let part = Part::bytes(bytes)
            .file_name(file_name)
            .mime_str("application/x-ndjson")?;
        let form = Form::new()
            .part("file", part)
            .text("format", export.format.as_str())
            .text("sample_count", export.sample_count.to_string())
            .text("content_digest", export.md5.clone())
            .text("digest_algorithm", "md5");
        let client = crate::services::proxy::runtime::ProxyRuntime::global()
            .build_client_with_timeouts("evolution.rl_dataset_server", 180, 10);
        let mut req = client.post(&target.endpoint).multipart(form);
        if let Some(token) = secret {
            req = req.bearer_auth(token);
        }
        for (k, v) in &target.headers {
            req = req.header(k, v);
        }
        let resp = req.send().await.context("rl dataset server push failed")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(
                "rl dataset server returned {}: {}",
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
