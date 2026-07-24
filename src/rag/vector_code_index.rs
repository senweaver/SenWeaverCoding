// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::memory::ivf_index::IvfVectorIndex;
use crate::memory::vector::index::VectorIndex;

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

#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedChunk {
    id: String,
    path: String,
    start_line: u32,
    end_line: u32,
    content: String,
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

const EMBED_CACHE_CAP: usize = 8_192;

struct EmbedCache {
    map: HashMap<[u8; 32], Vec<f32>>,
    order: std::collections::VecDeque<[u8; 32]>,
}

impl EmbedCache {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            order: std::collections::VecDeque::new(),
        }
    }

    fn get(&self, key: &[u8; 32]) -> Option<Vec<f32>> {
        self.map.get(key).cloned()
    }

    fn insert(&mut self, key: [u8; 32], vec: Vec<f32>) {
        if self.map.insert(key, vec).is_none() {
            self.order.push_back(key);
            while self.order.len() > EMBED_CACHE_CAP {
                if let Some(old) = self.order.pop_front() {
                    self.map.remove(&old);
                }
            }
        }
    }
}

fn content_hash(content: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hasher.finalize().into()
}

pub struct VectorCodeIndex {
    embedder: Box<dyn EmbeddingProvider>,
    index: Mutex<IvfVectorIndex>,
    chunks: Mutex<HashMap<String, CodeChunk>>,
    embed_cache: Mutex<EmbedCache>,
}

