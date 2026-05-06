// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const DB_FILE: &str = "token_saver/tracking.db";

static CONN: Mutex<Option<(PathBuf, Connection)>> = Mutex::new(None);

fn open_or_reuse(data_dir: &Path) -> Result<()> {
    let mut guard = CONN.lock().expect("token_saver tracking mutex poisoned");
    let target = data_dir.join(DB_FILE);
    if let Some((p, _)) = guard.as_ref() {
        if p == &target {
            return Ok(());
        }
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(&target)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS token_savings (
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
    *guard = Some((target, conn));
    Ok(())
}

pub fn record(
    command: &str,
    category: &str,
    raw_bytes: usize,
    compacted_bytes: usize,
    exit_code: i32,
    data_dir: &Path,
) -> Result<()> {
    if open_or_reuse(data_dir).is_err() {
        return Ok(());
    }
    let tokens_before = (raw_bytes.div_ceil(4)).saturating_add(4) as i64;
    let tokens_after = (compacted_bytes.div_ceil(4)).saturating_add(4) as i64;
    let tokens_saved = tokens_before.saturating_sub(tokens_after);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;

    let mut guard = CONN.lock().expect("token_saver tracking mutex poisoned");
    if let Some((_, conn)) = guard.as_mut() {
        let trimmed = command.chars().take(512).collect::<String>();
        conn.execute(
            "INSERT INTO token_savings (ts, command, category, tokens_before, tokens_after, tokens_saved, exit_code)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![ts, trimmed, category, tokens_before, tokens_after, tokens_saved, exit_code as i64],
        )?;
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

pub fn aggregate(window_seconds: u64, data_dir: &Path) -> Result<Aggregate> {
    open_or_reuse(data_dir)?;
    let mut guard = CONN.lock().expect("token_saver tracking mutex poisoned");
    let (_, conn) = guard.as_mut().ok_or_else(|| anyhow::anyhow!("no db"))?;
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
    open_or_reuse(data_dir)?;
    let mut guard = CONN.lock().expect("token_saver tracking mutex poisoned");
    let (_, conn) = guard.as_mut().ok_or_else(|| anyhow::anyhow!("no db"))?;
    let n = conn.execute("DELETE FROM token_savings", [])? as u64;
    Ok(n)
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
    open_or_reuse(data_dir)?;
    let mut guard = CONN.lock().expect("token_saver tracking mutex poisoned");
    let (_, conn) = guard.as_mut().ok_or_else(|| anyhow::anyhow!("no db"))?;
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
