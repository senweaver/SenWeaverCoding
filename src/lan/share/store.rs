// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::Path;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};

use super::types::{MyShareView, ShareWire};

pub struct ShareStore {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone)]
pub struct ShareRecord {
    pub id: String,
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: i64,
    pub content_hash: String,
    pub note: String,
    pub created_at: i64,
}

impl ShareStore {
    pub fn open(sen_dir: &Path) -> Result<Self> {
        let dir = sen_dir.join("lan");
        std::fs::create_dir_all(&dir).context("creating lan share store dir")?;
        let db_path = dir.join("lan_shares.db");
        let conn = Connection::open(&db_path).context("opening lan_shares.db")?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous  = NORMAL;
             PRAGMA busy_timeout = 5000;",
        )?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS shares (
                 id TEXT PRIMARY KEY,
                 name TEXT NOT NULL DEFAULT '',
                 path TEXT NOT NULL DEFAULT '',
                 is_dir INTEGER NOT NULL DEFAULT 0,
                 size INTEGER NOT NULL DEFAULT 0,
                 content_hash TEXT NOT NULL DEFAULT '',
                 note TEXT NOT NULL DEFAULT '',
                 created_at INTEGER NOT NULL DEFAULT 0
             );",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn upsert(&self, record: &ShareRecord) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO shares (id, name, path, is_dir, size, content_hash, note, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                path = excluded.path,
                is_dir = excluded.is_dir,
                size = excluded.size,
                content_hash = excluded.content_hash,
                note = excluded.note",
            params![
                record.id,
                record.name,
                record.path,
                i64::from(record.is_dir),
                record.size,
                record.content_hash,
                record.note,
                record.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn remove(&self, id: &str) -> bool {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM shares WHERE id = ?1", params![id])
            .map(|n| n > 0)
            .unwrap_or(false)
    }

    pub fn get(&self, id: &str) -> Option<ShareRecord> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT id, name, path, is_dir, size, content_hash, note, created_at
             FROM shares WHERE id = ?1",
            params![id],
            map_record,
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn list(&self) -> Vec<ShareRecord> {
        let conn = self.conn.lock();
        let mut stmt = match conn.prepare(
            "SELECT id, name, path, is_dir, size, content_hash, note, created_at
             FROM shares ORDER BY created_at DESC",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map([], map_record);
        match rows {
            Ok(rows) => rows.flatten().collect(),
            Err(_) => Vec::new(),
        }
    }

    pub fn my_views(&self) -> Vec<MyShareView> {
        self.list()
            .into_iter()
            .map(|r| MyShareView {
                id: r.id,
                name: r.name,
                path: r.path,
                is_dir: r.is_dir,
                size: r.size,
                note: r.note,
                created_at: r.created_at,
            })
            .collect()
    }

    pub fn wire_views(&self) -> Vec<ShareWire> {
        self.list()
            .into_iter()
            .map(|r| ShareWire {
                id: r.id,
                name: r.name,
                is_dir: r.is_dir,
                size: r.size,
                note: r.note,
                created_at: r.created_at,
            })
            .collect()
    }
}

fn map_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ShareRecord> {
    Ok(ShareRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        path: row.get(2)?,
        is_dir: row.get::<_, i64>(3)? != 0,
        size: row.get(4)?,
        content_hash: row.get(5)?,
        note: row.get(6)?,
        created_at: row.get(7)?,
    })
}
