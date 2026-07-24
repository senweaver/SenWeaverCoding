// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use async_trait::async_trait;

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {

    fn name(&self) -> &str;

    fn dimensions(&self) -> usize;

    fn fingerprint(&self) -> String {
        format!("{}:{}", self.name(), self.dimensions())
    }

    async fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>>;

    async fn embed_one(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let mut results = self.embed(&[text]).await?;
        results
            .pop()
            .ok_or_else(|| anyhow::anyhow!("Empty embedding result"))
    }
}

pub struct NoopEmbedding;

#[async_trait]
impl EmbeddingProvider for NoopEmbedding {
    fn name(&self) -> &str {
        "none"
    }

    fn dimensions(&self) -> usize {
        0
    }

    async fn embed(&self, _texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        Ok(Vec::new())
    }
}

pub struct OpenAiEmbedding {
    base_url: String,
    api_key: String,
    model: String,
    dims: usize,
}

impl OpenAiEmbedding {
    pub fn new(base_url: &str, api_key: &str, model: &str, dims: usize) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            dims,
        }
    }

    fn http_client(&self) -> reqwest::Client {
        crate::services::require_services()
            .proxy_runtime()
            .build_client_with_timeouts("memory.embeddings", 120, 10)
    }

    fn has_explicit_api_path(&self) -> bool {
        let Ok(url) = reqwest::Url::parse(&self.base_url) else {
            return false;
        };

        let path = url.path().trim_end_matches('/');
        !path.is_empty() && path != "/"
    }

    fn has_embeddings_endpoint(&self) -> bool {
        let Ok(url) = reqwest::Url::parse(&self.base_url) else {
            return false;
        };

        url.path().trim_end_matches('/').ends_with("/embeddings")
    }

    fn embeddings_url(&self) -> String {
        if self.has_embeddings_endpoint() {
            return self.base_url.clone();
        }

        if self.has_explicit_api_path() {
            format!("{}/embeddings", self.base_url)
        } else {
            format!("{}/v1/embeddings", self.base_url)
        }
    }
}

#[async_trait]
impl EmbeddingProvider for OpenAiEmbedding {
    fn name(&self) -> &str {
        "openai"
    }

    fn dimensions(&self) -> usize {
        self.dims
    }

    fn fingerprint(&self) -> String {
        format!("openai:{}:{}", self.model, self.dims)
    }

    async fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let mut body = serde_json::json!({
            "model": self.model,
            "input": texts,
        });
        if self.model.starts_with("text-embedding-3") {
            body["dimensions"] = serde_json::json!(self.dims);
        }

        let policy = crate::util::retry::RetryPolicy::embedding();
        let resp = crate::util::retry::retry(&policy, |attempt| {
            let body = body.clone();
            let client = self.http_client();
            let url = self.embeddings_url();
            let api_key = self.api_key.clone();
            async move {
                let resp = client
                    .post(url)
                    .header("Authorization", format!("Bearer {api_key}"))
                    .header("Content-Type", "application/json")
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| {
                        anyhow::Error::new(e)
                            .context(format!("embedding request failed on attempt {attempt}"))
                    })?;
                let status = resp.status();
                if status.is_server_error() || status.as_u16() == 429 {
                    let text = resp.text().await.unwrap_or_default();
                    anyhow::bail!(
                        "Embedding API transient {status} on attempt {attempt}: {}",
                        text.chars().take(200).collect::<String>()
                    );
                }
                Ok::<reqwest::Response, anyhow::Error>(resp)
            }
        })
        .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Embedding API error {status}: {text}");
        }

        let json: serde_json::Value = resp.json().await?;
        let data = json
            .get("data")
            .and_then(|d| d.as_array())
            .ok_or_else(|| anyhow::anyhow!("Invalid embedding response: missing 'data'"))?;

        let mut embeddings = Vec::with_capacity(data.len());
        for item in data {
            let embedding = item
                .get("embedding")
                .and_then(|e| e.as_array())
                .ok_or_else(|| anyhow::anyhow!("Invalid embedding item"))?;

            #[allow(clippy::cast_possible_truncation)]
            let vec: Vec<f32> = embedding
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();

            embeddings.push(vec);
        }

        Ok(embeddings)
    }
}

pub struct CohereEmbedding {
    base_url: String,
    api_key: String,
    model: String,
    dims: usize,
    input_type: String,
}

