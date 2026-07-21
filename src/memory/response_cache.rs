// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Result;
use chrono::{Duration, Local};
use parking_lot::Mutex;
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

struct InMemoryEntry {
    response: String,
    token_count: u32,
    created_at: std::time::Instant,
    accessed_at: std::time::Instant,
}

pub struct ResponseCache {
    conn: Mutex<Connection>,
    #[allow(dead_code)]
    db_path: PathBuf,
    ttl_minutes: i64,
    max_entries: usize,
    hot_cache: Mutex<HashMap<String, InMemoryEntry>>,
    hot_max_entries: usize,
}

impl ResponseCache {

    pub fn new(workspace_dir: &Path, ttl_minutes: u32, max_entries: usize) -> Result<Self> {
        Self::with_hot_cache(workspace_dir, ttl_minutes, max_entries, 256)
    }

    pub fn with_hot_cache(
        workspace_dir: &Path,
        ttl_minutes: u32,
        max_entries: usize,
        hot_max_entries: usize,
    ) -> Result<Self> {
        let db_dir = workspace_dir.join("memory");
        std::fs::create_dir_all(&db_dir)?;
        let db_path = db_dir.join("response_cache.db");

        let conn = Connection::open(&db_path)?;

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous  = NORMAL;
             PRAGMA temp_store   = MEMORY;",
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS response_cache (
                prompt_hash TEXT PRIMARY KEY,
                model       TEXT NOT NULL,
                response    TEXT NOT NULL,
                token_count INTEGER NOT NULL DEFAULT 0,
                created_at  TEXT NOT NULL,
                accessed_at TEXT NOT NULL,
                hit_count   INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_rc_accessed ON response_cache(accessed_at);
            CREATE INDEX IF NOT EXISTS idx_rc_created ON response_cache(created_at);",
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
            db_path,
            ttl_minutes: i64::from(ttl_minutes),
            max_entries,
            hot_cache: Mutex::new(HashMap::new()),
            hot_max_entries,
        })
    }

    pub fn cache_key(model: &str, system_prompt: Option<&str>, user_prompt: &str) -> String {
        Self::cache_key_parts(model, system_prompt, [user_prompt])
    }

    pub fn cache_key_parts<'a>(
        model: &str,
        system_prompt: Option<&str>,
        parts: impl IntoIterator<Item = &'a str>,
    ) -> String {
        let scope = crate::session::current_session_context()
            .map(|c| {
                if c.workspace_key.is_empty() {
                    c.session_id
                } else {
                    format!("{}/{}", c.workspace_key, c.session_id)
                }
            })
            .unwrap_or_else(|| "__global__".to_string());
        let mut hasher = Sha256::new();
        hasher.update(scope.as_bytes());
        hasher.update(b"|");
        hasher.update(model.as_bytes());
        hasher.update(b"|");
        if let Some(sys) = system_prompt {
            hasher.update(sys.as_bytes());
        }
        hasher.update(b"|");
        for part in parts {
            hasher.update(part.as_bytes());
        }
        let hash = hasher.finalize();
        format!("{:064x}", hash)
    }

    #[allow(clippy::cast_sign_loss)]
    pub fn get(&self, key: &str) -> Result<Option<String>> {

        {
            let mut hot = self.hot_cache.lock();
            if let Some(entry) = hot.get_mut(key) {
                let ttl = std::time::Duration::from_secs(self.ttl_minutes as u64 * 60);
                if entry.created_at.elapsed() > ttl {
                    hot.remove(key);
                } else {
                    entry.accessed_at = std::time::Instant::now();
                    let response = entry.response.clone();
                    drop(hot);

                    let conn = self.conn.lock();
                    let now_str = Local::now().to_rfc3339();
                    conn.execute(
                        "UPDATE response_cache
                         SET accessed_at = ?1, hit_count = hit_count + 1
                         WHERE prompt_hash = ?2",
                        params![now_str, key],
                    )?;
                    return Ok(Some(response));
                }
            }
        }

        let result: Option<(String, u32)> = {
            let conn = self.conn.lock();
            let now = Local::now();
            let cutoff = (now - Duration::minutes(self.ttl_minutes)).to_rfc3339();

            let mut stmt = conn.prepare(
                "SELECT response, token_count FROM response_cache
                 WHERE prompt_hash = ?1 AND created_at > ?2",
            )?;

            let result: Option<(String, u32)> = stmt
                .query_row(params![key, cutoff], |row| Ok((row.get(0)?, row.get(1)?)))
                .ok();

            if result.is_some() {
                let now_str = now.to_rfc3339();
                conn.execute(
                    "UPDATE response_cache
                     SET accessed_at = ?1, hit_count = hit_count + 1
                     WHERE prompt_hash = ?2",
                    params![now_str, key],
                )?;
            }

            result
        };

        if let Some((ref response, token_count)) = result {
            self.promote_to_hot(key, response, token_count);
        }

        Ok(result.map(|(r, _)| r))
    }

    pub fn put(&self, key: &str, model: &str, response: &str, token_count: u32) -> Result<()> {

        self.promote_to_hot(key, response, token_count);

        let conn = self.conn.lock();

        let now = Local::now().to_rfc3339();

        conn.execute(
            "INSERT OR REPLACE INTO response_cache
             (prompt_hash, model, response, token_count, created_at, accessed_at, hit_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
            params![key, model, response, token_count, now, now],
        )?;

        let cutoff = (Local::now() - Duration::minutes(self.ttl_minutes)).to_rfc3339();
        conn.execute(
            "DELETE FROM response_cache WHERE created_at <= ?1",
            params![cutoff],
        )?;

        #[allow(clippy::cast_possible_wrap)]
        let max = self.max_entries as i64;
        conn.execute(
            "DELETE FROM response_cache WHERE prompt_hash IN (
                SELECT prompt_hash FROM response_cache
                ORDER BY accessed_at ASC
                LIMIT MAX(0, (SELECT COUNT(*) FROM response_cache) - ?1)
            )",
            params![max],
        )?;

        Ok(())
    }

    fn promote_to_hot(&self, key: &str, response: &str, token_count: u32) {
        let mut hot = self.hot_cache.lock();

        if let Some(entry) = hot.get_mut(key) {
            entry.response = response.to_string();
            entry.token_count = token_count;
            entry.accessed_at = std::time::Instant::now();
            return;
        }

        if self.hot_max_entries > 0 && hot.len() >= self.hot_max_entries {
            if let Some(oldest_key) = hot
                .iter()
                .min_by_key(|(_, v)| v.accessed_at)
                .map(|(k, _)| k.clone())
            {
                hot.remove(&oldest_key);
            }
        }

        if self.hot_max_entries > 0 {
            let now = std::time::Instant::now();
            hot.insert(
                key.to_string(),
                InMemoryEntry {
                    response: response.to_string(),
                    token_count,
                    created_at: now,
                    accessed_at: now,
                },
            );
        }
    }

    pub fn stats(&self) -> Result<(usize, u64, u64)> {
        let conn = self.conn.lock();

        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM response_cache", [], |row| row.get(0))?;

        let hits: i64 = conn.query_row(
            "SELECT COALESCE(SUM(hit_count), 0) FROM response_cache",
            [],
            |row| row.get(0),
        )?;

        let tokens_saved: i64 = conn.query_row(
            "SELECT COALESCE(SUM(token_count * hit_count), 0) FROM response_cache",
            [],
            |row| row.get(0),
        )?;

        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        Ok((count as usize, hits as u64, tokens_saved as u64))
    }

    pub fn clear(&self) -> Result<usize> {
        self.hot_cache.lock().clear();
        let conn = self.conn.lock();
        let affected = conn.execute("DELETE FROM response_cache", [])?;
        Ok(affected)
    }
}
