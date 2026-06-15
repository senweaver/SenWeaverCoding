// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

const DB_FILE: &str = "token_saver/tracking.db";

const WRITE_QUEUE_CAPACITY: usize = 1024;

struct TrackingRow {
    db_path: PathBuf,
    ts: i64,
    command: String,
    category: String,
    tokens_before: i64,
    tokens_after: i64,
    tokens_saved: i64,
    exit_code: i64,
}

static WRITER: OnceLock<SyncSender<TrackingRow>> = OnceLock::new();
static DROPPED: AtomicU64 = AtomicU64::new(0);

fn open_db(db_path: &Path) -> Result<Connection> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(db_path)?;
    conn.execute_batch(
        "PRAGMA busy_timeout = 5000;
        CREATE TABLE IF NOT EXISTS token_savings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts INTEGER NOT NULL,
            command TEXT NOT NULL,
            category TEXT NOT NULL,
            tokens_before INTEGER NOT NULL,
            tokens_after INTEGER NOT NULL,
            tokens_saved INTEGER NOT NULL,
            exit_code INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_token_savings_ts ON token_savings(ts);
        CREATE INDEX IF NOT EXISTS idx_token_savings_category ON token_savings(category);",
    )?;
    Ok(conn)
}

fn writer_loop(rx: std::sync::mpsc::Receiver<TrackingRow>) {
    let mut conn: Option<(PathBuf, Connection)> = None;
    while let Ok(row) = rx.recv() {
        let reuse = conn
            .as_ref()
            .map(|(path, _)| path == &row.db_path)
            .unwrap_or(false);
        if !reuse {
            match open_db(&row.db_path) {
                Ok(opened) => conn = Some((row.db_path.clone(), opened)),
                Err(e) => {
                    tracing::warn!(
                        db = %row.db_path.display(),
                        error = %e,
                        "token saver tracking: failed to open database; dropping record"
                    );
                    conn = None;
                    continue;
                }
            }
        }
        if let Some((_, ref c)) = conn {
            if let Err(e) = c.execute(
                "INSERT INTO token_savings (ts, command, category, tokens_before, tokens_after, tokens_saved, exit_code)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    row.ts,
                    row.command,
                    row.category,
                    row.tokens_before,
                    row.tokens_after,
                    row.tokens_saved,
                    row.exit_code
                ],
            ) {
                tracing::warn!(error = %e, "token saver tracking: insert failed");
            }
        }
    }
}

fn writer_tx() -> &'static SyncSender<TrackingRow> {
    WRITER.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::sync_channel::<TrackingRow>(WRITE_QUEUE_CAPACITY);
        let spawned = std::thread::Builder::new()
            .name("token-saver-tracking".to_string())
            .spawn(move || writer_loop(rx));
        if let Err(e) = spawned {
            tracing::warn!(
                error = %e,
                "token saver tracking: writer thread failed to start; tracking disabled"
            );
        }
        tx
    })
}

#[allow(clippy::cast_possible_wrap)]
pub fn record(
    command: &str,
    category: &str,
    raw_bytes: usize,
    compacted_bytes: usize,
    exit_code: i32,
    data_dir: &Path,
) -> Result<()> {
    let tokens_before = (raw_bytes.div_ceil(4)).saturating_add(4) as i64;
    let tokens_after = (compacted_bytes.div_ceil(4)).saturating_add(4) as i64;
    let tokens_saved = tokens_before.saturating_sub(tokens_after);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;

    let row = TrackingRow {
        db_path: data_dir.join(DB_FILE),
        ts,
        command: command.chars().take(512).collect::<String>(),
        category: category.to_string(),
        tokens_before,
        tokens_after,
        tokens_saved,
        exit_code: i64::from(exit_code),
    };

    match writer_tx().try_send(row) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            let dropped = DROPPED.fetch_add(1, Ordering::Relaxed) + 1;
            if dropped == 1 || dropped % 100 == 0 {
                tracing::warn!(
                    dropped,
                    "token saver tracking: write queue full; dropping records"
                );
            }
        }
        Err(TrySendError::Disconnected(_)) => {}
    }
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Aggregate {
    pub commands: u64,
    pub tokens_before: u64,
    pub tokens_after: u64,
    pub tokens_saved: u64,
}

#[allow(clippy::cast_possible_wrap)]
pub fn aggregate(window_seconds: u64, data_dir: &Path) -> Result<Aggregate> {
    let conn = open_db(&data_dir.join(DB_FILE))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let cutoff = if window_seconds == 0 {
        0
    } else {
        now.saturating_sub(window_seconds as i64)
    };
    let mut stmt = conn.prepare(
        "SELECT COUNT(*), COALESCE(SUM(tokens_before),0), COALESCE(SUM(tokens_after),0), COALESCE(SUM(tokens_saved),0)
         FROM token_savings WHERE ts >= ?1",
    )?;
    let row = stmt.query_row(params![cutoff], |r| {
        Ok(Aggregate {
            commands: r.get::<_, i64>(0)? as u64,
            tokens_before: r.get::<_, i64>(1)? as u64,
            tokens_after: r.get::<_, i64>(2)? as u64,
            tokens_saved: r.get::<_, i64>(3)? as u64,
        })
    })?;
    Ok(row)
}

pub fn reset(data_dir: &Path) -> Result<u64> {
    let conn = open_db(&data_dir.join(DB_FILE))?;
    let n = conn.execute("DELETE FROM token_savings", [])? as u64;
    Ok(n)
}

pub async fn aggregate_async(window_seconds: u64, data_dir: &Path) -> Result<Aggregate> {
    let data_dir = data_dir.to_path_buf();
    tokio::task::spawn_blocking(move || aggregate(window_seconds, &data_dir)).await?
}

pub async fn reset_async(data_dir: &Path) -> Result<u64> {
    let data_dir = data_dir.to_path_buf();
    tokio::task::spawn_blocking(move || reset(&data_dir)).await?
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CategoryAggregate {
    pub category: String,
    pub hits: u64,
    pub raw_tokens: u64,
    pub compacted_tokens: u64,
    pub saved_tokens: u64,
}

impl CategoryAggregate {
    pub fn savings_pct(&self) -> f64 {
        if self.raw_tokens == 0 {
            0.0
        } else {
            (self.saved_tokens as f64 / self.raw_tokens as f64) * 100.0
        }
    }
}

pub fn aggregate_by_category(data_dir: &Path) -> Result<Vec<CategoryAggregate>> {
    let conn = open_db(&data_dir.join(DB_FILE))?;
    let mut stmt = conn.prepare(
        "SELECT category, COUNT(*) AS hits, COALESCE(SUM(tokens_before),0), \
         COALESCE(SUM(tokens_after),0), COALESCE(SUM(tokens_saved),0) \
         FROM token_savings GROUP BY category ORDER BY 5 DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(CategoryAggregate {
            category: r.get::<_, String>(0)?,
            hits: r.get::<_, i64>(1)? as u64,
            raw_tokens: r.get::<_, i64>(2)? as u64,
            compacted_tokens: r.get::<_, i64>(3)? as u64,
            saved_tokens: r.get::<_, i64>(4)? as u64,
        })
    })?;
    let out: Result<Vec<_>, _> = rows.collect();
    Ok(out?)
}

pub async fn aggregate_by_category_async(data_dir: &Path) -> Result<Vec<CategoryAggregate>> {
    let data_dir = data_dir.to_path_buf();
    tokio::task::spawn_blocking(move || aggregate_by_category(&data_dir)).await?
}
