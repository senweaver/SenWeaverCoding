// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::Path;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde::Serialize;

pub struct LanStore {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageView {
    pub id: String,
    #[serde(rename = "peerId")]
    pub peer_id: String,
    pub direction: String,
    pub kind: String,
    pub body: String,
    #[serde(rename = "fileName")]
    pub file_name: Option<String>,
    #[serde(rename = "filePath")]
    pub file_path: Option<String>,
    #[serde(rename = "fileSize")]
    pub file_size: Option<i64>,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    pub read: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransferView {
    pub id: String,
    #[serde(rename = "peerId")]
    pub peer_id: String,
    pub direction: String,
    pub name: String,
    pub path: Option<String>,
    pub size: i64,
    pub transferred: i64,
    pub status: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversationView {
    #[serde(rename = "peerId")]
    pub peer_id: String,
    pub nickname: String,
    #[serde(rename = "lastMessage")]
    pub last_message: String,
    #[serde(rename = "lastTs")]
    pub last_ts: i64,
    pub unread: i64,
}

#[derive(Debug, Clone)]
pub struct NewMessage {
    pub id: String,
    pub peer_id: String,
    pub direction: String,
    pub kind: String,
    pub body: String,
    pub file_name: Option<String>,
    pub file_path: Option<String>,
    pub file_size: Option<i64>,
    pub created_at: i64,
    pub read: bool,
}

impl LanStore {
    pub fn open(sen_dir: &Path) -> Result<Self> {
        let dir = sen_dir.join("lan");
        std::fs::create_dir_all(&dir).context("creating lan store dir")?;
        let db_path = dir.join("lan_comms.db");
        let conn = Connection::open(&db_path).context("opening lan_comms.db")?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous  = NORMAL;
             PRAGMA busy_timeout = 5000;
             PRAGMA foreign_keys = ON;",
        )?;
        Self::init_schema(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn init_schema(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS peers (
                 user_id TEXT PRIMARY KEY,
                 nickname TEXT NOT NULL,
                 email TEXT,
                 last_ip TEXT,
                 public_key TEXT,
                 first_seen INTEGER NOT NULL,
                 last_seen INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS messages (
                 id TEXT PRIMARY KEY,
                 peer_id TEXT NOT NULL,
                 direction TEXT NOT NULL,
                 kind TEXT NOT NULL,
                 body TEXT NOT NULL DEFAULT '',
                 file_name TEXT,
                 file_path TEXT,
                 file_size INTEGER,
                 created_at INTEGER NOT NULL,
                 read INTEGER NOT NULL DEFAULT 0
             );
             CREATE INDEX IF NOT EXISTS idx_messages_peer ON messages(peer_id, created_at);
             CREATE TABLE IF NOT EXISTS transfers (
                 id TEXT PRIMARY KEY,
                 peer_id TEXT NOT NULL,
                 direction TEXT NOT NULL,
                 name TEXT NOT NULL,
                 path TEXT,
                 size INTEGER NOT NULL DEFAULT 0,
                 transferred INTEGER NOT NULL DEFAULT 0,
                 status TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_transfers_peer ON transfers(peer_id, created_at);
             CREATE TABLE IF NOT EXISTS pinned_keys (
                 user_id TEXT PRIMARY KEY,
                 public_key TEXT NOT NULL,
                 trusted INTEGER NOT NULL DEFAULT 0,
                 first_pinned INTEGER NOT NULL
             );",
        )?;
        let version: i64 = conn
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap_or(0);
        if version == 0 {
            conn.execute("DELETE FROM schema_version", [])?;
            conn.execute("INSERT INTO schema_version (version) VALUES (1)", [])?;
        }
        Ok(())
    }

    pub fn upsert_peer(
        &self,
        user_id: &str,
        nickname: &str,
        email: Option<&str>,
        last_ip: Option<&str>,
        public_key: Option<&str>,
        now: i64,
    ) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO peers (user_id, nickname, email, last_ip, public_key, first_seen, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT(user_id) DO UPDATE SET
                 nickname = excluded.nickname,
                 email = COALESCE(excluded.email, peers.email),
                 last_ip = COALESCE(excluded.last_ip, peers.last_ip),
                 public_key = COALESCE(excluded.public_key, peers.public_key),
                 last_seen = excluded.last_seen",
            params![user_id, nickname, email, last_ip, public_key, now],
        )?;
        Ok(())
    }

    pub fn peer_public_key(&self, user_id: &str) -> Option<String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT public_key FROM peers WHERE user_id = ?1",
            params![user_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
    }

    pub fn pinned_public_key(&self, user_id: &str) -> Option<String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT public_key FROM pinned_keys WHERE user_id = ?1",
            params![user_id],
            |row| row.get::<_, String>(0),
        )
        .ok()
    }

    pub fn pin_public_key(&self, user_id: &str, public_key: &str, now: i64) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO pinned_keys (user_id, public_key, trusted, first_pinned)
             VALUES (?1, ?2, 0, ?3)
             ON CONFLICT(user_id) DO NOTHING",
            params![user_id, public_key, now],
        )?;
        Ok(())
    }

