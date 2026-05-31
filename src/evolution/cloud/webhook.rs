// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::path::Path;

use super::{CloudPushTarget, PushOutcome, truncate_excerpt};
use crate::evolution::types::{CloudTarget, ExportRecord};

pub struct WebhookTarget;

#[async_trait]
impl CloudPushTarget for WebhookTarget {
    async fn push(
        &self,
        target: &CloudTarget,
        secret: Option<&str>,
        export: &ExportRecord,
        file_path: &Path,
    ) -> Result<PushOutcome> {
        let bytes = tokio::fs::read(file_path)
            .await
            .with_context(|| format!("read export {}", file_path.display()))?;
        let mut headers = HeaderMap::new();
        for (k, v) in &target.headers {
            if let (Ok(name), Ok(value)) = (HeaderName::try_from(k.as_str()), HeaderValue::try_from(v.as_str())) {
                headers.insert(name, value);
            }
        }
        if let Some(token) = secret {
            if let Ok(value) = HeaderValue::try_from(format!("Bearer {token}")) {
                headers.insert(reqwest::header::AUTHORIZATION, value);
            }
        }
        if !headers.contains_key(reqwest::header::CONTENT_TYPE) {
            headers.insert(
                reqwest::header::CONTENT_TYPE,
                HeaderValue::from_static("application/x-ndjson"),
            );
        }
        headers.insert(
            "x-evolution-format",
            HeaderValue::try_from(export.format.as_str()).unwrap_or(HeaderValue::from_static("unknown")),
        );
        let client = crate::services::proxy::runtime::ProxyRuntime::global()
            .build_client_with_timeouts("evolution.webhook", 60, 10);
        let resp = client
            .post(&target.endpoint)
            .headers(headers)
            .body(bytes)
            .send()
            .await
            .context("webhook push failed")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "webhook returned {}: {}",
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
