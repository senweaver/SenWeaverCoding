// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use reqwest::header::{HeaderValue, CONTENT_TYPE};
use std::path::Path;

use super::{CloudPushTarget, PushOutcome, truncate_excerpt};
use crate::evolution::types::{CloudTarget, ExportRecord};

pub struct TinkerTarget;

#[async_trait]
impl CloudPushTarget for TinkerTarget {
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
        let client = crate::services::proxy::runtime::ProxyRuntime::global()
            .build_client_with_timeouts("evolution.tinker", 180, 10);
        let mut req = client
            .post(&target.endpoint)
            .header(CONTENT_TYPE, HeaderValue::from_static("application/x-ndjson"))
            .header(
                "x-tinker-format",
                HeaderValue::try_from(export.format.as_str()).unwrap_or(HeaderValue::from_static("unknown")),
            )
            .body(bytes);
        if let Some(token) = secret {
            req = req.bearer_auth(token);
        }
        for (k, v) in &target.headers {
            req = req.header(k, v);
        }
        let resp = req.send().await.context("tinker push failed")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(
                "tinker returned {}: {}",
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
