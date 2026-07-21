// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::channels::session::backend::{
    DesignArtifactRecord, LoadedMessage, RewindStash, SessionBackend, SessionMetadata, SessionQuery,
};
use crate::providers::traits::ChatMessage;
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use parking_lot::{Mutex, MutexGuard};
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

const READER_POOL_SIZE: usize = 3;

pub struct SqliteSessionBackend {
    writer: Mutex<Connection>,
    readers: Vec<Mutex<Connection>>,
    reader_cursor: AtomicUsize,
    #[allow(dead_code)]
    db_path: PathBuf,
}

impl SqliteSessionBackend {

    pub fn new(workspace_dir: &Path) -> Result<Self> {
        let sessions_dir = workspace_dir.join("sessions");
        std::fs::create_dir_all(&sessions_dir).context("Failed to create sessions directory")?;
        let db_path = sessions_dir.join("sessions.db");

        let conn = Connection::open(&db_path)
            .with_context(|| format!("Failed to open session DB: {}", db_path.display()))?;

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA temp_store = MEMORY;
             PRAGMA mmap_size = 4194304;
             PRAGMA busy_timeout = 5000;",
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                session_key TEXT NOT NULL,
                role        TEXT NOT NULL,
                content     TEXT NOT NULL,
                created_at  TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_sessions_key ON sessions(session_key);
             CREATE INDEX IF NOT EXISTS idx_sessions_key_id ON sessions(session_key, id);

             CREATE TABLE IF NOT EXISTS session_metadata (
                session_key  TEXT PRIMARY KEY,
                created_at   TEXT NOT NULL,
                last_activity TEXT NOT NULL,
                message_count INTEGER NOT NULL DEFAULT 0,
                name         TEXT
             );

             CREATE VIRTUAL TABLE IF NOT EXISTS sessions_fts USING fts5(
                session_key, content, content=sessions, content_rowid=id
             );

             CREATE TRIGGER IF NOT EXISTS sessions_ai AFTER INSERT ON sessions BEGIN
                INSERT INTO sessions_fts(rowid, session_key, content)
                VALUES (new.id, new.session_key, new.content);
             END;
             CREATE TRIGGER IF NOT EXISTS sessions_ad AFTER DELETE ON sessions BEGIN
                INSERT INTO sessions_fts(sessions_fts, rowid, session_key, content)
                VALUES ('delete', old.id, old.session_key, old.content);
             END;

             CREATE TABLE IF NOT EXISTS session_design_artifacts (
                session_key TEXT NOT NULL,
                rel_path    TEXT NOT NULL,
                submode     TEXT,
                surface     TEXT NOT NULL,
                created_at  INTEGER NOT NULL,
                updated_at  INTEGER NOT NULL,
                PRIMARY KEY (session_key, rel_path)
             );
             CREATE INDEX IF NOT EXISTS idx_design_artifacts_key
                ON session_design_artifacts(session_key, created_at);",
        )
        .context("Failed to initialize session schema")?;

