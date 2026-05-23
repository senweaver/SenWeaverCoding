// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::memory::ivf_index::IvfVectorIndex;
use crate::memory::vector_index::VectorIndex;

use super::embedding::EmbeddingProvider;

#[derive(Debug, Clone)]
pub struct VectorCodeIndexConfig {

    pub num_clusters: usize,

    pub nprobe: usize,
}

impl Default for VectorCodeIndexConfig {
    fn default() -> Self {
        Self {
            num_clusters: 128,
            nprobe: 8,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CodeChunk {
    pub id: String,
    pub path: PathBuf,
    pub start_line: u32,
    pub end_line: u32,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct VectorCodeHit {
    pub id: String,
    pub path: PathBuf,
    pub start_line: u32,
    pub end_line: u32,
    pub snippet: String,
    pub score: f32,
}

pub struct VectorCodeIndex {
    embedder: Box<dyn EmbeddingProvider>,
    index: Mutex<IvfVectorIndex>,
    chunks: Mutex<HashMap<String, CodeChunk>>,
}

impl VectorCodeIndex {
    pub fn new(embedder: Box<dyn EmbeddingProvider>, cfg: VectorCodeIndexConfig) -> Self {
        let index = IvfVectorIndex::new(cfg.num_clusters, cfg.nprobe);
        Self {
            embedder,
            index: Mutex::new(index),
            chunks: Mutex::new(HashMap::new()),
        }
    }

    pub fn dimensions(&self) -> usize {
        self.embedder.dimensions()
    }

    pub async fn len(&self) -> usize {
        self.chunks.lock().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.chunks.lock().await.is_empty()
    }

    pub async fn upsert_chunk(&self, chunk: CodeChunk) -> anyhow::Result<()> {
        let dims = self.embedder.dimensions();
        if dims == 0 {
            anyhow::bail!("vector index disabled: embedder has zero dimensions");
        }
        let vec = self.embedder.embed_one(&chunk.content).await?;
        if vec.is_empty() {
            anyhow::bail!("embedder returned empty vector for chunk {}", chunk.id);
        }
        let mut index = self.index.lock().await;
        index.upsert(&chunk.id, &vec);
        let mut chunks = self.chunks.lock().await;
        chunks.insert(chunk.id.clone(), chunk);
        Ok(())
    }

    pub async fn upsert_chunks(&self, chunks: Vec<CodeChunk>) -> anyhow::Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }
        let texts: Vec<&str> = chunks.iter().map(|c| c.content.as_str()).collect();
        let vecs = self.embedder.embed(&texts).await?;
        if vecs.len() != chunks.len() {
            anyhow::bail!(
                "embedder returned {got} vectors for {expected} inputs",
                got = vecs.len(),
                expected = chunks.len()
            );
        }
        let mut index = self.index.lock().await;
        let mut store = self.chunks.lock().await;
        for (chunk, vec) in chunks.into_iter().zip(vecs.into_iter()) {
            index.upsert(&chunk.id, &vec);
            store.insert(chunk.id.clone(), chunk);
        }
        Ok(())
    }

    pub async fn remove(&self, id: &str) {
        let mut index = self.index.lock().await;
        index.remove(id);
        let mut chunks = self.chunks.lock().await;
        chunks.remove(id);
    }

    pub async fn search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<VectorCodeHit>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        if self.embedder.dimensions() == 0 {
            return Ok(Vec::new());
        }
        let qvec = self.embedder.embed_one(query).await?;
        if qvec.is_empty() {
            return Ok(Vec::new());
        }
        let raw = {
            let index = self.index.lock().await;
            index.search(&qvec, limit)
        };
        let chunks = self.chunks.lock().await;
        let mut out = Vec::with_capacity(raw.len());
        for (id, score) in raw {
            if let Some(chunk) = chunks.get(&id) {
                out.push(VectorCodeHit {
                    id: chunk.id.clone(),
                    path: chunk.path.clone(),
                    start_line: chunk.start_line,
                    end_line: chunk.end_line,
                    snippet: chunk.content.clone(),
                    score,
                });
            }
        }
        Ok(out)
    }
}

pub fn reciprocal_rank_fusion<T: Clone + Eq + std::hash::Hash>(
    rankings: &[Vec<T>],
    k: usize,
) -> Vec<(T, f32)> {
    let k = k.max(1) as f32;
    let mut scores: HashMap<T, f32> = HashMap::new();
    for ranking in rankings {
        for (idx, item) in ranking.iter().enumerate() {
            let entry = scores.entry(item.clone()).or_insert(0.0);
            *entry += 1.0 / (k + (idx as f32 + 1.0));
        }
    }
    let mut out: Vec<(T, f32)> = scores.into_iter().collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    out
}

pub type SharedVectorCodeIndex = Arc<VectorCodeIndex>;