impl VectorCodeIndex {
    pub fn new(embedder: Box<dyn EmbeddingProvider>, cfg: VectorCodeIndexConfig) -> Self {
        let index = IvfVectorIndex::new(cfg.num_clusters, cfg.nprobe);
        Self {
            embedder,
            index: Mutex::new(index),
            chunks: Mutex::new(HashMap::new()),
            embed_cache: Mutex::new(EmbedCache::new()),
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
        let key = content_hash(&chunk.content);
        let cached = self.embed_cache.lock().await.get(&key);
        let vec = match cached {
            Some(v) => v,
            None => {
                let v = self.embedder.embed_one(&chunk.content).await?;
                if v.is_empty() {
                    anyhow::bail!("embedder returned empty vector for chunk {}", chunk.id);
                }
                self.embed_cache.lock().await.insert(key, v.clone());
                v
            }
        };
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
        let mut resolved: Vec<Option<Vec<f32>>> = Vec::with_capacity(chunks.len());
        let mut miss_indices: Vec<usize> = Vec::new();
        {
            let cache = self.embed_cache.lock().await;
            for (i, chunk) in chunks.iter().enumerate() {
                match cache.get(&content_hash(&chunk.content)) {
                    Some(v) => resolved.push(Some(v)),
                    None => {
                        resolved.push(None);
                        miss_indices.push(i);
                    }
                }
            }
        }
        if !miss_indices.is_empty() {
            let texts: Vec<&str> = miss_indices
                .iter()
                .map(|&i| chunks[i].content.as_str())
                .collect();
            let vecs = self.embedder.embed(&texts).await?;
            if vecs.len() != miss_indices.len() {
                anyhow::bail!(
                    "embedder returned {got} vectors for {expected} inputs",
                    got = vecs.len(),
                    expected = miss_indices.len()
                );
            }
            let mut cache = self.embed_cache.lock().await;
            for (&i, vec) in miss_indices.iter().zip(vecs.into_iter()) {
                cache.insert(content_hash(&chunks[i].content), vec.clone());
                resolved[i] = Some(vec);
            }
        }
        let mut index = self.index.lock().await;
        let mut store = self.chunks.lock().await;
        for (chunk, vec) in chunks.into_iter().zip(resolved.into_iter()) {
            let Some(vec) = vec else { continue };
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

    pub async fn remove_path(&self, path: &std::path::Path) {
        let key = norm_path_key(path);
        let prefix = format!("{key}/");
        let ids: Vec<String> = {
            let chunks = self.chunks.lock().await;
            chunks
                .values()
                .filter(|c| {
                    let ck = norm_path_key(&c.path);
                    ck == key || ck.starts_with(&prefix)
                })
                .map(|c| c.id.clone())
                .collect()
        };
        if ids.is_empty() {
            return;
        }
        let mut index = self.index.lock().await;
        let mut chunks = self.chunks.lock().await;
        for id in ids {
            index.remove(&id);
            chunks.remove(&id);
        }
    }

    pub async fn contains_same_content(&self, id: &str, content: &str) -> bool {
        let chunks = self.chunks.lock().await;
        chunks.get(id).is_some_and(|c| c.content == content)
    }

    pub async fn chunk_ids_for_path(&self, path: &std::path::Path) -> Vec<String> {
        let key = norm_path_key(path);
        let chunks = self.chunks.lock().await;
        chunks
            .values()
            .filter(|c| norm_path_key(&c.path) == key)
            .map(|c| c.id.clone())
            .collect()
    }

    pub async fn indexed_paths(&self) -> Vec<PathBuf> {
        let chunks = self.chunks.lock().await;
        let unique: std::collections::HashSet<PathBuf> =
            chunks.values().map(|c| c.path.clone()).collect();
        unique.into_iter().collect()
    }

    pub async fn remove_ids(&self, ids: &[String]) {
        if ids.is_empty() {
            return;
        }
        let mut index = self.index.lock().await;
        let mut chunks = self.chunks.lock().await;
        for id in ids {
            index.remove(id);
            chunks.remove(id);
        }
    }

    pub async fn save_snapshot(&self, dir: &std::path::Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(dir)?;
        let ivf_path = dir.join("vector_index.ivf");
        let chunks_path = dir.join("vector_chunks.json");
        let meta_path = dir.join("vector_index.meta.json");
        {
            let index = self.index.lock().await;
            index.save_to_path(&ivf_path)?;
        }
        let meta = serde_json::json!({ "embedder": self.embedder.fingerprint() });
        std::fs::write(&meta_path, serde_json::to_vec(&meta)?)?;
        let persisted: Vec<PersistedChunk> = {
            let chunks = self.chunks.lock().await;
            chunks
                .values()
                .map(|c| PersistedChunk {
                    id: c.id.clone(),
                    path: c.path.to_string_lossy().into_owned(),
                    start_line: c.start_line,
                    end_line: c.end_line,
                    content: c.content.clone(),
                })
                .collect()
        };
        let tmp = chunks_path.with_extension("tmp");
        let body = serde_json::to_vec(&persisted)?;
        std::fs::write(&tmp, body)?;
        std::fs::rename(&tmp, &chunks_path)?;
        Ok(())
    }

    pub async fn load_snapshot(&self, dir: &std::path::Path) -> anyhow::Result<usize> {
        let ivf_path = dir.join("vector_index.ivf");
        let chunks_path = dir.join("vector_chunks.json");
        let meta_path = dir.join("vector_index.meta.json");
        if !ivf_path.is_file() || !chunks_path.is_file() {
            return Ok(0);
        }
        let mut fingerprint_verified = false;
        if let Ok(raw) = std::fs::read(&meta_path) {
            if let Ok(meta) = serde_json::from_slice::<serde_json::Value>(&raw) {
                let persisted_fp = meta.get("embedder").and_then(|v| v.as_str()).unwrap_or("");
                let current_fp = self.embedder.fingerprint();
                if !persisted_fp.is_empty() {
                    if persisted_fp != current_fp {
                        anyhow::bail!(
                            "persisted vector index was built with embedder '{persisted_fp}' but the current embedder is '{current_fp}'; ignoring snapshot so the index is rebuilt"
                        );
                    }
                    fingerprint_verified = true;
                }
            }
        }
        let loaded = IvfVectorIndex::load_from_path(&ivf_path)?;
        if !fingerprint_verified {
            let embedder_dims = self.embedder.dimensions();
            if embedder_dims != 0 {
                if let Some(dim) = loaded.dimensions() {
                    if dim != embedder_dims {
                        anyhow::bail!(
                            "persisted vector index dims {dim} do not match embedder dims {embedder_dims}; ignoring snapshot"
                        );
                    }
                }
            }
        }
        let body = std::fs::read(&chunks_path)?;
        let persisted: Vec<PersistedChunk> = serde_json::from_slice(&body)?;
        let index_ids: std::collections::HashSet<String> =
            loaded.entry_ids().into_iter().collect();
        let mut restored: HashMap<String, CodeChunk> = HashMap::with_capacity(persisted.len());
        for p in persisted {
            if !index_ids.contains(&p.id) {
                continue;
            }
            restored.insert(
                p.id.clone(),
                CodeChunk {
                    id: p.id,
                    path: PathBuf::from(p.path),
                    start_line: p.start_line,
                    end_line: p.end_line,
                    content: p.content,
                },
            );
        }
        let count = restored.len();
        {
            let mut cache = self.embed_cache.lock().await;
            for (id, emb) in loaded.iter_entries() {
                if let Some(chunk) = restored.get(id) {
                    cache.insert(content_hash(&chunk.content), emb.to_vec());
                }
            }
        }
        {
            let mut index = self.index.lock().await;
            *index = loaded;
        }
        {
            let mut chunks = self.chunks.lock().await;
            *chunks = restored;
        }
        Ok(count)
    }

    pub async fn search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<VectorCodeHit>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        if self.embedder.dimensions() == 0 {
            return Ok(Vec::new());
        }
        if self.chunks.lock().await.is_empty() {
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

fn norm_path_key(p: &std::path::Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        s.to_ascii_lowercase()
    } else {
        s
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
