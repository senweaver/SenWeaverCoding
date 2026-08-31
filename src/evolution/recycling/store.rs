// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Result;
use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::{Connection, params};
use std::sync::Arc;

use super::types::{RecycledExperience, RecycledExperienceOutcome};

const SCHEMA_BOOTSTRAP: &str = r"
CREATE TABLE IF NOT EXISTS recycled_experiences (
    id               TEXT PRIMARY KEY,
    session_id       TEXT NOT NULL,
    turn_id          TEXT NOT NULL,
    coding_mode      TEXT,
    outcome          TEXT NOT NULL,
    reward           REAL NOT NULL DEFAULT 0,
    headline         TEXT NOT NULL,
    context_excerpt  TEXT NOT NULL DEFAULT '',
    response_excerpt TEXT NOT NULL DEFAULT '',
    tools_summary    TEXT NOT NULL DEFAULT '',
    tags             TEXT NOT NULL DEFAULT '[]',
    shape_signature  TEXT NOT NULL DEFAULT '',
    hits             INTEGER NOT NULL DEFAULT 0,
    created_at       INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_recycled_signature ON recycled_experiences(shape_signature);
CREATE INDEX IF NOT EXISTS idx_recycled_created ON recycled_experiences(created_at);
CREATE INDEX IF NOT EXISTS idx_recycled_outcome ON recycled_experiences(outcome);
";

#[derive(Clone)]
pub struct RecyclingStore {
    db: Arc<Mutex<Connection>>,
}

impl RecyclingStore {
    pub fn bind(db: Arc<Mutex<Connection>>) -> Result<Self> {
        {
            let conn = db.lock();
            conn.execute_batch(SCHEMA_BOOTSTRAP)?;
        }
        Ok(Self { db })
    }

    pub fn upsert(&self, exp: &RecycledExperience) -> Result<()> {
        let conn = self.db.lock();
        let tags = serde_json::to_string(&exp.tags).unwrap_or_else(|_| "[]".into());
        conn.execute(
            "INSERT INTO recycled_experiences
             (id, session_id, turn_id, coding_mode, outcome, reward,
              headline, context_excerpt, response_excerpt, tools_summary,
              tags, shape_signature, hits, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
             ON CONFLICT(id) DO UPDATE SET
                outcome=excluded.outcome,
                reward=excluded.reward,
                headline=excluded.headline,
                context_excerpt=excluded.context_excerpt,
                response_excerpt=excluded.response_excerpt,
                tools_summary=excluded.tools_summary,
                tags=excluded.tags,
                shape_signature=excluded.shape_signature",
            params![
                exp.id,
                exp.session_id,
                exp.turn_id,
                exp.coding_mode,
                exp.outcome.as_str(),
                f64::from(exp.reward),
                exp.headline,
                exp.context_excerpt,
                exp.response_excerpt,
                exp.tools_summary,
                tags,
                exp.shape_signature,
                i64::try_from(exp.hits).unwrap_or(i64::MAX),
                exp.created_at.timestamp_millis(),
            ],
        )?;
        Ok(())
    }

    pub fn exists_for_signature(&self, signature: &str) -> Result<bool> {
        if signature.is_empty() {
            return Ok(false);
        }
        let conn = self.db.lock();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM recycled_experiences WHERE shape_signature = ?1",
            params![signature],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn list_recent(&self, limit: usize) -> Result<Vec<RecycledExperience>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, turn_id, coding_mode, outcome, reward,
                    headline, context_excerpt, response_excerpt, tools_summary,
                    tags, shape_signature, hits, created_at
             FROM recycled_experiences
             ORDER BY created_at DESC
             LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![i64::try_from(limit).unwrap_or(100)], row_to_experience)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn list_for_session(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<RecycledExperience>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, turn_id, coding_mode, outcome, reward,
                    headline, context_excerpt, response_excerpt, tools_summary,
                    tags, shape_signature, hits, created_at
             FROM recycled_experiences
             WHERE session_id = ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(
                params![session_id, i64::try_from(limit).unwrap_or(100)],
                row_to_experience,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn update_reward_for_turn(
        &self,
        turn_id: &str,
        reward: f32,
        outcome: RecycledExperienceOutcome,
    ) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "UPDATE recycled_experiences SET reward = ?1, outcome = ?2 WHERE turn_id = ?3",
            params![f64::from(reward), outcome.as_str(), turn_id],
        )?;
        Ok(())
    }

    pub fn bump_hits(&self, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let conn = self.db.lock();
        let mut stmt =
            conn.prepare("UPDATE recycled_experiences SET hits = hits + 1 WHERE id = ?1")?;
        for id in ids {
            stmt.execute(params![id])?;
        }
        Ok(())
    }

    pub fn count(&self) -> Result<u64> {
        let conn = self.db.lock();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM recycled_experiences", [], |row| {
                row.get(0)
            })
            .unwrap_or(0);
        Ok(u64::try_from(count).unwrap_or(0))
    }

    pub fn total_count(&self) -> Result<u64> {
        self.count()
    }

    pub fn count_since(&self, ts_ms: i64) -> Result<u64> {
        let conn = self.db.lock();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM recycled_experiences WHERE created_at >= ?1",
                params![ts_ms],
                |row| row.get(0),
            )
            .unwrap_or(0);
        Ok(u64::try_from(count).unwrap_or(0))
    }

    pub fn last_harvest_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        let conn = self.db.lock();
        let ts: Option<i64> = conn
            .query_row(
                "SELECT MAX(created_at) FROM recycled_experiences",
                [],
                |row| row.get(0),
            )
            .ok();
        ts.and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
    }

    pub fn prune_to_capacity(&self, max_retained: usize) -> Result<u64> {
        if max_retained == 0 {
            return Ok(0);
        }
        let conn = self.db.lock();
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM recycled_experiences", [], |row| {
                row.get(0)
            })
            .unwrap_or(0);
        let max = i64::try_from(max_retained).unwrap_or(i64::MAX);
        if total <= max {
            return Ok(0);
        }
        let drop_count = total - max;
        let removed = conn.execute(
            "DELETE FROM recycled_experiences
             WHERE id IN (
                SELECT id FROM recycled_experiences
                ORDER BY hits ASC, created_at ASC
                LIMIT ?1
             )",
            params![drop_count],
        )?;
        Ok(u64::try_from(removed).unwrap_or(0))
    }

    pub fn purge_all(&self) -> Result<u64> {
        let conn = self.db.lock();
        let removed = conn.execute("DELETE FROM recycled_experiences", [])?;
        Ok(u64::try_from(removed).unwrap_or(0))
    }
}

fn row_to_experience(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecycledExperience> {
    let tags_raw: String = row.get(10)?;
    let outcome_raw: String = row.get(4)?;
    let reward: f64 = row.get(5)?;
    let created_ms: i64 = row.get(13)?;
    #[allow(clippy::cast_possible_truncation)]
    Ok(RecycledExperience {
        id: row.get(0)?,
        session_id: row.get(1)?,
        turn_id: row.get(2)?,
        coding_mode: row.get(3)?,
        outcome: RecycledExperienceOutcome::parse(&outcome_raw),
        reward: reward as f32,
        headline: row.get(6)?,
        context_excerpt: row.get(7)?,
        response_excerpt: row.get(8)?,
        tools_summary: row.get(9)?,
        tags: serde_json::from_str(&tags_raw).unwrap_or_default(),
        shape_signature: row.get(11)?,
        hits: u64::try_from(row.get::<_, i64>(12)?).unwrap_or(0),
        created_at: chrono::DateTime::from_timestamp_millis(created_ms).unwrap_or_else(Utc::now),
    })
}