        let has_name: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('session_metadata') WHERE name = 'name'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);
        if !has_name {
            let _ = conn.execute("ALTER TABLE session_metadata ADD COLUMN name TEXT", []);
        }

        let has_work_dir: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('session_metadata') WHERE name = 'work_dir'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);
        if !has_work_dir {
            let _ = conn.execute(
                "ALTER TABLE session_metadata ADD COLUMN work_dir TEXT",
                [],
            );
        }

        let has_tombstone: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('sessions') WHERE name = 'tombstoned_at'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);
        if !has_tombstone {
            let _ = conn.execute(
                "ALTER TABLE sessions ADD COLUMN tombstoned_at TEXT",
                [],
            );
        }

        let has_hidden: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('sessions') WHERE name = 'hidden_for_ui'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);
        if !has_hidden {
            let _ = conn.execute(
                "ALTER TABLE sessions ADD COLUMN hidden_for_ui INTEGER NOT NULL DEFAULT 0",
                [],
            );
        }

        let has_metadata: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('sessions') WHERE name = 'metadata'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);
        if !has_metadata {
            let _ = conn.execute("ALTER TABLE sessions ADD COLUMN metadata TEXT", []);
        }

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS session_edit_batches (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_key TEXT NOT NULL,
                user_message_index INTEGER NOT NULL,
                edit_batch_id TEXT NOT NULL,
                created_at TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_seb_key_uidx
                 ON session_edit_batches(session_key, user_message_index);
             CREATE UNIQUE INDEX IF NOT EXISTS idx_seb_unique_per_turn
                 ON session_edit_batches(session_key, user_message_index, edit_batch_id);

             CREATE TABLE IF NOT EXISTS session_rewind_stash (
                rewind_id          TEXT PRIMARY KEY,
                session_key        TEXT NOT NULL,
                user_message_index INTEGER NOT NULL,
                stash_json         TEXT NOT NULL,
                created_at         TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_srs_key
                 ON session_rewind_stash(session_key, created_at);",
        )
        .context("Failed to initialize rewind/edit-batch schema")?;

        let mut readers = Vec::with_capacity(READER_POOL_SIZE);
        for _ in 0..READER_POOL_SIZE {
            let reader = Connection::open(&db_path).with_context(|| {
                format!("Failed to open session DB reader: {}", db_path.display())
            })?;
            reader.execute_batch(
                "PRAGMA busy_timeout = 5000;
                 PRAGMA temp_store = MEMORY;
                 PRAGMA mmap_size = 4194304;
                 PRAGMA query_only = ON;",
            )?;
            readers.push(Mutex::new(reader));
        }

        Ok(Self {
            writer: Mutex::new(conn),
            readers,
            reader_cursor: AtomicUsize::new(0),
            db_path,
        })
    }

    fn read_conn(&self) -> MutexGuard<'_, Connection> {
        let start = self.reader_cursor.fetch_add(1, Ordering::Relaxed);
        let len = self.readers.len();
        for offset in 0..len {
            let idx = (start + offset) % len;
            if let Some(guard) = self.readers[idx].try_lock() {
                return guard;
            }
        }
        self.readers[start % len].lock()
    }

    fn append_inner(
        &self,
        session_key: &str,
        message: &ChatMessage,
        hidden_for_ui: bool,
    ) -> std::io::Result<()> {
        let mut conn = self.writer.lock();
        let now = Utc::now().to_rfc3339();
        let metadata_json = if message.metadata.is_empty() {
            None
        } else {
            serde_json::to_string(&message.metadata).ok()
        };

        let tx = conn
            .transaction()
            .map_err(std::io::Error::other)?;
        tx.execute(
            "INSERT INTO sessions (session_key, role, content, created_at, hidden_for_ui, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                session_key,
                message.role,
                message.content,
                now,
                i32::from(hidden_for_ui),
                metadata_json,
            ],
        )
        .map_err(std::io::Error::other)?;
        tx.execute(
            "INSERT INTO session_metadata (session_key, created_at, last_activity, message_count)
             VALUES (?1, ?2, ?3, 1)
             ON CONFLICT(session_key) DO UPDATE SET
                last_activity = excluded.last_activity,
                message_count = message_count + 1",
            params![session_key, now, now],
        )
        .map_err(std::io::Error::other)?;
        tx.commit().map_err(std::io::Error::other)?;

        Ok(())
    }

    pub fn migrate_from_jsonl(&self, workspace_dir: &Path) -> Result<usize> {
        let sessions_dir = workspace_dir.join("sessions");
        let entries = match std::fs::read_dir(&sessions_dir) {
            Ok(e) => e,
            Err(_) => return Ok(0),
        };

        let mut migrated = 0;
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let name = match entry.file_name().into_string() {
                Ok(n) => n,
                Err(_) => continue,
            };
            let Some(key) = name.strip_suffix(".jsonl") else {
                continue;
            };

            let path = entry.path();
            let file = match std::fs::File::open(&path) {
                Ok(f) => f,
                Err(_) => continue,
            };

            let reader = std::io::BufReader::new(file);
            let mut count = 0;
            let mut failed = 0;
            for line in std::io::BufRead::lines(reader) {
                let Ok(line) = line else { continue };
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(msg) = serde_json::from_str::<ChatMessage>(trimmed) {
                    match self.append(key, &msg) {
                        Ok(()) => count += 1,
                        Err(e) => {
                            failed += 1;
                            tracing::warn!(
                                target: "session.sqlite_migrate",
                                session_key = key,
                                error = %e,
                                "failed to migrate JSONL row into sqlite session store"
                            );
                        }
                    }
                }
            }

            if failed > 0 {
                tracing::warn!(
                    target: "session.sqlite_migrate",
                    session_key = key,
                    migrated = count,
                    failed,
                    "JSONL migration completed with some failed rows; source file retained"
                );
            }

            if count > 0 && failed == 0 {
                let migrated_path = path.with_extension("jsonl.migrated");
                if let Err(e) = std::fs::rename(&path, &migrated_path) {
                    tracing::warn!(
                        target: "session.sqlite_migrate",
                        session_key = key,
                        error = %e,
                        "failed to rename migrated JSONL source file"
                    );
                }
                migrated += 1;
            }
        }

        Ok(migrated)
    }

    fn parse_metadata_cell(
        raw: Option<String>,
    ) -> std::collections::HashMap<String, serde_json::Value> {
        raw.as_deref()
            .filter(|s| !s.trim().is_empty())
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default()
    }

    fn delete_session_rows(conn: &Connection, session_key: &str) -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM sessions WHERE session_key = ?1",
            params![session_key],
        )?;
        conn.execute(
            "DELETE FROM session_metadata WHERE session_key = ?1",
            params![session_key],
        )?;
        conn.execute(
            "DELETE FROM session_edit_batches WHERE session_key = ?1",
            params![session_key],
        )?;
        conn.execute(
            "DELETE FROM session_rewind_stash WHERE session_key = ?1",
            params![session_key],
        )?;
        Ok(())
    }
}