    pub fn mark_peer_trusted(&self, user_id: &str, public_key: &str, now: i64) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO pinned_keys (user_id, public_key, trusted, first_pinned)
             VALUES (?1, ?2, 1, ?3)
             ON CONFLICT(user_id) DO UPDATE SET public_key = excluded.public_key, trusted = 1",
            params![user_id, public_key, now],
        )?;
        Ok(())
    }

    pub fn peer_nickname(&self, user_id: &str) -> Option<String> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT nickname FROM peers WHERE user_id = ?1",
            params![user_id],
            |row| row.get::<_, String>(0),
        )
        .ok()
    }

    pub fn record_message(&self, message: &NewMessage) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT OR REPLACE INTO messages
                (id, peer_id, direction, kind, body, file_name, file_path, file_size, created_at, read)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                message.id,
                message.peer_id,
                message.direction,
                message.kind,
                message.body,
                message.file_name,
                message.file_path,
                message.file_size,
                message.created_at,
                i64::from(message.read),
            ],
        )?;
        Ok(())
    }

    pub fn list_messages(&self, peer_id: &str, limit: i64) -> Result<Vec<MessageView>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, peer_id, direction, kind, body, file_name, file_path, file_size, created_at, read
             FROM messages WHERE peer_id = ?1 ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![peer_id, limit], Self::map_message)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        out.reverse();
        Ok(out)
    }

    pub fn mark_read(&self, peer_id: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE messages SET read = 1 WHERE peer_id = ?1 AND direction = 'in' AND read = 0",
            params![peer_id],
        )?;
        Ok(())
    }

    pub fn unread_total(&self) -> i64 {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE direction = 'in' AND read = 0",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0)
    }

    pub fn conversations(&self) -> Result<Vec<ConversationView>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT m.peer_id,
                    COALESCE(p.nickname, m.peer_id) AS nickname,
                    m.body,
                    m.kind,
                    m.created_at,
                    (SELECT COUNT(*) FROM messages mu
                       WHERE mu.peer_id = m.peer_id AND mu.direction = 'in' AND mu.read = 0) AS unread
             FROM messages m
             JOIN (
                 SELECT peer_id, MAX(created_at) AS max_ts FROM messages GROUP BY peer_id
             ) last ON last.peer_id = m.peer_id AND last.max_ts = m.created_at
             LEFT JOIN peers p ON p.user_id = m.peer_id
             ORDER BY m.created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let kind: String = row.get(3)?;
            let body: String = row.get(2)?;
            let preview = if kind == "file" && body.is_empty() {
                "[file]".to_string()
            } else {
                body
            };
            Ok(ConversationView {
                peer_id: row.get(0)?,
                nickname: row.get(1)?,
                last_message: preview,
                last_ts: row.get(4)?,
                unread: row.get(5)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn record_transfer(
        &self,
        id: &str,
        peer_id: &str,
        direction: &str,
        name: &str,
        path: Option<&str>,
        size: i64,
        status: &str,
        now: i64,
    ) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT OR REPLACE INTO transfers
                (id, peer_id, direction, name, path, size, transferred, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8, ?8)",
            params![id, peer_id, direction, name, path, size, status, now],
        )?;
        Ok(())
    }

    pub fn update_transfer(
        &self,
        id: &str,
        transferred: i64,
        status: &str,
        now: i64,
    ) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE transfers SET transferred = ?2, status = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, transferred, status, now],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_transfer(
        &self,
        id: &str,
        peer_id: &str,
        direction: &str,
        name: &str,
        path: Option<&str>,
        size: i64,
        transferred: i64,
        status: &str,
        now: i64,
    ) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO transfers
                (id, peer_id, direction, name, path, size, transferred, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                path = COALESCE(excluded.path, transfers.path),
                size = excluded.size,
                transferred = excluded.transferred,
                status = excluded.status,
                updated_at = excluded.updated_at",
            params![id, peer_id, direction, name, path, size, transferred, status, now],
        )?;
        Ok(())
    }

    pub fn list_transfers(&self, limit: i64) -> Result<Vec<TransferView>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, peer_id, direction, name, path, size, transferred, status, created_at, updated_at
             FROM transfers ORDER BY updated_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(TransferView {
                id: row.get(0)?,
                peer_id: row.get(1)?,
                direction: row.get(2)?,
                name: row.get(3)?,
                path: row.get(4)?,
                size: row.get(5)?,
                transferred: row.get(6)?,
                status: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    fn map_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageView> {
        Ok(MessageView {
            id: row.get(0)?,
            peer_id: row.get(1)?,
            direction: row.get(2)?,
            kind: row.get(3)?,
            body: row.get(4)?,
            file_name: row.get(5)?,
            file_path: row.get(6)?,
            file_size: row.get(7)?,
            created_at: row.get(8)?,
            read: row.get::<_, i64>(9)? != 0,
        })
    }
}
