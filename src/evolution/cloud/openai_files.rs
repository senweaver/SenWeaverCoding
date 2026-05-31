// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use reqwest::multipart::{Form, Part};
use std::path::Path;

use super::{CloudPushTarget, PushOutcome, truncate_excerpt};
use crate::evolution::types::{CloudTarget, ExportRecord};

pub struct OpenaiFilesTarget;

#[async_trait]
impl CloudPushTarget for OpenaiFilesTarget {
    async fn push(
        &self,
        target: &CloudTarget,
        secret: Option<&str>,
        export: &ExportRecord,
        file_path: &Path,
    ) -> Result<PushOutcome> {
        let token = secret.ok_or_else(|| anyhow!("missing_api_key"))?;
        let endpoint = if target.endpoint.is_empty() {
            "https://api.openai.com/v1/files".to_string()
        } else {
            target.endpoint.clone()
        };
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
            .text("purpose", "fine-tune");
        let _ = export;
        let client = crate::services::proxy::runtime::ProxyRuntime::global()
            .build_client_with_timeouts("evolution.openai_files", 120, 10);
        let resp = client
            .post(&endpoint)
            .bearer_auth(token)
            .multipart(form)
            .send()
            .await
            .context("openai files push failed")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(
                "openai files returned {}: {}",
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
