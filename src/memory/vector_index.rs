// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

pub trait VectorIndex: Send + Sync {

    fn upsert(&mut self, id: &str, embedding: &[f32]);

    fn remove(&mut self, id: &str);

    fn search(&self, query: &[f32], limit: usize) -> Vec<(String, f32)>;

    fn len(&self) -> usize;

    fn backend_name(&self) -> &'static str;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub struct LinearIndex {

    ids: Vec<String>,
    embeddings: Vec<Vec<f32>>,
    norms: Vec<f32>,
}

impl LinearIndex {
    pub fn new() -> Self {
        Self {
            ids: Vec::new(),
            embeddings: Vec::new(),
            norms: Vec::new(),
        }
    }

    fn compute_norm(v: &[f32]) -> f32 {
        v.iter().map(|x| x * x).sum::<f32>().sqrt()
    }

    fn find_position(&self, id: &str) -> Option<usize> {
        self.ids.iter().position(|existing| existing == id)
    }
}

impl Default for LinearIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl VectorIndex for LinearIndex {
    fn upsert(&mut self, id: &str, embedding: &[f32]) {
        let norm = Self::compute_norm(embedding);
        match self.find_position(id) {
            Some(idx) => {
                self.embeddings[idx] = embedding.to_vec();
                self.norms[idx] = norm;
            }
            None => {
                self.ids.push(id.to_string());
                self.embeddings.push(embedding.to_vec());
                self.norms.push(norm);
            }
        }
    }

    fn remove(&mut self, id: &str) {
        if let Some(idx) = self.find_position(id) {
            self.ids.swap_remove(idx);
            self.embeddings.swap_remove(idx);
            self.norms.swap_remove(idx);
        }
    }

    fn search(&self, query: &[f32], limit: usize) -> Vec<(String, f32)> {
        if limit == 0 || self.ids.is_empty() {
            return Vec::new();
        }

        let query_norm = Self::compute_norm(query);
        if query_norm < f32::EPSILON {
            return Vec::new();
        }

        let mut heap: BinaryHeap<Reverse<(ordered_float::OrderedFloat<f32>, usize)>> =
            BinaryHeap::with_capacity(limit + 1);
        let mut min_sim: f32 = 0.0;

        for (idx, emb) in self.embeddings.iter().enumerate() {
            let dot: f32 = query.iter().zip(emb.iter()).map(|(a, b)| a * b).sum();
            let emb_norm = self.norms[idx];
            if emb_norm < f32::EPSILON {
                continue;
            }
            let sim = dot / (query_norm * emb_norm);
            if heap.len() >= limit && sim <= min_sim {
                continue;
            }
            heap.push(Reverse((ordered_float::OrderedFloat(sim), idx)));
            if heap.len() > limit {
                heap.pop();
                if let Some(Reverse((of, _))) = heap.peek() {
                    min_sim = of.0;
                }
            }
        }

        let mut out: Vec<(String, f32)> = heap
            .into_iter()
            .map(|Reverse((of, idx))| (self.ids[idx].clone(), of.0))
            .collect();
        out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        out
    }

    fn len(&self) -> usize {
        self.ids.len()
    }

    fn backend_name(&self) -> &'static str {
        "linear"
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum VectorBackend {
    Linear,
    Sharded,
    Ivf,
}

impl Default for VectorBackend {
    fn default() -> Self {
        VectorBackend::Ivf
    }
}

impl VectorBackend {

    pub fn from_str_lenient(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "linear" | "brute" | "bruteforce" => Some(Self::Linear),
            "sharded" | "shard" | "parallel" => Some(Self::Sharded),
            "ivf" | "inverted" | "clustered" | "default" => Some(Self::Ivf),
            _ => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Sharded => "sharded",
            Self::Ivf => "ivf",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Linear => "brute-force cosine (best for < 1k vectors)",
            Self::Sharded => "parallel fan-out across CPU shards (~10k sweet spot)",
            Self::Ivf => "inverted-file clustering, sqrt(N) probe (default, ~100k+)",
        }
    }

    pub const fn all() -> &'static [Self] {
        &[Self::Linear, Self::Sharded, Self::Ivf]
    }
}

