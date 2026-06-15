// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::super::embeddings::EmbeddingProvider;
use super::super::traits::{ExportFilter, Memory, MemoryCategory, MemoryEntry};
use super::super::vector;
use super::super::vector::{VectorIndex, build_default_backend};
use crate::config::schema::SearchMode;
use anyhow::Context;
use async_trait::async_trait;
use chrono::Local;
use parking_lot::{Mutex, RwLock};
use rusqlite::{Connection, params};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use uuid::Uuid;

const SQLITE_OPEN_TIMEOUT_CAP_SECS: u64 = 300;

pub struct SqliteMemory {
    conn: Arc<Mutex<Connection>>,

    read_pool: Vec<Arc<Mutex<Connection>>>,
    read_pool_idx: std::sync::atomic::AtomicUsize,
    #[allow(dead_code)]
    db_path: PathBuf,
    embedder: Arc<dyn EmbeddingProvider>,
    vector_weight: f32,
    keyword_weight: f32,
    cache_max: usize,
    search_mode: SearchMode,

    vec_index: Arc<RwLock<VecIndexCache>>,
}

pub(crate) struct VecIndexCache {
    index: Option<Box<dyn VectorIndex>>,

    indexed_rows: i64,
}

impl VecIndexCache {
    fn empty() -> Self {
        Self {
            index: None,
            indexed_rows: 0,
        }
    }
}

const VEC_INDEX_REBUILD_THRESHOLD: i64 = 64;

const READ_POOL_SIZE: usize = 4;

impl SqliteMemory {
    pub fn new(workspace_dir: &Path) -> anyhow::Result<Self> {
        Self::with_embedder(
            workspace_dir,
            Arc::new(super::super::embeddings::NoopEmbedding),
            0.7,
            0.3,
            10_000,
            None,
            SearchMode::default(),
        )
    }

