// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::embeddings::EmbeddingProvider;
use super::traits::{Memory, MemoryCategory, MemoryEntry};
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::OnceCell;
use uuid::Uuid;

pub struct QdrantMemory {
    client: reqwest::Client,
    base_url: String,
    collection: String,
    api_key: Option<String>,
    embedder: Arc<dyn EmbeddingProvider>,

    initialized: OnceCell<()>,
}

impl QdrantMemory {

    pub async fn new(
        url: &str,
        collection: &str,
        api_key: Option<String>,
        embedder: Arc<dyn EmbeddingProvider>,
    ) -> Result<Self> {
        let mem = Self::new_lazy(url, collection, api_key, embedder)?;

        mem.ensure_collection().await?;
        mem.initialized.set(()).ok();

        Ok(mem)
    }

    pub fn new_lazy(
        url: &str,
        collection: &str,
        api_key: Option<String>,
        embedder: Arc<dyn EmbeddingProvider>,
    ) -> Result<Self> {
        let parsed = reqwest::Url::parse(url.trim()).with_context(|| {
            format!(
                "invalid qdrant url '{url}'; expected http(s)://host[:port][/path] (e.g. http://localhost:6333)"
            )
        })?;
        if !matches!(parsed.scheme(), "http" | "https") {
            anyhow::bail!(
                "qdrant url scheme '{}' is unsupported; expected http or https",
                parsed.scheme()
            );
        }
        if parsed.host_str().map(str::is_empty).unwrap_or(true) {
            anyhow::bail!("qdrant url '{url}' is missing a host component");
        }

        let base_url = url.trim().trim_end_matches('/').to_string();
        let client = crate::services::require_services()
            .proxy_runtime()
            .build_client("memory.qdrant");

        Ok(Self {
            client,
            base_url,
            collection: collection.to_string(),
            api_key,
            embedder,
            initialized: OnceCell::new(),
        })
    }

    async fn ensure_initialized(&self) -> Result<()> {
        self.initialized
            .get_or_try_init(|| async {
                self.ensure_collection().await?;
                Ok::<(), anyhow::Error>(())
            })
            .await?;
        Ok(())
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.request(method, &url);

        if let Some(ref key) = self.api_key {
            req = req.header("api-key", key);
        }

        req.header("Content-Type", "application/json")
    }

    async fn send_with_retry(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<reqwest::Response> {
        let policy = crate::util::retry::RetryPolicy::http();
        crate::util::retry::retry(&policy, |attempt| {
            let method = method.clone();
            let path = path.to_string();
            let body = body.clone();
            async move {
                let mut req = self.request(method, &path);
                if let Some(b) = body {
                    req = req.json(&b);
                }
                match req.send().await {
                    Ok(resp) => {
                        let status = resp.status();
                        if status.is_server_error() || status.as_u16() == 429 {
                            anyhow::bail!(
                                "qdrant transient HTTP {} on attempt {attempt}",
                                status.as_u16()
                            );
                        }
                        Ok(resp)
                    }
                    Err(e) => Err(anyhow::Error::new(e)
                        .context(format!("qdrant request failed on attempt {attempt}"))),
                }
            }
        })
        .await
    }

    async fn ensure_collection(&self) -> Result<()> {
        let dims = self.embedder.dimensions();
        if dims == 0 {

            tracing::warn!(
                "Qdrant memory using noop embedder (0 dimensions); vector search disabled"
            );
            return Ok(());
        }

        let path = format!("/collections/{}", self.collection);
        let resp = self
            .send_with_retry(reqwest::Method::GET, &path, None)
            .await;

        match resp {
            Ok(r) if r.status().is_success() => {

                return Ok(());
            }
            Ok(r) if r.status().as_u16() == 404 => {

            }
            Ok(r) => {
                let status = r.status();
                let text = r.text().await.unwrap_or_default();
                anyhow::bail!("Qdrant collection check failed ({status}): {text}");
            }
            Err(e) => {
                anyhow::bail!("Qdrant connection failed: {e}");
            }
        }

        let create_body = serde_json::json!({
            "vectors": {
                "size": dims,
                "distance": "Cosine"
            }
        });

        let resp = self
            .send_with_retry(reqwest::Method::PUT, &path, Some(create_body))
            .await
            .context("failed to create Qdrant collection")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Qdrant collection creation failed ({status}): {text}");
        }

        tracing::info!(
            "Created Qdrant collection '{}' with {} dimensions",
            self.collection,
            dims
        );

        Ok(())
    }