pub fn build_backend(kind: VectorBackend) -> Box<dyn VectorIndex> {
    match kind {
        VectorBackend::Linear => Box::new(LinearIndex::new()),
        VectorBackend::Sharded => {
            Box::new(crate::memory::sharded_index::ShardedVectorIndex::with_cpu_count())
        }
        VectorBackend::Ivf => Box::new(crate::memory::ivf_index::IvfVectorIndex::for_size(10_000)),
    }
}

pub fn build_default_backend() -> Box<dyn VectorIndex> {
    let kind = crate::services::try_get_services()
        .map(|svc| {
            let cfg = svc.shared_config.load();
            cfg.memory.vector_backend.unwrap_or_default()
        })
        .unwrap_or_default();
    build_backend(kind)
}

pub struct SqliteVecIndex {
    conn: std::sync::Arc<parking_lot::Mutex<rusqlite::Connection>>,
    dim: usize,
}

impl SqliteVecIndex {

    pub fn attach(
        conn: std::sync::Arc<parking_lot::Mutex<rusqlite::Connection>>,
        dim: usize,
    ) -> anyhow::Result<Self> {
        {
            let c = conn.lock();
            c.execute_batch(
                "CREATE TABLE IF NOT EXISTS vec_memories (
                    id         TEXT PRIMARY KEY,
                    embedding  BLOB NOT NULL,
                    norm       REAL NOT NULL,
                    updated_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_vec_memories_updated
                    ON vec_memories(updated_at);
                CREATE TABLE IF NOT EXISTS vec_migration_state (
                    key        TEXT PRIMARY KEY,
                    value      TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );",
            )?;
        }
        Ok(Self { conn, dim })
    }

    pub fn dim(&self) -> usize {
        self.dim
    }

    pub fn is_migration_complete(&self) -> bool {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT value FROM vec_migration_state WHERE key = 'legacy_memories'",
            [],
            |row| row.get::<_, String>(0),
        )
        .map(|v| v == "done")
        .unwrap_or(false)
    }

    fn mark_migration_done(&self) {
        let conn = self.conn.lock();
        let now = chrono::Utc::now().to_rfc3339();
        let _ = conn.execute(
            "INSERT INTO vec_migration_state (key, value, updated_at) \
             VALUES ('legacy_memories', 'done', ?1) \
             ON CONFLICT(key) DO UPDATE SET value = 'done', updated_at = ?1",
            rusqlite::params![now],
        );
    }

    pub fn migrate_from_legacy_memories(&self) -> anyhow::Result<(usize, usize)> {
        if self.is_migration_complete() {
            return Ok((0, 0));
        }

        let conn = self.conn.lock();

        let legacy_exists: bool = conn
            .query_row(
                "SELECT name FROM sqlite_master \
                 WHERE type='table' AND name='memories'",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);
        if !legacy_exists {

            drop(conn);
            self.mark_migration_done();
            return Ok((0, 0));
        }

        let mut col_probe = conn.prepare("PRAGMA table_info(memories)")?;
        let has_embedding = col_probe
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(Result::ok)
            .any(|name| name == "embedding");
        drop(col_probe);
        if !has_embedding {
            drop(conn);
            self.mark_migration_done();
            return Ok((0, 0));
        }

        let mut stmt =
            conn.prepare("SELECT id, embedding FROM memories WHERE embedding IS NOT NULL")?;
        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let blob: Vec<u8> = row.get(1)?;
                Ok((id, blob))
            })?
            .filter_map(Result::ok)
            .collect();
        drop(stmt);

        let mut existing_stmt = conn.prepare("SELECT id FROM vec_memories")?;
        use std::collections::HashSet;
        let existing: HashSet<String> = existing_stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .filter_map(Result::ok)
            .collect();
        drop(existing_stmt);

        let now = chrono::Utc::now().to_rfc3339();
        let mut migrated = 0usize;
        let mut skipped = 0usize;

        {
            let tx = conn.unchecked_transaction()?;
            {
                let mut insert_stmt = tx.prepare(
                    "INSERT OR REPLACE INTO vec_memories (id, embedding, norm, updated_at) \
                     VALUES (?1, ?2, ?3, ?4)",
                )?;

                for (id, blob) in rows {
                    if existing.contains(&id) {
                        skipped += 1;
                        continue;
                    }
                    let emb = Self::decode_embedding(&blob);
                    if emb.is_empty() {
                        skipped += 1;
                        continue;
                    }
                    let norm = Self::compute_norm(&emb);
                    insert_stmt.execute(rusqlite::params![id, blob, norm as f64, now,])?;
                    migrated += 1;
                }
            }
            tx.commit()?;
        }

        drop(conn);
        self.mark_migration_done();
        Ok((migrated, skipped))
    }

    fn encode_embedding(v: &[f32]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(v.len() * 4);
        for x in v {
            bytes.extend_from_slice(&x.to_le_bytes());
        }
        bytes
    }

    fn decode_embedding(bytes: &[u8]) -> Vec<f32> {
        let mut out = Vec::with_capacity(bytes.len() / 4);
        for chunk in bytes.chunks_exact(4) {
            let mut buf = [0u8; 4];
            buf.copy_from_slice(chunk);
            out.push(f32::from_le_bytes(buf));
        }
        out
    }

    fn compute_norm(v: &[f32]) -> f32 {
        v.iter().map(|x| x * x).sum::<f32>().sqrt()
    }
}