impl CohereEmbedding {
    pub fn new(base_url: &str, api_key: &str, model: &str, dims: usize) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            dims,
            input_type: "search_document".to_string(),
        }
    }

    fn http_client(&self) -> reqwest::Client {
        crate::services::require_services()
            .proxy_runtime()
            .build_client_with_timeouts("memory.embeddings", 120, 10)
    }

    fn embed_url(&self) -> String {
        if self.base_url.trim_end_matches('/').ends_with("/embed") {
            return self.base_url.clone();
        }
        format!("{}/v1/embed", self.base_url)
    }
}

#[async_trait]
impl EmbeddingProvider for CohereEmbedding {
    fn name(&self) -> &str {
        "cohere"
    }

    fn dimensions(&self) -> usize {
        self.dims
    }

    fn fingerprint(&self) -> String {
        format!("cohere:{}:{}", self.model, self.dims)
    }

    async fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let body = serde_json::json!({
            "model": self.model,
            "texts": texts,
            "input_type": self.input_type,
        });

        let policy = crate::util::retry::RetryPolicy::embedding();
        let resp = crate::util::retry::retry(&policy, |attempt| {
            let body = body.clone();
            let client = self.http_client();
            let url = self.embed_url();
            let api_key = self.api_key.clone();
            async move {
                let resp = client
                    .post(url)
                    .header("Authorization", format!("Bearer {api_key}"))
                    .header("Content-Type", "application/json")
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| {
                        anyhow::Error::new(e)
                            .context(format!("cohere embedding request failed on attempt {attempt}"))
                    })?;
                let status = resp.status();
                if status.is_server_error() || status.as_u16() == 429 {
                    let text = resp.text().await.unwrap_or_default();
                    anyhow::bail!(
                        "Cohere embedding API transient {status} on attempt {attempt}: {}",
                        text.chars().take(200).collect::<String>()
                    );
                }
                Ok::<reqwest::Response, anyhow::Error>(resp)
            }
        })
        .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Cohere embedding API error {status}: {text}");
        }

        let json: serde_json::Value = resp.json().await?;
        let arrays = json
            .get("embeddings")
            .and_then(|e| {
                e.as_array()
                    .cloned()
                    .or_else(|| e.get("float").and_then(|f| f.as_array()).cloned())
            })
            .ok_or_else(|| anyhow::anyhow!("Invalid cohere embedding response: missing 'embeddings'"))?;

        let mut embeddings = Vec::with_capacity(arrays.len());
        for item in &arrays {
            let embedding = item
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("Invalid cohere embedding item"))?;

            #[allow(clippy::cast_possible_truncation)]
            let vec: Vec<f32> = embedding
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();

            embeddings.push(vec);
        }

        Ok(embeddings)
    }
}

pub fn create_embedding_provider(
    provider: &str,
    api_key: Option<&str>,
    model: &str,
    dims: usize,
) -> Box<dyn EmbeddingProvider> {
    match provider.trim() {
        "openai" => {
            let key = api_key.unwrap_or("");
            Box::new(OpenAiEmbedding::new(
                "https://api.openai.com",
                key,
                model,
                dims,
            ))
        }
        "openrouter" => {
            let key = api_key.unwrap_or("");
            Box::new(OpenAiEmbedding::new(
                "https://openrouter.ai/api/v1",
                key,
                model,
                dims,
            ))
        }
        "cohere" => {
            let key = api_key.unwrap_or("");
            Box::new(CohereEmbedding::new(
                "https://api.cohere.com",
                key,
                model,
                dims,
            ))
        }
        "ollama" => {
            let base = crate::util::get_runtime_var("OLLAMA_HOST")
                .or_else(|| crate::util::get_runtime_var("SEN_OLLAMA_HOST"))
                .map(|h| {
                    let h = h.trim().trim_end_matches('/');
                    if h.contains("://") {
                        format!("{h}/v1")
                    } else {
                        format!("http://{h}/v1")
                    }
                })
                .unwrap_or_else(|| "http://localhost:11434/v1".to_string());
            Box::new(OpenAiEmbedding::new(&base, api_key.unwrap_or(""), model, dims))
        }
        "none" => Box::new(NoopEmbedding),
        name if name.starts_with("custom:") => {
            let base_url = name.strip_prefix("custom:").unwrap_or("");
            let key = api_key.unwrap_or("");
            Box::new(OpenAiEmbedding::new(base_url, key, model, dims))
        }
        other => {
            tracing::warn!(
                "Unknown embedding provider '{}', falling back to noop (vector/semantic search disabled). Supported: none, openai, openrouter, cohere, ollama, custom:<url>. Run `sen doctor` for details.",
                other
            );
            Box::new(NoopEmbedding)
        }
    }
}