    fn category_to_str(category: &MemoryCategory) -> String {
        match category {
            MemoryCategory::Core => "core".to_string(),
            MemoryCategory::Daily => "daily".to_string(),
            MemoryCategory::Conversation => "conversation".to_string(),
            MemoryCategory::Custom(name) => name.clone(),
        }
    }

    fn parse_category(value: &str) -> MemoryCategory {
        match value {
            "core" => MemoryCategory::Core,
            "daily" => MemoryCategory::Daily,
            "conversation" => MemoryCategory::Conversation,
            other => MemoryCategory::Custom(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MemoryPayload {
    key: String,
    content: String,
    category: String,
    timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QdrantSearchResult {
    result: Vec<QdrantScoredPoint>,
}

#[derive(Debug, Deserialize)]
struct QdrantScoredPoint {
    id: serde_json::Value,
    score: f64,
    payload: Option<MemoryPayload>,
}

#[derive(Debug, Deserialize)]
struct QdrantScrollResult {
    result: QdrantScrollPoints,
}

#[derive(Debug, Deserialize)]
struct QdrantScrollPoints {
    points: Vec<QdrantPoint>,
}

#[derive(Debug, Deserialize)]
struct QdrantPoint {
    id: serde_json::Value,
    payload: Option<MemoryPayload>,
}

#[async_trait]
impl Memory for QdrantMemory {
    fn name(&self) -> &str {
        "qdrant"
    }

    async fn store(
        &self,
        key: &str,
        content: &str,
        category: MemoryCategory,
        session_id: Option<&str>,
    ) -> Result<()> {
        self.ensure_initialized().await?;

        let combined_text = format!("{}\n{}", key, content);
        let embedding = self.embedder.embed_one(&combined_text).await?;

        if embedding.is_empty() {
            anyhow::bail!("Qdrant requires non-zero dimensional embeddings");
        }

        let id = Uuid::new_v4().to_string();
        let timestamp = Utc::now().to_rfc3339();

        let payload = MemoryPayload {
            key: key.to_string(),
            content: content.to_string(),
            category: Self::category_to_str(&category),
            timestamp,
            session_id: session_id.map(str::to_string),
        };

        let _ = self.forget(key).await;

        let upsert_body = serde_json::json!({
            "points": [{
                "id": id,
                "vector": embedding,
                "payload": payload
            }]
        });

        let resp = self
            .request(
                reqwest::Method::PUT,
                &format!("/collections/{}/points", self.collection),
            )
            .query(&[("wait", "true")])
            .json(&upsert_body)
            .send()
            .await
            .context("failed to upsert point to Qdrant")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Qdrant upsert failed ({status}): {text}");
        }

        Ok(())
    }

    async fn recall(
        &self,
        query: &str,
        limit: usize,
        session_id: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
    ) -> Result<Vec<MemoryEntry>> {
        if query.trim().is_empty() {
            let mut entries = self.list(None, session_id).await?;
            if let Some(s) = since {
                entries.retain(|e| e.timestamp.as_str() >= s);
            }
            if let Some(u) = until {
                entries.retain(|e| e.timestamp.as_str() <= u);
            }
            entries.truncate(limit);
            return Ok(entries);
        }

        self.ensure_initialized().await?;

        let embedding = self.embedder.embed_one(query).await?;

        if embedding.is_empty() {

            return self.list(None, session_id).await;
        }

        let filter = session_id.map(|sid| {
            serde_json::json!({
                "must": [{
                    "key": "session_id",
                    "match": { "value": sid }
                }]
            })
        });

        let mut search_body = serde_json::json!({
            "vector": embedding,
            "limit": limit,
            "with_payload": true
        });

        if let Some(f) = filter {
            search_body["filter"] = f;
        }

        let resp = self
            .request(
                reqwest::Method::POST,
                &format!("/collections/{}/points/search", self.collection),
            )
            .json(&search_body)
            .send()
            .await
            .context("failed to search Qdrant")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Qdrant search failed ({status}): {text}");
        }

        let result: QdrantSearchResult = resp.json().await?;

        let mut entries: Vec<MemoryEntry> = result
            .result
            .into_iter()
            .filter_map(|point| {
                let payload = point.payload?;
                let id = match &point.id {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    _ => return None,
                };

                Some(MemoryEntry {
                    id,
                    key: payload.key,
                    content: payload.content,
                    category: Self::parse_category(&payload.category),
                    timestamp: payload.timestamp,
                    session_id: payload.session_id,
                    score: Some(point.score),
                    namespace: "default".into(),
                    importance: None,
                    superseded_by: None,
                })
            })
            .collect();

        if let Some(s) = since {
            entries.retain(|e| e.timestamp.as_str() >= s);
        }
        if let Some(u) = until {
            entries.retain(|e| e.timestamp.as_str() <= u);
        }

        Ok(entries)
    }

    async fn get(&self, key: &str) -> Result<Option<MemoryEntry>> {
        self.ensure_initialized().await?;

        let scroll_body = serde_json::json!({
            "filter": {
                "must": [{
                    "key": "key",
                    "match": { "value": key }
                }]
            },
            "limit": 1,
            "with_payload": true
        });

        let resp = self
            .request(
                reqwest::Method::POST,
                &format!("/collections/{}/points/scroll", self.collection),
            )
            .json(&scroll_body)
            .send()
            .await
            .context("failed to scroll Qdrant")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Qdrant scroll failed ({status}): {text}");
        }

        let result: QdrantScrollResult = resp.json().await?;

        let entry = result.result.points.into_iter().next().and_then(|point| {
            let payload = point.payload?;
            let id = match &point.id {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                _ => return None,
            };

            Some(MemoryEntry {
                id,
                key: payload.key,
                content: payload.content,
                category: Self::parse_category(&payload.category),
                timestamp: payload.timestamp,
                session_id: payload.session_id,
                score: None,
                namespace: "default".into(),
                importance: None,
                superseded_by: None,
            })
        });

        Ok(entry)
    }

    async fn list(
        &self,
        category: Option<&MemoryCategory>,
        session_id: Option<&str>,
    ) -> Result<Vec<MemoryEntry>> {
        self.ensure_initialized().await?;

        let mut must_conditions = Vec::new();

        if let Some(cat) = category {
            must_conditions.push(serde_json::json!({
                "key": "category",
                "match": { "value": Self::category_to_str(cat) }
            }));
        }

        if let Some(sid) = session_id {
            must_conditions.push(serde_json::json!({
                "key": "session_id",
                "match": { "value": sid }
            }));
        }

        let mut scroll_body = serde_json::json!({
            "limit": 1000,
            "with_payload": true
        });

        if !must_conditions.is_empty() {
            scroll_body["filter"] = serde_json::json!({ "must": must_conditions });
        }

        let resp = self
            .request(
                reqwest::Method::POST,
                &format!("/collections/{}/points/scroll", self.collection),
            )
            .json(&scroll_body)
            .send()
            .await
            .context("failed to scroll Qdrant")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Qdrant scroll failed ({status}): {text}");
        }

        let result: QdrantScrollResult = resp.json().await?;

        let entries = result
            .result
            .points
            .into_iter()
            .filter_map(|point| {
                let payload = point.payload?;
                let id = match &point.id {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    _ => return None,
                };

                Some(MemoryEntry {
                    id,
                    key: payload.key,
                    content: payload.content,
                    category: Self::parse_category(&payload.category),
                    timestamp: payload.timestamp,
                    session_id: payload.session_id,
                    score: None,
                    namespace: "default".into(),
                    importance: None,
                    superseded_by: None,
                })
            })
            .collect();

        Ok(entries)
    }

    async fn forget(&self, key: &str) -> Result<bool> {
        self.ensure_initialized().await?;

        let delete_body = serde_json::json!({
            "filter": {
                "must": [{
                    "key": "key",
                    "match": { "value": key }
                }]
            }
        });

        let resp = self
            .request(
                reqwest::Method::POST,
                &format!("/collections/{}/points/delete", self.collection),
            )
            .query(&[("wait", "true")])
            .json(&delete_body)
            .send()
            .await
            .context("failed to delete from Qdrant")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Qdrant delete failed ({status}): {text}");
        }

        Ok(true)
    }

    async fn count(&self) -> Result<usize> {
        self.ensure_initialized().await?;

        let resp = self
            .request(
                reqwest::Method::GET,
                &format!("/collections/{}", self.collection),
            )
            .send()
            .await
            .context("failed to get Qdrant collection info")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Qdrant collection info failed ({status}): {text}");
        }

        let json: serde_json::Value = resp.json().await?;

        let count = json
            .get("result")
            .and_then(|r| r.get("points_count"))
            .and_then(|c| c.as_u64())
            .unwrap_or(0);

        let count =
            usize::try_from(count).context("Qdrant returned a points count that exceeds usize")?;
        Ok(count)
    }

    async fn health_check(&self) -> bool {
        let resp = self.request(reqwest::Method::GET, "/").send().await;

        matches!(resp, Ok(r) if r.status().is_success())
    }
}