impl VectorIndex for SqliteVecIndex {
    fn upsert(&mut self, id: &str, embedding: &[f32]) {
        let bytes = Self::encode_embedding(embedding);
        let norm = Self::compute_norm(embedding);
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        let _ = conn.execute(
            "INSERT INTO vec_memories (id, embedding, norm, updated_at) \
             VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT(id) DO UPDATE SET embedding = excluded.embedding, \
             norm = excluded.norm, updated_at = excluded.updated_at",
            rusqlite::params![id, bytes, norm as f64, now],
        );
    }

    fn remove(&mut self, id: &str) {
        let conn = self.conn.lock();
        let _ = conn.execute("DELETE FROM vec_memories WHERE id = ?1", [id]);
    }

    fn search(&self, query: &[f32], limit: usize) -> Vec<(String, f32)> {
        if limit == 0 {
            return Vec::new();
        }
        let query_norm = Self::compute_norm(query);
        if query_norm < f32::EPSILON {
            return Vec::new();
        }

        let conn = self.conn.lock();
        let mut stmt = match conn.prepare("SELECT id, embedding, norm FROM vec_memories") {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = match stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            let norm: f64 = row.get(2)?;
            Ok((id, blob, norm as f32))
        }) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };

        use std::cmp::Reverse;
        use std::collections::BinaryHeap;
        let mut heap: BinaryHeap<Reverse<(ordered_float::OrderedFloat<f32>, String)>> =
            BinaryHeap::with_capacity(limit + 1);
        let mut min_sim: f32 = 0.0;

        for row in rows.flatten() {
            let (id, bytes, emb_norm) = row;
            if emb_norm < f32::EPSILON {
                continue;
            }
            let emb = Self::decode_embedding(&bytes);
            let dot: f32 = query.iter().zip(emb.iter()).map(|(a, b)| a * b).sum();
            let sim = dot / (query_norm * emb_norm);

            if heap.len() >= limit && sim <= min_sim {
                continue;
            }
            heap.push(Reverse((ordered_float::OrderedFloat(sim), id)));
            if heap.len() > limit {
                heap.pop();
                if let Some(Reverse((of, _))) = heap.peek() {
                    min_sim = of.0;
                }
            }
        }

        let mut out: Vec<(String, f32)> = heap
            .into_iter()
            .map(|Reverse((of, id))| (id, of.0))
            .collect();
        out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        out
    }

    fn len(&self) -> usize {
        let conn = self.conn.lock();
        conn.query_row("SELECT COUNT(*) FROM vec_memories", [], |row| {
            row.get::<_, i64>(0)
        })
        .map(|n| n as usize)
        .unwrap_or(0)
    }

    fn backend_name(&self) -> &'static str {
        "sqlite_persistent"
    }
}
