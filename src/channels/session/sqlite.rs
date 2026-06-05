// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::channels::session::backend::{
    LoadedMessage, RewindStash, SessionBackend, SessionMetadata, SessionQuery,
};
use crate::providers::traits::ChatMessage;
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use parking_lot::Mutex;
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};

pub struct SqliteSessionBackend {
    conn: Mutex<Connection>,
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
             PRAGMA mmap_size = 4194304;",
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
             END;",
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

        Ok(Self {
            conn: Mutex::new(conn),
            db_path,
        })
    }

    fn append_inner(
        &self,
        session_key: &str,
        message: &ChatMessage,
        hidden_for_ui: bool,
    ) -> std::io::Result<()> {
        let conn = self.conn.lock();
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO sessions (session_key, role, content, created_at, hidden_for_ui)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                session_key,
                message.role,
                message.content,
                now,
                i32::from(hidden_for_ui),
            ],
        )
        .map_err(std::io::Error::other)?;

        conn.execute(
            "INSERT INTO session_metadata (session_key, created_at, last_activity, message_count)
             VALUES (?1, ?2, ?3, 1)
             ON CONFLICT(session_key) DO UPDATE SET
                last_activity = excluded.last_activity,
                message_count = message_count + 1",
            params![session_key, now, now],
        )
        .map_err(std::io::Error::other)?;

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
        let conn = self.conn.lock();

        let mut stmt = match conn
            .prepare(
                "SELECT role, content FROM sessions
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
            Ok(ChatMessage {
                role: row.get(0)?,
                content: row.get(1)?,
                metadata: Default::default(),
            })
        }) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };

        rows.filter_map(|r| r.ok()).collect()
    }

    fn load_with_tombstones(&self, session_key: &str) -> Vec<LoadedMessage> {
        let conn = self.conn.lock();
        let mut stmt = match conn.prepare(
            "SELECT id, role, content, tombstoned_at, hidden_for_ui FROM sessions
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
            Ok(LoadedMessage {
                id,
                message: ChatMessage {
                    role,
                    content,
                    metadata: Default::default(),
                },
                tombstoned_at,
                hidden_for_ui: hidden_for_ui != 0,
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
        let conn = self.conn.lock();

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
        let conn = self.conn.lock();
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
        let conn = self.conn.lock();
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

    fn cleanup_stale(&self, ttl_hours: u32) -> std::io::Result<usize> {
        let conn = self.conn.lock();
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
        for key in &stale_keys {
            let _ = conn.execute("DELETE FROM sessions WHERE session_key = ?1", params![key]);
            let _ = conn.execute(
                "DELETE FROM session_metadata WHERE session_key = ?1",
                params![key],
            );
        }

        Ok(count)
    }

    fn delete_session(&self, session_key: &str) -> std::io::Result<bool> {
        let conn = self.conn.lock();

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
        let conn = self.conn.lock();
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
        let conn = self.conn.lock();
        let name_val = if name.is_empty() { None } else { Some(name) };
        conn.execute(
            "UPDATE session_metadata SET name = ?1 WHERE session_key = ?2",
            params![name_val, session_key],
        )
        .map_err(std::io::Error::other)?;
        Ok(())
    }

    fn get_session_name(&self, session_key: &str) -> std::io::Result<Option<String>> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT name FROM session_metadata WHERE session_key = ?1",
            params![session_key],
            |row| row.get(0),
        )
        .map_err(std::io::Error::other)
    }

    fn set_session_work_dir(&self, session_key: &str, dir: &str) -> std::io::Result<()> {
        let conn = self.conn.lock();
        let dir_val = dir.trim();
        conn.execute(
            "UPDATE session_metadata SET work_dir = ?1 WHERE session_key = ?2",
            params![dir_val, session_key],
        )
        .map_err(std::io::Error::other)?;
        Ok(())
    }

    fn get_session_work_dir(&self, session_key: &str) -> std::io::Result<Option<String>> {
        let conn = self.conn.lock();
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

        let conn = self.conn.lock();
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
        let conn = self.conn.lock();
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
        let conn = self.conn.lock();
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
        let conn = self.conn.lock();
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
        let conn = self.conn.lock();
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
        let conn = self.conn.lock();
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
        let conn = self.conn.lock();
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
        let conn = self.conn.lock();
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
        let conn = self.conn.lock();
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
        let _ = conn.execute(
            "DELETE FROM session_rewind_stash WHERE rewind_id = ?1",
            params![rewind_id],
        );
        Some(stash)
    }

    fn latest_rewind_stash_for_session(
        &self,
        session_key: &str,
    ) -> Option<RewindStash> {
        let conn = self.conn.lock();
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
}
