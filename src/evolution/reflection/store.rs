// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Result;
use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};
use std::sync::Arc;

use super::types::{ReflectionRun, ReflectionRunStatus, ReflectionSummary};

const SCHEMA_BOOTSTRAP: &str = r"
CREATE TABLE IF NOT EXISTS reflection_runs (
    id                TEXT PRIMARY KEY,
    session_id        TEXT,
    trigger           TEXT NOT NULL,
    depth             TEXT NOT NULL,
    status            TEXT NOT NULL,
    model             TEXT,
    lessons_produced  INTEGER NOT NULL DEFAULT 0,
    turns_analyzed    INTEGER NOT NULL DEFAULT 0,
    summary           TEXT,
    error             TEXT,
    started_at        INTEGER NOT NULL,
    completed_at      INTEGER
);
CREATE INDEX IF NOT EXISTS idx_reflection_started ON reflection_runs(started_at);
CREATE INDEX IF NOT EXISTS idx_reflection_status ON reflection_runs(status);
";

#[derive(Clone)]
pub struct ReflectionStore {
    db: Arc<Mutex<Connection>>,
}

impl ReflectionStore {
    pub fn bind(db: Arc<Mutex<Connection>>) -> Result<Self> {
        {
            let conn = db.lock();
            conn.execute_batch(SCHEMA_BOOTSTRAP)?;
        }
        Ok(Self { db })
    }