impl SessionBackend for SqliteSessionBackend {
    fn load(&self, session_key: &str) -> Vec<ChatMessage> {
        let conn = self.read_conn();

        let mut stmt = match conn
            .prepare(
                "SELECT role, content, metadata FROM sessions
                 WHERE session_key = ?1
                   AND tombstoned_at IS NULL
                   AND COALESCE(hidden_for_ui, 0) = 0
                 ORDER BY id ASC",
            )
        {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        let rows = match stmt.query_map(params![session_key], |row| {
            let metadata_raw: Option<String> = row.get(2).unwrap_or(None);
            Ok(ChatMessage {
                role: row.get(0)?,
                content: row.get(1)?,
                metadata: Self::parse_metadata_cell(metadata_raw),
            })
        }) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };

        rows.filter_map(|r| r.ok()).collect()
    }

    fn load_with_tombstones(&self, session_key: &str) -> Vec<LoadedMessage> {
        let conn = self.read_conn();
        let mut stmt = match conn.prepare(
            "SELECT id, role, content, tombstoned_at, hidden_for_ui, metadata, created_at
             FROM sessions
             WHERE session_key = ?1 ORDER BY id ASC",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        let rows = match stmt.query_map(params![session_key], |row| {
            let id: i64 = row.get(0)?;
            let role: String = row.get(1)?;
            let content: String = row.get(2)?;
            let tombstoned_at: Option<String> = row.get(3)?;
            let hidden_for_ui: i64 = row.get(4).unwrap_or(0);
            let metadata_raw: Option<String> = row.get(5).unwrap_or(None);
            let created_at: Option<String> = row.get(6).unwrap_or(None);
            Ok(LoadedMessage {
                id,
                message: ChatMessage {
                    role,
                    content,
                    metadata: Self::parse_metadata_cell(metadata_raw),
                },
                tombstoned_at,
                hidden_for_ui: hidden_for_ui != 0,
                created_at,
            })
        }) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };

        rows.filter_map(|r| r.ok()).collect()
    }

    fn append(&self, session_key: &str, message: &ChatMessage) -> std::io::Result<()> {
        self.append_inner(session_key, message, false)
    }

    fn append_hidden(&self, session_key: &str, message: &ChatMessage) -> std::io::Result<()> {
        self.append_inner(session_key, message, true)
    }

    fn remove_last(&self, session_key: &str) -> std::io::Result<bool> {
        let conn = self.writer.lock();

        let last_id: Option<i64> = conn
            .query_row(
                "SELECT id FROM sessions WHERE session_key = ?1 ORDER BY id DESC LIMIT 1",
                params![session_key],
                |row| row.get(0),
            )
            .ok();

        let Some(id) = last_id else {
            return Ok(false);
        };

        conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])
            .map_err(std::io::Error::other)?;

        conn.execute(
            "UPDATE session_metadata SET message_count = MAX(0, message_count - 1)
             WHERE session_key = ?1",
            params![session_key],
        )
        .map_err(std::io::Error::other)?;

        Ok(true)
    }

    fn list_sessions(&self) -> Vec<String> {
        let conn = self.read_conn();
        let mut stmt = match conn
            .prepare("SELECT session_key FROM session_metadata ORDER BY last_activity DESC")
        {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        let rows = match stmt.query_map([], |row| row.get(0)) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };

        rows.filter_map(|r| r.ok()).collect()
    }

    fn list_sessions_with_metadata(&self) -> Vec<SessionMetadata> {
        let conn = self.read_conn();
        let mut stmt = match conn.prepare(
            "SELECT session_key, created_at, last_activity, message_count, name, work_dir
             FROM session_metadata ORDER BY last_activity DESC",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        let rows = match stmt.query_map([], |row| {
            let key: String = row.get(0)?;
            let created_str: String = row.get(1)?;
            let activity_str: String = row.get(2)?;
            let count: i64 = row.get(3)?;
            let name: Option<String> = row.get(4)?;
            let work_cell: Option<String> = row.get(5)?;
            let work_dir = work_cell.and_then(|s| {
                let t = s.trim();
                (!t.is_empty()).then(|| t.to_string())
            });

            let created = DateTime::parse_from_rfc3339(&created_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let activity = DateTime::parse_from_rfc3339(&activity_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            Ok(SessionMetadata {
                key,
                name,
                work_dir,
                created_at: created,
                last_activity: activity,
                message_count: count as usize,
            })
        }) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };

        rows.filter_map(|r| r.ok()).collect()
    }

    fn get_session_metadata(&self, session_key: &str) -> Option<SessionMetadata> {
        let conn = self.read_conn();
        conn.query_row(
            "SELECT session_key, created_at, last_activity, message_count, name, work_dir
             FROM session_metadata WHERE session_key = ?1",
            params![session_key],
            |row| {
                let key: String = row.get(0)?;
                let created_str: String = row.get(1)?;
                let activity_str: String = row.get(2)?;
                let count: i64 = row.get(3)?;
                let name: Option<String> = row.get(4)?;
                let work_cell: Option<String> = row.get(5)?;
                let work_dir = work_cell.and_then(|s| {
                    let t = s.trim();
                    (!t.is_empty()).then(|| t.to_string())
                });
                let created = DateTime::parse_from_rfc3339(&created_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                let activity = DateTime::parse_from_rfc3339(&activity_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                Ok(SessionMetadata {
                    key,
                    name,
                    work_dir,
                    created_at: created,
                    last_activity: activity,
                    message_count: count as usize,
                })
            },
        )
        .ok()
    }

    fn count_user_messages(&self, session_key: &str) -> usize {
        let conn = self.read_conn();
        conn.query_row(
            "SELECT COUNT(*) FROM sessions
              WHERE session_key = ?1
                AND role = 'user'
                AND tombstoned_at IS NULL
                AND COALESCE(hidden_for_ui, 0) = 0",
            params![session_key],
            |row| row.get::<_, i64>(0),
        )
        .map(|n| usize::try_from(n).unwrap_or(0))
        .unwrap_or(0)
    }

    fn count_live_user_messages_before_id(&self, session_key: &str, before_id: i64) -> usize {
        let conn = self.read_conn();
        conn.query_row(
            "SELECT COUNT(*) FROM sessions
              WHERE session_key = ?1
                AND role = 'user'
                AND tombstoned_at IS NULL
                AND COALESCE(hidden_for_ui, 0) = 0
                AND id < ?2",
            params![session_key, before_id],
            |row| row.get::<_, i64>(0),
        )
        .map(|n| usize::try_from(n).unwrap_or(0))
        .unwrap_or(0)
    }

    fn count_messages(&self, session_key: &str) -> usize {
        let conn = self.read_conn();
        conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE session_key = ?1",
            params![session_key],
            |row| row.get::<_, i64>(0),
        )
        .map(|n| usize::try_from(n).unwrap_or(0))
        .unwrap_or(0)
    }

    fn load_page_with_counts(
        &self,
        session_key: &str,
        before: Option<usize>,
        limit: usize,
    ) -> (Vec<LoadedMessage>, usize, usize, usize) {
        let conn = self.read_conn();
        // One deferred transaction = one WAL snapshot for all three reads, so a
        // purge/delete committing mid-page can't shift the OFFSET window and
        // mislabel the served indexes.
        if conn.execute_batch("BEGIN DEFERRED").is_err() {
            drop(conn);
            let total = self.count_messages(session_key);
            let end = before.unwrap_or(total).min(total);
            let start = end.saturating_sub(limit.max(1));
            let loaded = self.load_with_tombstones_range(session_key, start, end - start);
            let base = loaded
                .first()
                .map(|m| self.count_live_user_messages_before_id(session_key, m.id))
                .unwrap_or(0);
            return (loaded, start, total, base);
        }
        let result = (|| -> rusqlite::Result<(Vec<LoadedMessage>, usize, usize, usize)> {
            let total: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sessions WHERE session_key = ?1",
                params![session_key],
                |row| row.get(0),
            )?;
            let total = usize::try_from(total).unwrap_or(0);
            let end = before.unwrap_or(total).min(total);
            let start = end.saturating_sub(limit.max(1));
            let count = end - start;

            let mut stmt = conn.prepare(
                "SELECT id, role, content, tombstoned_at, hidden_for_ui, metadata, created_at
                 FROM sessions
                 WHERE session_key = ?1 ORDER BY id ASC LIMIT ?2 OFFSET ?3",
            )?;
            #[allow(clippy::cast_possible_wrap)]
            let rows = stmt.query_map(
                params![session_key, count as i64, start as i64],
                |row| {
                    let id: i64 = row.get(0)?;
                    let role: String = row.get(1)?;
                    let content: String = row.get(2)?;
                    let tombstoned_at: Option<String> = row.get(3)?;
                    let hidden_for_ui: i64 = row.get(4).unwrap_or(0);
                    let metadata_raw: Option<String> = row.get(5).unwrap_or(None);
                    let created_at: Option<String> = row.get(6).unwrap_or(None);
                    Ok(LoadedMessage {
                        id,
                        message: ChatMessage {
                            role,
                            content,
                            metadata: Self::parse_metadata_cell(metadata_raw),
                        },
                        tombstoned_at,
                        hidden_for_ui: hidden_for_ui != 0,
                        created_at,
                    })
                },
            )?;
            let loaded: Vec<LoadedMessage> = rows.filter_map(|r| r.ok()).collect();

            let base_user_index = match loaded.first() {
                Some(first) => {
                    let n: i64 = conn.query_row(
                        "SELECT COUNT(*) FROM sessions
                          WHERE session_key = ?1
                            AND role = 'user'
                            AND tombstoned_at IS NULL
                            AND COALESCE(hidden_for_ui, 0) = 0
                            AND id < ?2",
                        params![session_key, first.id],
                        |row| row.get(0),
                    )?;
                    usize::try_from(n).unwrap_or(0)
                }
                None => 0,
            };
            Ok((loaded, start, total, base_user_index))
        })();
        let _ = conn.execute_batch("COMMIT");
        result.unwrap_or_else(|_| (Vec::new(), 0, 0, 0))
    }

    fn load_tail(&self, session_key: &str, limit: usize) -> Vec<ChatMessage> {
        let conn = self.read_conn();
        let mut stmt = match conn.prepare(
            "SELECT role, content, metadata FROM (
                SELECT id, role, content, metadata FROM sessions
                WHERE session_key = ?1
                  AND tombstoned_at IS NULL
                  AND COALESCE(hidden_for_ui, 0) = 0
                ORDER BY id DESC LIMIT ?2
             ) ORDER BY id ASC",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        #[allow(clippy::cast_possible_wrap)]
        let rows = match stmt.query_map(params![session_key, limit as i64], |row| {
            let metadata_raw: Option<String> = row.get(2).unwrap_or(None);
            Ok(ChatMessage {
                role: row.get(0)?,
                content: row.get(1)?,
                metadata: Self::parse_metadata_cell(metadata_raw),
            })
        }) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };

        rows.filter_map(|r| r.ok()).collect()
    }

    fn load_with_tombstones_range(
        &self,
        session_key: &str,
        offset: usize,
        limit: usize,
    ) -> Vec<LoadedMessage> {
        let conn = self.read_conn();
        let mut stmt = match conn.prepare(
            "SELECT id, role, content, tombstoned_at, hidden_for_ui, metadata, created_at
             FROM sessions
             WHERE session_key = ?1 ORDER BY id ASC LIMIT ?2 OFFSET ?3",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        #[allow(clippy::cast_possible_wrap)]
        let rows = match stmt.query_map(
            params![session_key, limit as i64, offset as i64],
            |row| {
                let id: i64 = row.get(0)?;
                let role: String = row.get(1)?;
                let content: String = row.get(2)?;
                let tombstoned_at: Option<String> = row.get(3)?;
                let hidden_for_ui: i64 = row.get(4).unwrap_or(0);
                let metadata_raw: Option<String> = row.get(5).unwrap_or(None);
                let created_at: Option<String> = row.get(6).unwrap_or(None);
                Ok(LoadedMessage {
                    id,
                    message: ChatMessage {
                        role,
                        content,
                        metadata: Self::parse_metadata_cell(metadata_raw),
                    },
                    tombstoned_at,
                    hidden_for_ui: hidden_for_ui != 0,
                    created_at,
                })
            },
        ) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };

        rows.filter_map(|r| r.ok()).collect()
    }

    fn cleanup_stale(&self, ttl_hours: u32) -> std::io::Result<usize> {
        let conn = self.writer.lock();
        let cutoff = (Utc::now() - Duration::hours(i64::from(ttl_hours))).to_rfc3339();

        let stale_keys: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT session_key FROM session_metadata WHERE last_activity < ?1")
                .map_err(std::io::Error::other)?;
            let rows = stmt
                .query_map(params![cutoff], |row| row.get(0))
                .map_err(std::io::Error::other)?;
            rows.filter_map(|r| r.ok()).collect()
        };

        let count = stale_keys.len();
        if count > 0 {
            let tx = conn.unchecked_transaction().map_err(std::io::Error::other)?;
            for key in &stale_keys {
                Self::delete_session_rows(&tx, key).map_err(std::io::Error::other)?;
            }
            tx.commit().map_err(std::io::Error::other)?;
        }

        Ok(count)
    }

    fn delete_session(&self, session_key: &str) -> std::io::Result<bool> {
        let conn = self.writer.lock();

        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM session_metadata WHERE session_key = ?1",
                params![session_key],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if !exists {
            return Ok(false);
        }

        let tx = conn.unchecked_transaction().map_err(std::io::Error::other)?;
        Self::delete_session_rows(&tx, session_key).map_err(std::io::Error::other)?;
        tx.commit().map_err(std::io::Error::other)?;

        Ok(true)
    }

    fn delete_sessions(&self, session_keys: &[String]) -> std::io::Result<usize> {
        if session_keys.is_empty() {
            return Ok(0);
        }
        let conn = self.writer.lock();
        let tx = conn.unchecked_transaction().map_err(std::io::Error::other)?;
        let mut deleted = 0usize;
        for session_key in session_keys {
            let exists: bool = tx
                .query_row(
                    "SELECT COUNT(*) > 0 FROM session_metadata WHERE session_key = ?1",
                    params![session_key],
                    |row| row.get(0),
                )
                .unwrap_or(false);
            if !exists {
                continue;
            }
            Self::delete_session_rows(&tx, session_key).map_err(std::io::Error::other)?;
            deleted += 1;
        }
        tx.commit().map_err(std::io::Error::other)?;
        Ok(deleted)
    }

    fn set_session_name(&self, session_key: &str, name: &str) -> std::io::Result<()> {
        let conn = self.writer.lock();
        let name_val = if name.is_empty() { None } else { Some(name) };
        conn.execute(
            "UPDATE session_metadata SET name = ?1 WHERE session_key = ?2",
            params![name_val, session_key],
        )
        .map_err(std::io::Error::other)?;
        Ok(())
    }

    fn get_session_name(&self, session_key: &str) -> std::io::Result<Option<String>> {
        let conn = self.read_conn();
        conn.query_row(
            "SELECT name FROM session_metadata WHERE session_key = ?1",
            params![session_key],
            |row| row.get(0),
        )
        .map_err(std::io::Error::other)
    }

    fn set_session_work_dir(&self, session_key: &str, dir: &str) -> std::io::Result<()> {
        let conn = self.writer.lock();
        let dir_val = dir.trim();
        conn.execute(
            "UPDATE session_metadata SET work_dir = ?1 WHERE session_key = ?2",
            params![dir_val, session_key],
        )
        .map_err(std::io::Error::other)?;
        Ok(())
    }

    fn get_session_work_dir(&self, session_key: &str) -> std::io::Result<Option<String>> {
        let conn = self.read_conn();
        match conn.query_row(
            "SELECT work_dir FROM session_metadata WHERE session_key = ?1",
            params![session_key],
            |row| row.get::<_, Option<String>>(0),
        ) {
            Ok(opt) => Ok(opt.and_then(|s| {
                let t = s.trim();
                (!t.is_empty()).then(|| t.to_string())
            })),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(err) => Err(std::io::Error::other(err)),
        }
    }

    fn search(&self, query: &SessionQuery) -> Vec<SessionMetadata> {
        let Some(keyword) = &query.keyword else {
            return self.list_sessions_with_metadata();
        };

        let conn = self.read_conn();
        #[allow(clippy::cast_possible_wrap)]
        let limit = query.limit.unwrap_or(50) as i64;

        let mut stmt = match conn.prepare(
            "SELECT DISTINCT f.session_key
             FROM sessions_fts f
             WHERE sessions_fts MATCH ?1
             LIMIT ?2",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        let fts_query: String = keyword
            .split_whitespace()
            .map(|w| format!("\"{w}\""))
            .collect::<Vec<_>>()
            .join(" OR ");

        let keys: Vec<String> = match stmt.query_map(params![fts_query, limit], |row| row.get(0)) {
            Ok(r) => r.filter_map(|r| r.ok()).collect(),
            Err(_) => return Vec::new(),
        };

        keys.iter()
            .filter_map(|key| {
                conn.query_row(
                    "SELECT created_at, last_activity, message_count, name, work_dir FROM session_metadata WHERE session_key = ?1",
                    params![key],
                    |row| {
                        let created_str: String = row.get(0)?;
                        let activity_str: String = row.get(1)?;
                        let count: i64 = row.get(2)?;
                        let name: Option<String> = row.get(3)?;
                        let work_cell: Option<String> = row.get(4)?;
                        let work_dir = work_cell.and_then(|s| {
                            let t = s.trim();
                            (!t.is_empty()).then(|| t.to_string())
                        });
                        Ok(SessionMetadata {
                            key: key.clone(),
                            name,
                            work_dir,
                            created_at: DateTime::parse_from_rfc3339(&created_str)
                                .map(|dt| dt.with_timezone(&Utc))
                                .unwrap_or_else(|_| Utc::now()),
                            last_activity: DateTime::parse_from_rfc3339(&activity_str)
                                .map(|dt| dt.with_timezone(&Utc))
                                .unwrap_or_else(|_| Utc::now()),
                            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                            message_count: count as usize,
                        })
                    },
                )
                .ok()
            })
            .collect()
    }

    fn tombstone_from(&self, session_key: &str, first_id: i64) -> std::io::Result<usize> {
        let conn = self.writer.lock();
        let now = Utc::now().to_rfc3339();
        let n = conn
            .execute(
                "UPDATE sessions
                    SET tombstoned_at = ?1
                  WHERE session_key = ?2
                    AND id >= ?3
                    AND tombstoned_at IS NULL",
                params![now, session_key, first_id],
            )
            .map_err(std::io::Error::other)?;

        let live: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE session_key = ?1 AND tombstoned_at IS NULL",
                params![session_key],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let _ = conn.execute(
            "UPDATE session_metadata SET message_count = ?1 WHERE session_key = ?2",
            params![live, session_key],
        );
        Ok(n)
    }

    fn clear_tombstones(&self, session_key: &str) -> std::io::Result<usize> {
        let conn = self.writer.lock();
        let n = conn
            .execute(
                "UPDATE sessions
                    SET tombstoned_at = NULL
                  WHERE session_key = ?1
                    AND tombstoned_at IS NOT NULL",
                params![session_key],
            )
            .map_err(std::io::Error::other)?;
        let live: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE session_key = ?1 AND tombstoned_at IS NULL",
                params![session_key],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let _ = conn.execute(
            "UPDATE session_metadata SET message_count = ?1 WHERE session_key = ?2",
            params![live, session_key],
        );
        Ok(n)
    }

    fn purge_tombstoned(&self, session_key: &str) -> std::io::Result<usize> {
        let conn = self.writer.lock();
        let n = conn
            .execute(
                "DELETE FROM sessions
                  WHERE session_key = ?1
                    AND tombstoned_at IS NOT NULL",
                params![session_key],
            )
            .map_err(std::io::Error::other)?;
        let live: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE session_key = ?1",
                params![session_key],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let _ = conn.execute(
            "UPDATE session_metadata SET message_count = ?1 WHERE session_key = ?2",
            params![live, session_key],
        );
        Ok(n)
    }

    fn record_edit_batch(
        &self,
        session_key: &str,
        user_message_index: i64,
        edit_batch_id: &str,
    ) -> std::io::Result<()> {
        let conn = self.writer.lock();
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT OR IGNORE INTO session_edit_batches
               (session_key, user_message_index, edit_batch_id, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![session_key, user_message_index, edit_batch_id, now],
        )
        .map_err(std::io::Error::other)?;
        Ok(())
    }

    fn edit_batches_after(
        &self,
        session_key: &str,
        from_index: i64,
    ) -> Vec<String> {
        let conn = self.read_conn();
        let mut stmt = match conn.prepare(
            "SELECT edit_batch_id FROM session_edit_batches
              WHERE session_key = ?1 AND user_message_index >= ?2
              ORDER BY id ASC",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = match stmt.query_map(params![session_key, from_index], |row| row.get(0)) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        rows.filter_map(|r| r.ok()).collect()
    }

    fn drop_edit_batches_after(
        &self,
        session_key: &str,
        from_index: i64,
    ) -> std::io::Result<usize> {
        let conn = self.writer.lock();
        let n = conn
            .execute(
                "DELETE FROM session_edit_batches
                  WHERE session_key = ?1 AND user_message_index >= ?2",
                params![session_key, from_index],
            )
            .map_err(std::io::Error::other)?;
        Ok(n)
    }

    fn save_rewind_stash(
        &self,
        rewind_id: &str,
        session_key: &str,
        user_message_index: i64,
        stash_json: &str,
    ) -> std::io::Result<()> {
        let conn = self.writer.lock();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO session_rewind_stash
               (rewind_id, session_key, user_message_index, stash_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(rewind_id) DO UPDATE SET
                stash_json = excluded.stash_json,
                created_at = excluded.created_at",
            params![rewind_id, session_key, user_message_index, stash_json, now],
        )
        .map_err(std::io::Error::other)?;
        Ok(())
    }

    fn take_rewind_stash(&self, rewind_id: &str) -> Option<RewindStash> {
        let conn = self.writer.lock();
        let stash = conn
            .query_row(
                "SELECT rewind_id, session_key, user_message_index, stash_json
                   FROM session_rewind_stash WHERE rewind_id = ?1",
                params![rewind_id],
                |row| {
                    Ok(RewindStash {
                        rewind_id: row.get(0)?,
                        session_key: row.get(1)?,
                        user_message_index: row.get(2)?,
                        stash_json: row.get(3)?,
                    })
                },
            )
            .ok()?;
        if let Err(e) = conn.execute(
            "DELETE FROM session_rewind_stash WHERE rewind_id = ?1",
            params![rewind_id],
        ) {
            tracing::error!(
                target: "session.sqlite",
                rewind_id,
                error = %e,
                "failed to delete rewind stash row; treating it as not taken to avoid replay loops"
            );
            return None;
        }
        Some(stash)
    }

    fn latest_rewind_stash_for_session(
        &self,
        session_key: &str,
    ) -> Option<RewindStash> {
        let conn = self.read_conn();
        conn.query_row(
            "SELECT rewind_id, session_key, user_message_index, stash_json
               FROM session_rewind_stash
              WHERE session_key = ?1
              ORDER BY created_at DESC
              LIMIT 1",
            params![session_key],
            |row| {
                Ok(RewindStash {
                    rewind_id: row.get(0)?,
                    session_key: row.get(1)?,
                    user_message_index: row.get(2)?,
                    stash_json: row.get(3)?,
                })
            },
        )
        .ok()
    }

    fn record_design_artifact(
        &self,
        session_key: &str,
        rel_path: &str,
        submode: Option<&str>,
        surface: &str,
    ) -> std::io::Result<()> {
        let now = Utc::now().timestamp();
        let conn = self.writer.lock();
        conn.execute(
            "INSERT INTO session_design_artifacts
                (session_key, rel_path, submode, surface, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)
             ON CONFLICT(session_key, rel_path) DO UPDATE SET
                updated_at = ?5,
                surface = ?4,
                submode = COALESCE(?3, submode)",
            params![session_key, rel_path, submode, surface, now],
        )
        .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(())
    }

    fn list_design_artifacts(&self, session_key: &str) -> Vec<DesignArtifactRecord> {
        let conn = self.read_conn();
        let mut stmt = match conn.prepare(
            "SELECT rel_path, submode, surface, created_at, updated_at
               FROM session_design_artifacts
              WHERE session_key = ?1
              ORDER BY created_at ASC, rel_path ASC",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(params![session_key], |row| {
            Ok(DesignArtifactRecord {
                rel_path: row.get(0)?,
                submode: row.get(1)?,
                surface: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        });
        match rows {
            Ok(mapped) => mapped.filter_map(Result::ok).collect(),
            Err(_) => Vec::new(),
        }
    }

    fn delete_design_artifact(&self, session_key: &str, rel_path: &str) -> std::io::Result<()> {
        let conn = self.writer.lock();
        conn.execute(
            "DELETE FROM session_design_artifacts WHERE session_key = ?1 AND rel_path = ?2",
            params![session_key, rel_path],
        )
        .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(())
    }
}