    pub fn new_named(workspace_dir: &Path, db_name: &str) -> anyhow::Result<Self> {
        let db_path = workspace_dir.join("memory").join(format!("{db_name}.db"));
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Self::open_connection(&db_path, None)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous  = NORMAL;
             PRAGMA busy_timeout = 5000;
             PRAGMA mmap_size    = 268435456;
             PRAGMA cache_size   = -65536;
             PRAGMA temp_store   = MEMORY;",
        )?;
        Self::init_schema(&conn)?;
        let read_pool = Self::open_read_pool(&db_path, READ_POOL_SIZE);
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            read_pool,
            read_pool_idx: std::sync::atomic::AtomicUsize::new(0),
            db_path,
            embedder: Arc::new(super::super::embeddings::NoopEmbedding),
            vector_weight: 0.7,
            keyword_weight: 0.3,
            cache_max: 10_000,
            search_mode: SearchMode::default(),
            vec_index: Arc::new(RwLock::new(VecIndexCache::empty())),
        })
    }

    pub fn with_embedder(
        workspace_dir: &Path,
        embedder: Arc<dyn EmbeddingProvider>,
        vector_weight: f32,
        keyword_weight: f32,
        cache_max: usize,
        open_timeout_secs: Option<u64>,
        search_mode: SearchMode,
    ) -> anyhow::Result<Self> {
        let db_path = workspace_dir.join("memory").join("brain.db");

        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Self::open_connection(&db_path, open_timeout_secs)?;

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous  = NORMAL;
             PRAGMA busy_timeout = 5000;
             PRAGMA mmap_size    = 268435456;
             PRAGMA cache_size   = -65536;
             PRAGMA temp_store   = MEMORY;",
        )?;

        Self::init_schema(&conn)?;
        let read_pool = Self::open_read_pool(&db_path, READ_POOL_SIZE);

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            read_pool,
            read_pool_idx: std::sync::atomic::AtomicUsize::new(0),
            db_path,
            embedder,
            vector_weight,
            keyword_weight,
            cache_max,
            search_mode,
            vec_index: Arc::new(RwLock::new(VecIndexCache::empty())),
        })
    }

    fn ensure_vec_index_fresh(
        vec_index: &Arc<RwLock<VecIndexCache>>,
        conn: &Connection,
    ) -> anyhow::Result<()> {
        let current_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memories WHERE embedding IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        {
            let cache = vec_index.read();
            if cache.index.is_some()
                && (current_rows - cache.indexed_rows).abs() < VEC_INDEX_REBUILD_THRESHOLD
            {
                return Ok(());
            }
        }

        let mut cache = vec_index.write();

        if cache.index.is_some()
            && (current_rows - cache.indexed_rows).abs() < VEC_INDEX_REBUILD_THRESHOLD
        {
            return Ok(());
        }

        let mut backend = build_default_backend();
        let mut stmt =
            conn.prepare("SELECT id, embedding FROM memories WHERE embedding IS NOT NULL")?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((id, blob))
        })?;
        let mut loaded = 0usize;
        for row in rows {
            let (id, blob) = row?;
            let emb = vector::bytes_to_vec(&blob);
            if !emb.is_empty() {
                backend.upsert(&id, &emb);
                loaded += 1;
            }
        }
        tracing::debug!(
            backend = backend.backend_name(),
            rows = loaded,
            "Rebuilt SqliteMemory vector index"
        );
        cache.index = Some(backend);
        cache.indexed_rows = current_rows;
        Ok(())
    }

    fn open_connection(
        db_path: &Path,
        open_timeout_secs: Option<u64>,
    ) -> anyhow::Result<Connection> {
        let path_buf = db_path.to_path_buf();

        let conn = if let Some(secs) = open_timeout_secs {
            let capped = secs.min(SQLITE_OPEN_TIMEOUT_CAP_SECS);
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                let result = Connection::open(&path_buf);
                let _ = tx.send(result);
            });
            match rx.recv_timeout(Duration::from_secs(capped)) {
                Ok(Ok(c)) => c,
                Ok(Err(e)) => return Err(e).context("SQLite failed to open database"),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    anyhow::bail!("SQLite connection open timed out after {} seconds", capped);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    anyhow::bail!("SQLite open thread exited unexpectedly");
                }
            }
        } else {
            Connection::open(&path_buf).context("SQLite failed to open database")?
        };

        Ok(conn)
    }

    fn open_read_pool(db_path: &Path, size: usize) -> Vec<Arc<Mutex<Connection>>> {
        (0..size)
            .filter_map(|_| {
                let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                    | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;
                match Connection::open_with_flags(db_path, flags) {
                    Ok(c) => {
                        let _ = c.execute_batch(
                            "PRAGMA mmap_size = 8388608; PRAGMA cache_size = -1000;",
                        );
                        Some(Arc::new(Mutex::new(c)))
                    }
                    Err(e) => {
                        tracing::warn!("Failed to open read-only SQLite connection: {e}");
                        None
                    }
                }
            })
            .collect()
    }

    pub fn read_conn(&self) -> Arc<Mutex<Connection>> {
        if self.read_pool.is_empty() {
            return self.conn.clone();
        }
        let idx = self
            .read_pool_idx
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            % self.read_pool.len();
        self.read_pool[idx].clone()
    }

    fn init_schema(conn: &Connection) -> anyhow::Result<()> {
        conn.execute_batch(
            "-- Schema version tracking
            CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);

            -- Core memories table
            CREATE TABLE IF NOT EXISTS memories (
                id          TEXT PRIMARY KEY,
                key         TEXT NOT NULL UNIQUE,
                content     TEXT NOT NULL,
                category    TEXT NOT NULL DEFAULT 'core',
                embedding   BLOB,
                embedding_norm REAL,
                created_at  TEXT NOT NULL,
                updated_at  TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_memories_category ON memories(category);
            CREATE INDEX IF NOT EXISTS idx_memories_key ON memories(key);

            -- FTS5 full-text search (BM25 scoring)
            CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
                key, content, content=memories, content_rowid=rowid
            );

            -- FTS5 triggers: keep in sync with memories table
            CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
                INSERT INTO memories_fts(rowid, key, content)
                VALUES (new.rowid, new.key, new.content);
            END;
            CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
                INSERT INTO memories_fts(memories_fts, rowid, key, content)
                VALUES ('delete', old.rowid, old.key, old.content);
            END;
            CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
                INSERT INTO memories_fts(memories_fts, rowid, key, content)
                VALUES ('delete', old.rowid, old.key, old.content);
                INSERT INTO memories_fts(rowid, key, content)
                VALUES (new.rowid, new.key, new.content);
            END;

            -- Embedding cache with LRU eviction
            CREATE TABLE IF NOT EXISTS embedding_cache (
                content_hash TEXT PRIMARY KEY,
                embedding    BLOB NOT NULL,
                created_at   TEXT NOT NULL,
                accessed_at  TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_cache_accessed ON embedding_cache(accessed_at);",
        )?;

        Self::run_migrations(conn)?;
        Ok(())
    }

    fn get_schema_version(conn: &Connection) -> anyhow::Result<i32> {
        let count: i32 =
            conn.query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))?;
        if count == 0 {
            return Ok(0);
        }
        let version: i32 = conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )?;
        Ok(version)
    }

    fn set_schema_version(conn: &Connection, version: i32) -> anyhow::Result<()> {
        conn.execute("DELETE FROM schema_version", [])?;
        conn.execute(
            "INSERT INTO schema_version (version) VALUES (?1)",
            [version],
        )?;
        Ok(())
    }

    fn run_migrations(conn: &Connection) -> anyhow::Result<()> {
        let current = Self::get_schema_version(conn)?;

        let baseline = if current == 0 {
            let schema_sql: String = conn
                .prepare("SELECT sql FROM sqlite_master WHERE type='table' AND name='memories'")
                .and_then(|mut stmt| stmt.query_row([], |row| row.get::<_, String>(0)))
                .unwrap_or_default();
            let mut v = 0i32;
            if schema_sql.contains("session_id") {
                v = 1;
            }
            if schema_sql.contains("namespace") {
                v = 2;
            }
            if schema_sql.contains("importance") {
                v = 3;
            }
            if schema_sql.contains("superseded_by") {
                v = 4;
            }
            if schema_sql.contains("embedding_norm") {
                v = v.max(6);
            }
            v
        } else {
            current
        };

        const MIGRATIONS: &[(i32, &str)] = &[
            (
                1,
                "ALTER TABLE memories ADD COLUMN session_id TEXT;
                 CREATE INDEX IF NOT EXISTS idx_memories_session ON memories(session_id);",
            ),
            (
                2,
                "ALTER TABLE memories ADD COLUMN namespace TEXT DEFAULT 'default';
                 CREATE INDEX IF NOT EXISTS idx_memories_namespace ON memories(namespace);",
            ),
            (
                3,
                "ALTER TABLE memories ADD COLUMN importance REAL DEFAULT 0.5;",
            ),
            (4, "ALTER TABLE memories ADD COLUMN superseded_by TEXT;"),
            (
                5,
                "CREATE INDEX IF NOT EXISTS idx_memories_updated_at ON memories(updated_at);
                 CREATE INDEX IF NOT EXISTS idx_memories_importance ON memories(importance);
                 CREATE INDEX IF NOT EXISTS idx_memories_superseded ON memories(superseded_by);",
            ),

            (6, "ALTER TABLE memories ADD COLUMN embedding_norm REAL;"),
        ];

        let effective = baseline.max(current);
        for &(version, sql) in MIGRATIONS {
            if version > effective {
                if let Err(e) = conn.execute_batch(sql) {
                    let msg = e.to_string();
                    if msg.contains("duplicate column name")
                        || msg.contains("already exists")
                    {
                        tracing::debug!(
                            "memories migration v{version} idempotent skip: {msg}"
                        );
                        continue;
                    }
                    return Err(e.into());
                }
            }
        }

        let final_version = MIGRATIONS
            .last()
            .map_or(effective, |&(v, _)| v.max(effective));
        if final_version > current {
            Self::set_schema_version(conn, final_version)?;
        }
        Ok(())
    }

    fn category_to_str(cat: &MemoryCategory) -> String {
        match cat {
            MemoryCategory::Core => "core".into(),
            MemoryCategory::Daily => "daily".into(),
            MemoryCategory::Conversation => "conversation".into(),
            MemoryCategory::Custom(name) => name.clone(),
        }
    }

    fn str_to_category(s: &str) -> MemoryCategory {
        match s {
            "core" => MemoryCategory::Core,
            "daily" => MemoryCategory::Daily,
            "conversation" => MemoryCategory::Conversation,
            other => MemoryCategory::Custom(other.to_string()),
        }
    }

    fn content_hash(text: &str) -> String {
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(text.as_bytes());

        format!(
            "{:016x}",
            u64::from_be_bytes(
                hash[..8]
                    .try_into()
                    .expect("SHA-256 always produces >= 8 bytes")
            )
        )
    }

    pub fn connection(&self) -> &Arc<Mutex<Connection>> {
        &self.conn
    }

    pub fn attach_vec_index(
        &self,
        dim: usize,
    ) -> anyhow::Result<crate::memory::vector::index::SqliteVecIndex> {
        let idx = crate::memory::vector::index::SqliteVecIndex::attach(self.conn.clone(), dim)?;
        let (migrated, skipped) = idx.migrate_from_legacy_memories()?;
        if migrated > 0 {
            tracing::info!(
                migrated,
                skipped,
                "vec_memories: migrated legacy embeddings into vector index"
            );
        }
        Ok(idx)
    }

    pub async fn get_or_compute_embedding(&self, text: &str) -> anyhow::Result<Option<Vec<f32>>> {
        if self.embedder.dimensions() == 0 {
            return Ok(None);
        }

        let hash = Self::content_hash(text);
        let now = Local::now().to_rfc3339();

        let conn = self.conn.clone();
        let hash_c = hash.clone();
        let now_c = now.clone();
        let cached = tokio::task::spawn_blocking(move || -> anyhow::Result<Option<Vec<f32>>> {
            let conn = conn.lock();
            let mut stmt =
                conn.prepare("SELECT embedding FROM embedding_cache WHERE content_hash = ?1")?;
            let blob: Option<Vec<u8>> = stmt.query_row(params![hash_c], |row| row.get(0)).ok();
            if let Some(bytes) = blob {
                conn.execute(
                    "UPDATE embedding_cache SET accessed_at = ?1 WHERE content_hash = ?2",
                    params![now_c, hash_c],
                )?;
                return Ok(Some(vector::bytes_to_vec(&bytes)));
            }
            Ok(None)
        })
        .await??;

        if cached.is_some() {
            return Ok(cached);
        }

        let embedding = self.embedder.embed_one(text).await?;
        let bytes = vector::vec_to_bytes(&embedding);

        let conn = self.conn.clone();
        #[allow(clippy::cast_possible_wrap)]
        let cache_max = self.cache_max as i64;
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let conn = conn.lock();
            conn.execute(
                "INSERT OR REPLACE INTO embedding_cache (content_hash, embedding, created_at, accessed_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![hash, bytes, now, now],
            )?;
            conn.execute(
                "DELETE FROM embedding_cache WHERE content_hash IN (
                    SELECT content_hash FROM embedding_cache
                    ORDER BY accessed_at ASC
                    LIMIT MAX(0, (SELECT COUNT(*) FROM embedding_cache) - ?1)
                )",
                params![cache_max],
            )?;
            Ok(())
        })
        .await??;

        Ok(Some(embedding))
    }

    pub fn fts5_search(
        conn: &Connection,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<(String, f32)>> {

        let fts_query: String = query
            .split_whitespace()
            .map(|w| format!("\"{w}\""))
            .collect::<Vec<_>>()
            .join(" OR ");

        if fts_query.is_empty() {
            return Ok(Vec::new());
        }

        let sql = "SELECT m.id, bm25(memories_fts) as score
                   FROM memories_fts f
                   JOIN memories m ON m.rowid = f.rowid
                   WHERE memories_fts MATCH ?1
                   ORDER BY score
                   LIMIT ?2";

        let mut stmt = conn.prepare(sql)?;
        #[allow(clippy::cast_possible_wrap)]
        let limit_i64 = limit as i64;

        let rows = stmt.query_map(params![fts_query, limit_i64], |row| {
            let id: String = row.get(0)?;
            let score: f64 = row.get(1)?;

            #[allow(clippy::cast_possible_truncation)]
            Ok((id, (-score) as f32))
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    pub fn vector_search(
        conn: &Connection,
        query_embedding: &[f32],
        limit: usize,
        category: Option<&str>,
        session_id: Option<&str>,
    ) -> anyhow::Result<Vec<(String, f32)>> {
        let mut sql =
            "SELECT id, embedding, embedding_norm FROM memories WHERE embedding IS NOT NULL"
                .to_string();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut idx = 1;

        if let Some(cat) = category {
            let _ = write!(sql, " AND category = ?{idx}");
            param_values.push(Box::new(cat.to_string()));
            idx += 1;
        }
        if let Some(sid) = session_id {
            let _ = write!(sql, " AND (session_id IS NULL OR session_id = ?{idx})");
            param_values.push(Box::new(sid.to_string()));
        }

        let query_norm: f32 = query_embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if query_norm < f32::EPSILON {
            return Ok(Vec::new());
        }

        let mut stmt = conn.prepare(&sql)?;
        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(AsRef::as_ref).collect();
        let rows = stmt.query_map(params_ref.as_slice(), |row| {
            let id: String = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            let cached_norm: Option<f64> = row.get(2)?;
            Ok((id, blob, cached_norm))
        })?;

        use std::cmp::Reverse;
        use std::collections::BinaryHeap;
        let mut heap: BinaryHeap<Reverse<(ordered_float::OrderedFloat<f32>, String)>> =
            BinaryHeap::with_capacity(limit + 1);

        let mut min_sim: f32 = 0.0;

        for row in rows {
            let (id, blob, cached_norm) = row?;
            let emb = vector::bytes_to_vec(&blob);

            let dot: f32 = query_embedding
                .iter()
                .zip(emb.iter())
                .map(|(a, b)| a * b)
                .sum();
            let emb_norm: f32 = match cached_norm {
                Some(n) if n > 0.0 => n as f32,
                _ => emb.iter().map(|x| x * x).sum::<f32>().sqrt(),
            };
            if emb_norm < f32::EPSILON {
                continue;
            }
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

        let mut scored: Vec<(String, f32)> = heap
            .into_iter()
            .map(|Reverse((of, id))| (id, of.0))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(scored)
    }

    pub(crate) fn vector_search_indexed(
        vec_index: &Arc<RwLock<VecIndexCache>>,
        conn: &Connection,
        query_embedding: &[f32],
        limit: usize,
        category: Option<&str>,
        session_id: Option<&str>,
    ) -> anyhow::Result<Vec<(String, f32)>> {
        Self::ensure_vec_index_fresh(vec_index, conn)?;

        let overfetch = 8usize;
        let candidate_limit = limit.saturating_mul(overfetch).max(limit);

        let candidates: Vec<(String, f32)> = {
            let cache = vec_index.read();
            let Some(index) = cache.index.as_ref() else {
                drop(cache);
                return Self::vector_search(conn, query_embedding, limit, category, session_id);
            };
            if index.is_empty() {
                drop(cache);
                return Self::vector_search(conn, query_embedding, limit, category, session_id);
            }
            index.search(query_embedding, candidate_limit)
        };

        if category.is_none() && session_id.is_none() {
            return Ok(candidates.into_iter().take(limit).collect());
        }

        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        let id_placeholders: String = (0..candidates.len())
            .map(|i| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(",");
        let mut sql = format!(
            "SELECT id FROM memories WHERE id IN ({}) AND embedding IS NOT NULL",
            id_placeholders
        );
        let mut next_idx = candidates.len() + 1;
        if category.is_some() {
            let _ = write!(sql, " AND category = ?{next_idx}");
            next_idx += 1;
        }
        if session_id.is_some() {
            let _ = write!(sql, " AND (session_id IS NULL OR session_id = ?{next_idx})");
        }

        let mut stmt = conn.prepare(&sql)?;
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = candidates
            .iter()
            .map(|(id, _)| Box::new(id.clone()) as Box<dyn rusqlite::types::ToSql>)
            .collect();
        if let Some(cat) = category {
            params.push(Box::new(cat.to_string()));
        }
        if let Some(sid) = session_id {
            params.push(Box::new(sid.to_string()));
        }
        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(AsRef::as_ref).collect();

        let surviving_ids: std::collections::HashSet<String> = stmt
            .query_map(params_ref.as_slice(), |row| row.get::<_, String>(0))?
            .filter_map(std::result::Result::ok)
            .collect();

        let filtered: Vec<(String, f32)> = candidates
            .into_iter()
            .filter(|(id, _)| surviving_ids.contains(id))
            .take(limit)
            .collect();
        Ok(filtered)
    }

    pub fn embedding_norm(embedding: &[f32]) -> f32 {
        embedding.iter().map(|x| x * x).sum::<f32>().sqrt()
    }

    async fn recall_by_time_only(
        &self,
        limit: usize,
        session_id: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self.conn.clone();
        let sid = session_id.map(String::from);
        let since_owned = since.map(String::from);
        let until_owned = until.map(String::from);

        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<MemoryEntry>> {
            let conn = conn.lock();
            let since_ref = since_owned.as_deref();
            let until_ref = until_owned.as_deref();

            let mut sql =
                "SELECT id, key, content, category, created_at, session_id, namespace, importance, superseded_by FROM memories \
                           WHERE superseded_by IS NULL AND 1=1"
                    .to_string();
            let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            let mut idx = 1;

            if let Some(sid) = sid.as_deref() {
                let _ = write!(sql, " AND session_id = ?{idx}");
                param_values.push(Box::new(sid.to_string()));
                idx += 1;
            }
            if let Some(s) = since_ref {
                let _ = write!(sql, " AND created_at >= ?{idx}");
                param_values.push(Box::new(s.to_string()));
                idx += 1;
            }
            if let Some(u) = until_ref {
                let _ = write!(sql, " AND created_at <= ?{idx}");
                param_values.push(Box::new(u.to_string()));
                idx += 1;
            }
            let _ = write!(sql, " ORDER BY updated_at DESC LIMIT ?{idx}");
            #[allow(clippy::cast_possible_wrap)]
            param_values.push(Box::new(limit as i64));

            let mut stmt = conn.prepare(&sql)?;
            let params_ref: Vec<&dyn rusqlite::types::ToSql> =
                param_values.iter().map(AsRef::as_ref).collect();
            let rows = stmt.query_map(params_ref.as_slice(), |row| {
                Ok(MemoryEntry {
                    id: row.get(0)?,
                    key: row.get(1)?,
                    content: row.get(2)?,
                    category: Self::str_to_category(&row.get::<_, String>(3)?),
                    timestamp: row.get(4)?,
                    session_id: row.get(5)?,
                    score: None,
                    namespace: row.get::<_, Option<String>>(6)?.unwrap_or_else(|| "default".into()),
                    importance: row.get(7)?,
                    superseded_by: row.get(8)?,
                })
            })?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row?);
            }
            Ok(results)
        })
        .await?
    }
}

#[async_trait]
impl Memory for SqliteMemory {
    fn name(&self) -> &str {
        "sqlite"
    }

    async fn store(
        &self,
        key: &str,
        content: &str,
        category: MemoryCategory,
        session_id: Option<&str>,
    ) -> anyhow::Result<()> {

        let embedding_bytes = match self.get_or_compute_embedding(content).await {
            Ok(embedding) => embedding.map(|emb| vector::vec_to_bytes(&emb)),
            Err(err) => {
                tracing::warn!(
                    key = %key,
                    error = %err,
                    "memory store: embedding failed; storing entry without vector"
                );
                None
            }
        };

        let key_owned = key.to_string();
        let content_owned = content.to_string();
        let sid_owned = session_id.map(String::from);
        let conn_arc = self.conn.clone();

        let policy = crate::util::retry::RetryPolicy::sqlite_busy();
        crate::util::retry::retry(&policy, |_attempt| {
            let conn = conn_arc.clone();
            let key = key_owned.clone();
            let content = content_owned.clone();
            let sid = sid_owned.clone();
            let category = category.clone();
            let embedding_bytes = embedding_bytes.clone();
            async move {
                tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                    let conn = conn.lock();
                    let now = Local::now().to_rfc3339();
                    let cat = Self::category_to_str(&category);
                    let id = Uuid::new_v4().to_string();

                    conn.execute(
                        "INSERT INTO memories (id, key, content, category, embedding, created_at, updated_at, session_id, namespace, importance)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'default', 0.5)
                         ON CONFLICT(key) DO UPDATE SET
                            content = excluded.content,
                            category = excluded.category,
                            embedding = excluded.embedding,
                            updated_at = excluded.updated_at,
                            session_id = excluded.session_id",
                        params![id, key, content, cat, embedding_bytes, now, now, sid],
                    )?;
                    Ok(())
                })
                .await?
            }
        })
        .await
    }

    async fn recall(
        &self,
        query: &str,
        limit: usize,
        session_id: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {

        if query.trim().is_empty() {
            return self
                .recall_by_time_only(limit, session_id, since, until)
                .await;
        }

        let mut effective_mode = self.search_mode.clone();
        let query_embedding = if self.search_mode == SearchMode::Bm25 {
            None
        } else {
            match self.get_or_compute_embedding(query).await {
                Ok(embedding) => embedding,
                Err(err) => {
                    tracing::warn!(
                        mode = ?self.search_mode,
                        error = %err,
                        "memory recall: embedding failed; degrading to keyword-only (FTS) search"
                    );
                    effective_mode = SearchMode::Bm25;
                    None
                }
            }
        };

        let conn = self.conn.clone();
        let vec_index = self.vec_index.clone();
        let query = query.to_string();
        let sid = session_id.map(String::from);
        let since_owned = since.map(String::from);
        let until_owned = until.map(String::from);
        let vector_weight = self.vector_weight;
        let keyword_weight = self.keyword_weight;
        let search_mode = effective_mode;

        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<MemoryEntry>> {
            let conn = conn.lock();
            let session_ref = sid.as_deref();
            let since_ref = since_owned.as_deref();
            let until_ref = until_owned.as_deref();

            let keyword_results = if search_mode == SearchMode::Embedding {
                Vec::new()
            } else {
                Self::fts5_search(&conn, &query, limit * 2).unwrap_or_else(|e| {
                    tracing::warn!("FTS5 search failed: {e}");
                    Vec::new()
                })
            };

            let vector_results = if search_mode == SearchMode::Bm25 {
                Vec::new()
            } else if let Some(ref qe) = query_embedding {
                Self::vector_search_indexed(&vec_index, &conn, qe, limit * 2, None, session_ref)
                    .unwrap_or_else(|e| {
                        tracing::warn!("Vector search failed: {e}");
                        Vec::new()
                    })
            } else {
                Vec::new()
            };

            let merged = if vector_results.is_empty() {
                keyword_results
                    .iter()
                    .map(|(id, score)| vector::ScoredResult {
                        id: id.clone(),
                        vector_score: None,
                        keyword_score: Some(*score),
                        final_score: *score,
                    })
                    .collect::<Vec<_>>()
            } else if keyword_results.is_empty() {
                vector_results
                    .iter()
                    .map(|(id, score)| vector::ScoredResult {
                        id: id.clone(),
                        vector_score: Some(*score),
                        keyword_score: None,
                        final_score: *score,
                    })
                    .collect::<Vec<_>>()
            } else {
                vector::hybrid_merge(
                    &vector_results,
                    &keyword_results,
                    vector_weight,
                    keyword_weight,
                    limit,
                )
            };

            let mut results = Vec::new();
            if !merged.is_empty() {
                let placeholders: String = (1..=merged.len())
                    .map(|i| format!("?{i}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let sql = format!(
                    "SELECT id, key, content, category, created_at, session_id, namespace, importance, superseded_by \
                     FROM memories WHERE superseded_by IS NULL AND id IN ({placeholders})"
                );
                let mut stmt = conn.prepare(&sql)?;
                let id_params: Vec<Box<dyn rusqlite::types::ToSql>> = merged
                    .iter()
                    .map(|s| Box::new(s.id.clone()) as Box<dyn rusqlite::types::ToSql>)
                    .collect();
                let params_ref: Vec<&dyn rusqlite::types::ToSql> =
                    id_params.iter().map(AsRef::as_ref).collect();
                let rows = stmt.query_map(params_ref.as_slice(), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, Option<f64>>(7)?,
                        row.get::<_, Option<String>>(8)?,
                    ))
                })?;

                let mut entry_map = std::collections::HashMap::new();
                for row in rows {
                    let (id, key, content, cat, ts, sid, ns, imp, sup) = row?;
                    entry_map.insert(id, (key, content, cat, ts, sid, ns, imp, sup));
                }

                for scored in &merged {
                    if let Some((key, content, cat, ts, sid, ns, imp, sup)) = entry_map.remove(&scored.id) {
                        if let Some(s) = since_ref {
                            if ts.as_str() < s {
                                continue;
                            }
                        }
                        if let Some(u) = until_ref {
                            if ts.as_str() > u {
                                continue;
                            }
                        }
                        let entry = MemoryEntry {
                            id: scored.id.clone(),
                            key,
                            content,
                            category: Self::str_to_category(&cat),
                            timestamp: ts,
                            session_id: sid,
                            score: Some(f64::from(scored.final_score)),
                            namespace: ns.unwrap_or_else(|| "default".into()),
                            importance: imp,
                            superseded_by: sup,
                        };
                        if let Some(filter_sid) = session_ref {
                            if let Some(other) = entry.session_id.as_deref() {
                                if other != filter_sid {
                                    continue;
                                }
                            }
                        }
                        results.push(entry);
                    }
                }
            }

            if results.is_empty() {
                const MAX_LIKE_KEYWORDS: usize = 8;
                let keywords: Vec<String> = query
                    .split_whitespace()
                    .take(MAX_LIKE_KEYWORDS)
                    .map(|w| format!("%{w}%"))
                    .collect();
                if !keywords.is_empty() {
                    let conditions: Vec<String> = keywords
                        .iter()
                        .enumerate()
                        .map(|(i, _)| {
                            format!("(content LIKE ?{} OR key LIKE ?{})", i * 2 + 1, i * 2 + 2)
                        })
                        .collect();
                    let where_clause = conditions.join(" OR ");
                    let mut param_idx = keywords.len() * 2 + 1;
                    let mut time_conditions = String::new();
                    if since_ref.is_some() {
                        let _ = write!(time_conditions, " AND created_at >= ?{param_idx}");
                        param_idx += 1;
                    }
                    if until_ref.is_some() {
                        let _ = write!(time_conditions, " AND created_at <= ?{param_idx}");
                        param_idx += 1;
                    }
                    let sql = format!(
                        "SELECT id, key, content, category, created_at, session_id, namespace, importance, superseded_by FROM memories
                         WHERE superseded_by IS NULL AND ({where_clause}){time_conditions}
                         ORDER BY updated_at DESC
                         LIMIT ?{param_idx}"
                    );
                    let mut stmt = conn.prepare(&sql)?;
                    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
                    for kw in &keywords {
                        param_values.push(Box::new(kw.clone()));
                        param_values.push(Box::new(kw.clone()));
                    }
                    if let Some(s) = since_ref {
                        param_values.push(Box::new(s.to_string()));
                    }
                    if let Some(u) = until_ref {
                        param_values.push(Box::new(u.to_string()));
                    }
                    #[allow(clippy::cast_possible_wrap)]
                    param_values.push(Box::new(limit as i64));
                    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
                        param_values.iter().map(AsRef::as_ref).collect();
                    let rows = stmt.query_map(params_ref.as_slice(), |row| {
                        Ok(MemoryEntry {
                            id: row.get(0)?,
                            key: row.get(1)?,
                            content: row.get(2)?,
                            category: Self::str_to_category(&row.get::<_, String>(3)?),
                            timestamp: row.get(4)?,
                            session_id: row.get(5)?,
                            score: Some(1.0),
                            namespace: row.get::<_, Option<String>>(6)?.unwrap_or_else(|| "default".into()),
                            importance: row.get(7)?,
                            superseded_by: row.get(8)?,
                        })
                    })?;
                    for row in rows {
                        let entry = row?;
                        if let Some(filter_sid) = session_ref {
                            if let Some(other) = entry.session_id.as_deref() {
                                if other != filter_sid {
                                    continue;
                                }
                            }
                        }
                        results.push(entry);
                    }
                }
            }

            results.truncate(limit);
            Ok(results)
        })
        .await?
    }

    async fn get(&self, key: &str) -> anyhow::Result<Option<MemoryEntry>> {
        let conn = self.conn.clone();
        let key = key.to_string();

        tokio::task::spawn_blocking(move || -> anyhow::Result<Option<MemoryEntry>> {
            let conn = conn.lock();
            let mut stmt = conn.prepare(
                "SELECT id, key, content, category, created_at, session_id, namespace, importance, superseded_by FROM memories WHERE key = ?1",
            )?;

            let mut rows = stmt.query_map(params![key], |row| {
                Ok(MemoryEntry {
                    id: row.get(0)?,
                    key: row.get(1)?,
                    content: row.get(2)?,
                    category: Self::str_to_category(&row.get::<_, String>(3)?),
                    timestamp: row.get(4)?,
                    session_id: row.get(5)?,
                    score: None,
                    namespace: row.get::<_, Option<String>>(6)?.unwrap_or_else(|| "default".into()),
                    importance: row.get(7)?,
                    superseded_by: row.get(8)?,
                })
            })?;

            match rows.next() {
                Some(Ok(entry)) => Ok(Some(entry)),
                _ => Ok(None),
            }
        })
        .await?
    }

    async fn list(
        &self,
        category: Option<&MemoryCategory>,
        session_id: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        const DEFAULT_LIST_LIMIT: i64 = 1000;

        let conn = self.conn.clone();
        let category = category.cloned();
        let sid = session_id.map(String::from);

        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<MemoryEntry>> {
            let conn = conn.lock();
            let session_ref = sid.as_deref();
            let mut results = Vec::new();

            let row_mapper = |row: &rusqlite::Row| -> rusqlite::Result<MemoryEntry> {
                Ok(MemoryEntry {
                    id: row.get(0)?,
                    key: row.get(1)?,
                    content: row.get(2)?,
                    category: Self::str_to_category(&row.get::<_, String>(3)?),
                    timestamp: row.get(4)?,
                    session_id: row.get(5)?,
                    score: None,
                    namespace: row.get::<_, Option<String>>(6)?.unwrap_or_else(|| "default".into()),
                    importance: row.get(7)?,
                    superseded_by: row.get(8)?,
                })
            };

            let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
                match (&category, session_ref) {
                    (Some(cat), Some(sid)) => {
                        let cat_str = Self::category_to_str(cat);
                        (
                            "SELECT id, key, content, category, created_at, session_id, namespace, importance, superseded_by FROM memories
                             WHERE superseded_by IS NULL AND category = ?1 AND session_id = ?2 ORDER BY updated_at DESC LIMIT ?3".into(),
                            vec![Box::new(cat_str) as Box<dyn rusqlite::types::ToSql>, Box::new(sid.to_string()), Box::new(DEFAULT_LIST_LIMIT)],
                        )
                    }
                    (Some(cat), None) => {
                        let cat_str = Self::category_to_str(cat);
                        (
                            "SELECT id, key, content, category, created_at, session_id, namespace, importance, superseded_by FROM memories
                             WHERE superseded_by IS NULL AND category = ?1 ORDER BY updated_at DESC LIMIT ?2".into(),
                            vec![Box::new(cat_str) as Box<dyn rusqlite::types::ToSql>, Box::new(DEFAULT_LIST_LIMIT)],
                        )
                    }
                    (None, Some(sid)) => {
                        (
                            "SELECT id, key, content, category, created_at, session_id, namespace, importance, superseded_by FROM memories
                             WHERE superseded_by IS NULL AND session_id = ?1 ORDER BY updated_at DESC LIMIT ?2".into(),
                            vec![Box::new(sid.to_string()) as Box<dyn rusqlite::types::ToSql>, Box::new(DEFAULT_LIST_LIMIT)],
                        )
                    }
                    (None, None) => {
                        (
                            "SELECT id, key, content, category, created_at, session_id, namespace, importance, superseded_by FROM memories
                             WHERE superseded_by IS NULL ORDER BY updated_at DESC LIMIT ?1".into(),
                            vec![Box::new(DEFAULT_LIST_LIMIT) as Box<dyn rusqlite::types::ToSql>],
                        )
                    }
                };
            let params_refs: Vec<&dyn rusqlite::types::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params_refs.as_slice(), row_mapper)?;
            for row in rows {
                results.push(row?);
            }

            Ok(results)
        })
        .await?
    }

    async fn forget(&self, key: &str) -> anyhow::Result<bool> {
        let conn = self.conn.clone();
        let key = key.to_string();

        tokio::task::spawn_blocking(move || -> anyhow::Result<bool> {
            let conn = conn.lock();
            let affected = conn.execute("DELETE FROM memories WHERE key = ?1", params![key])?;
            Ok(affected > 0)
        })
        .await?
    }

    async fn purge_namespace(&self, namespace: &str) -> anyhow::Result<usize> {
        let conn = self.conn.clone();
        let namespace = namespace.to_string();

        tokio::task::spawn_blocking(move || -> anyhow::Result<usize> {
            let conn = conn.lock();
            let affected = conn.execute(
                "DELETE FROM memories WHERE namespace = ?1",
                params![namespace],
            )?;
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            Ok(affected as usize)
        })
        .await?
    }

    async fn purge_session(&self, session_id: &str) -> anyhow::Result<usize> {
        let conn = self.conn.clone();
        let session_id = session_id.to_string();

        tokio::task::spawn_blocking(move || -> anyhow::Result<usize> {
            let conn = conn.lock();
            let affected = conn.execute(
                "DELETE FROM memories WHERE session_id = ?1",
                params![session_id],
            )?;
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            Ok(affected as usize)
        })
        .await?
    }

    async fn count(&self) -> anyhow::Result<usize> {

        let conn = self.read_conn();

        tokio::task::spawn_blocking(move || -> anyhow::Result<usize> {
            let conn = conn.lock();
            let count: i64 =
                conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))?;
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            Ok(count as usize)
        })
        .await?
    }

    async fn health_check(&self) -> bool {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || conn.lock().execute_batch("SELECT 1").is_ok())
            .await
            .unwrap_or(false)
    }

    async fn export(&self, filter: &ExportFilter) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self.conn.clone();
        let filter = filter.clone();

        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<MemoryEntry>> {
            let conn = conn.lock();
            let mut sql =
                "SELECT id, key, content, category, created_at, session_id, namespace, importance, superseded_by \
                 FROM memories WHERE 1=1"
                    .to_string();
            let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            let mut idx = 1;

            if let Some(ref ns) = filter.namespace {
                let _ = write!(sql, " AND namespace = ?{idx}");
                param_values.push(Box::new(ns.clone()));
                idx += 1;
            }
            if let Some(ref sid) = filter.session_id {
                let _ = write!(sql, " AND session_id = ?{idx}");
                param_values.push(Box::new(sid.clone()));
                idx += 1;
            }
            if let Some(ref cat) = filter.category {
                let _ = write!(sql, " AND category = ?{idx}");
                param_values.push(Box::new(Self::category_to_str(cat)));
                idx += 1;
            }
            if let Some(ref since) = filter.since {
                let _ = write!(sql, " AND created_at >= ?{idx}");
                param_values.push(Box::new(since.clone()));
                idx += 1;
            }
            if let Some(ref until) = filter.until {
                let _ = write!(sql, " AND created_at <= ?{idx}");
                param_values.push(Box::new(until.clone()));
                let _ = idx;
            }
            sql.push_str(" ORDER BY created_at ASC");

            let mut stmt = conn.prepare(&sql)?;
            let params_ref: Vec<&dyn rusqlite::types::ToSql> =
                param_values.iter().map(AsRef::as_ref).collect();
            let rows = stmt.query_map(params_ref.as_slice(), |row| {
                Ok(MemoryEntry {
                    id: row.get(0)?,
                    key: row.get(1)?,
                    content: row.get(2)?,
                    category: Self::str_to_category(&row.get::<_, String>(3)?),
                    timestamp: row.get(4)?,
                    session_id: row.get(5)?,
                    score: None,
                    namespace: row.get::<_, Option<String>>(6)?.unwrap_or_else(|| "default".into()),
                    importance: row.get(7)?,
                    superseded_by: row.get(8)?,
                })
            })?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row?);
            }
            Ok(results)
        })
        .await?
    }

    async fn recall_namespaced(
        &self,
        namespace: &str,
        query: &str,
        limit: usize,
        session_id: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        if query.trim().is_empty() {
            let conn = self.conn.clone();
            let ns = namespace.to_string();
            let sid = session_id.map(String::from);
            let since_owned = since.map(String::from);
            let until_owned = until.map(String::from);
            return tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<MemoryEntry>> {
                let conn = conn.lock();
                let mut sql = String::from(
                    "SELECT id, key, content, category, created_at, session_id, namespace, importance, superseded_by, updated_at \
                     FROM memories WHERE superseded_by IS NULL AND namespace = ?1",
                );
                let mut param_values: Vec<Box<dyn rusqlite::types::ToSql + Send>> = vec![Box::new(ns)];
                let mut idx = 2;
                if let Some(ref s) = sid {
                    let _ = write!(sql, " AND session_id = ?{idx}");
                    param_values.push(Box::new(s.clone()));
                    idx += 1;
                }
                if let Some(ref s) = since_owned {
                    let _ = write!(sql, " AND created_at >= ?{idx}");
                    param_values.push(Box::new(s.clone()));
                    idx += 1;
                }
                if let Some(ref s) = until_owned {
                    let _ = write!(sql, " AND created_at <= ?{idx}");
                    param_values.push(Box::new(s.clone()));
                    let _ = idx;
                }
                sql.push_str(" ORDER BY updated_at DESC LIMIT ");
                sql.push_str(&limit.to_string());

                let mut stmt = conn.prepare(&sql)?;
                let params_ref: Vec<&dyn rusqlite::types::ToSql> =
                    param_values.iter().map(|b| b.as_ref() as &dyn rusqlite::types::ToSql).collect();
                let rows = stmt.query_map(params_ref.as_slice(), |row| {
                    Ok(MemoryEntry {
                        id: row.get(0)?,
                        key: row.get(1)?,
                        content: row.get(2)?,
                        category: Self::str_to_category(&row.get::<_, String>(3)?),
                        timestamp: row.get(4)?,
                        session_id: row.get(5)?,
                        namespace: row.get::<_, Option<String>>(6)?.unwrap_or_else(|| "default".into()),
                        importance: Some(row.get::<_, Option<f64>>(7)?.unwrap_or(0.5)),
                        superseded_by: row.get(8)?,
                        score: None,
                    })
                })?;
                rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
            })
            .await?;
        }

        let entries = self
            .recall(query, limit * 2, session_id, since, until)
            .await?;
        let filtered: Vec<MemoryEntry> = entries
            .into_iter()
            .filter(|e| e.namespace == namespace)
            .take(limit)
            .collect();
        Ok(filtered)
    }

    async fn store_with_metadata(
        &self,
        key: &str,
        content: &str,
        category: MemoryCategory,
        session_id: Option<&str>,
        namespace: Option<&str>,
        importance: Option<f64>,
    ) -> anyhow::Result<()> {
        let embedding_bytes = match self.get_or_compute_embedding(content).await {
            Ok(embedding) => embedding.map(|emb| vector::vec_to_bytes(&emb)),
            Err(err) => {
                tracing::warn!(
                    key = %key,
                    error = %err,
                    "memory store: embedding failed; storing entry without vector"
                );
                None
            }
        };

        let conn = self.conn.clone();
        let key = key.to_string();
        let content = content.to_string();
        let sid = session_id.map(String::from);
        let ns = namespace.unwrap_or("default").to_string();
        let imp = importance.unwrap_or(0.5);

        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let conn = conn.lock();
            let now = Local::now().to_rfc3339();
            let cat = Self::category_to_str(&category);
            let id = Uuid::new_v4().to_string();

            conn.execute(
                "INSERT INTO memories (id, key, content, category, embedding, created_at, updated_at, session_id, namespace, importance)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(key) DO UPDATE SET
                    content = excluded.content,
                    category = excluded.category,
                    embedding = excluded.embedding,
                    updated_at = excluded.updated_at,
                    session_id = excluded.session_id,
                    namespace = excluded.namespace,
                    importance = excluded.importance",
                params![id, key, content, cat, embedding_bytes, now, now, sid, ns, imp],
            )?;
            Ok(())
        })
        .await?
    }
}