    pub fn record_start(
        &self,
        id: &str,
        session_id: Option<&str>,
        trigger: &str,
        depth: &str,
        model: Option<&str>,
    ) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "INSERT INTO reflection_runs
             (id, session_id, trigger, depth, status, model,
              lessons_produced, turns_analyzed, summary, error, started_at, completed_at)
             VALUES (?1,?2,?3,?4,?5,?6,0,0,NULL,NULL,?7,NULL)",
            params![
                id,
                session_id,
                trigger,
                depth,
                ReflectionRunStatus::Running.as_str(),
                model,
                Utc::now().timestamp_millis(),
            ],
        )?;
        Ok(())
    }

    pub fn record_skipped(&self, id: &str, reason: &str) -> Result<()> {
        let conn = self.db.lock();
        let now_ms = Utc::now().timestamp_millis();
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM reflection_runs WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if exists > 0 {
            conn.execute(
                "UPDATE reflection_runs
                 SET status = ?2, error = ?3, completed_at = ?4
                 WHERE id = ?1",
                params![id, ReflectionRunStatus::Skipped.as_str(), reason, now_ms],
            )?;
        } else {
            conn.execute(
                "INSERT INTO reflection_runs
                 (id, session_id, trigger, depth, status, model,
                  lessons_produced, turns_analyzed, summary, error,
                  started_at, completed_at)
                 VALUES (?1,NULL,?2,?3,?4,NULL,0,0,NULL,?5,?6,?6)",
                params![
                    id,
                    "manual",
                    "quick",
                    ReflectionRunStatus::Skipped.as_str(),
                    reason,
                    now_ms,
                ],
            )?;
        }
        Ok(())
    }

    pub fn average_lessons_per_run(&self) -> Option<f64> {
        let conn = self.db.lock();
        let avg: Option<f64> = conn
            .query_row(
                "SELECT AVG(CAST(lessons_produced AS REAL))
                 FROM reflection_runs WHERE status = ?1",
                params![ReflectionRunStatus::Completed.as_str()],
                |row| row.get::<_, Option<f64>>(0),
            )
            .ok()
            .flatten();
        avg
    }

    pub fn last_run_at_and_status(&self) -> Option<(chrono::DateTime<Utc>, String)> {
        let conn = self.db.lock();
        conn.query_row(
            "SELECT started_at, status FROM reflection_runs ORDER BY started_at DESC LIMIT 1",
            [],
            |row| {
                let ms: i64 = row.get(0)?;
                let status: String = row.get(1)?;
                Ok((ms, status))
            },
        )
        .optional()
        .ok()
        .flatten()
        .and_then(|(ms, status)| {
            chrono::DateTime::<Utc>::from_timestamp_millis(ms).map(|ts| (ts, status))
        })
    }

    pub fn record_completion(
        &self,
        id: &str,
        status: ReflectionRunStatus,
        lessons_produced: u32,
        turns_analyzed: u32,
        summary: Option<&str>,
        error: Option<&str>,
    ) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "UPDATE reflection_runs
             SET status = ?2,
                 lessons_produced = ?3,
                 turns_analyzed = ?4,
                 summary = ?5,
                 error = ?6,
                 completed_at = ?7
             WHERE id = ?1",
            params![
                id,
                status.as_str(),
                i64::from(lessons_produced),
                i64::from(turns_analyzed),
                summary,
                error,
                Utc::now().timestamp_millis(),
            ],
        )?;
        Ok(())
    }

    pub fn list_recent(&self, limit: usize) -> Result<Vec<ReflectionRun>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, trigger, depth, status, model,
                    lessons_produced, turns_analyzed, summary, error,
                    started_at, completed_at
             FROM reflection_runs
             ORDER BY started_at DESC
             LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![i64::try_from(limit).unwrap_or(50)], row_to_run)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn latest(&self) -> Result<Option<ReflectionRun>> {
        let conn = self.db.lock();
        conn.query_row(
            "SELECT id, session_id, trigger, depth, status, model,
                    lessons_produced, turns_analyzed, summary, error,
                    started_at, completed_at
             FROM reflection_runs ORDER BY started_at DESC LIMIT 1",
            [],
            row_to_run,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn summary(&self) -> Result<ReflectionSummary> {
        let conn = self.db.lock();
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM reflection_runs", [], |row| row.get(0))
            .unwrap_or(0);
        let completed: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM reflection_runs WHERE status = ?1",
                params![ReflectionRunStatus::Completed.as_str()],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let failed: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM reflection_runs WHERE status = ?1",
                params![ReflectionRunStatus::Failed.as_str()],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let lessons_produced: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(lessons_produced), 0) FROM reflection_runs",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let last = conn
            .query_row(
                "SELECT started_at, status FROM reflection_runs ORDER BY started_at DESC LIMIT 1",
                [],
                |row| {
                    let ms: i64 = row.get(0)?;
                    let status: String = row.get(1)?;
                    Ok((ms, status))
                },
            )
            .optional()?;
        Ok(ReflectionSummary {
            total_runs: u64::try_from(total).unwrap_or(0),
            completed_runs: u64::try_from(completed).unwrap_or(0),
            failed_runs: u64::try_from(failed).unwrap_or(0),
            total_lessons_produced: u64::try_from(lessons_produced).unwrap_or(0),
            last_run_at: last
                .as_ref()
                .and_then(|(ms, _)| chrono::DateTime::from_timestamp_millis(*ms)),
            last_status: last.map(|(_, s)| s),
        })
    }
}

fn row_to_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReflectionRun> {
    let started_ms: i64 = row.get(10)?;
    let completed_ms: Option<i64> = row.get(11)?;
    let status_raw: String = row.get(4)?;
    Ok(ReflectionRun {
        id: row.get(0)?,
        session_id: row.get(1)?,
        trigger: row.get(2)?,
        depth: row.get(3)?,
        status: ReflectionRunStatus::parse(&status_raw),
        model: row.get(5)?,
        lessons_produced: u32::try_from(row.get::<_, i64>(6)?).unwrap_or(0),
        turns_analyzed: u32::try_from(row.get::<_, i64>(7)?).unwrap_or(0),
        summary: row.get(8)?,
        error: row.get(9)?,
        started_at: chrono::DateTime::from_timestamp_millis(started_ms).unwrap_or_else(Utc::now),
        completed_at: completed_ms.and_then(chrono::DateTime::<Utc>::from_timestamp_millis),
    })
}
