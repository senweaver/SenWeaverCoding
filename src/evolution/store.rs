// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{Context, Result};
use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::types::{
    AuditEvent, CloudTarget, CloudTargetKind, EvolutionExportFormat, ExportRecord, Lesson,
    PersistenceStatus, Playbook, PurgeReport, PurgeScope, PushReceipt, ThumbVote, TurnRecord,
};

const SCHEMA_BOOTSTRAP: &str = r"
CREATE TABLE IF NOT EXISTS lessons (
    id            TEXT PRIMARY KEY,
    title         TEXT NOT NULL,
    body          TEXT NOT NULL,
    tags          TEXT NOT NULL DEFAULT '[]',
    coding_mode   TEXT,
    source_turn_ids TEXT NOT NULL DEFAULT '[]',
    hits          INTEGER NOT NULL DEFAULT 0,
    enabled       INTEGER NOT NULL DEFAULT 1,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_lessons_mode ON lessons(coding_mode);
CREATE INDEX IF NOT EXISTS idx_lessons_enabled ON lessons(enabled);

CREATE TABLE IF NOT EXISTS playbooks (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL,
    body        TEXT NOT NULL,
    coding_mode TEXT,
    hits        INTEGER NOT NULL DEFAULT 0,
    enabled     INTEGER NOT NULL DEFAULT 1,
    created_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS thumbs (
    id         TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    turn_id    TEXT NOT NULL,
    score      INTEGER NOT NULL,
    comment    TEXT,
    ts         INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_thumbs_turn ON thumbs(turn_id);
CREATE INDEX IF NOT EXISTS idx_thumbs_session ON thumbs(session_id);

CREATE TABLE IF NOT EXISTS exports (
    id                TEXT PRIMARY KEY,
    format            TEXT NOT NULL,
    path              TEXT NOT NULL,
    sample_count      INTEGER NOT NULL DEFAULT 0,
    size_bytes        INTEGER NOT NULL DEFAULT 0,
    md5               TEXT NOT NULL DEFAULT '',
    time_window_start INTEGER,
    time_window_end   INTEGER,
    created_at        INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_exports_created ON exports(created_at);

CREATE TABLE IF NOT EXISTS cloud_targets (
    id                            TEXT PRIMARY KEY,
    name                          TEXT NOT NULL,
    kind                          TEXT NOT NULL,
    endpoint                      TEXT NOT NULL,
    headers                       TEXT NOT NULL DEFAULT '{}',
    secret_ref                    TEXT,
    default_format                TEXT NOT NULL,
    enabled                       INTEGER NOT NULL DEFAULT 1,
    auto_push                     INTEGER NOT NULL DEFAULT 0,
    auto_push_min_samples         INTEGER NOT NULL DEFAULT 0,
    auto_push_min_interval_hours  INTEGER NOT NULL DEFAULT 0,
    last_pushed_at                INTEGER,
    created_at                    INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS push_receipts (
    id               TEXT PRIMARY KEY,
    export_id        TEXT NOT NULL,
    target_id        TEXT NOT NULL,
    status           TEXT NOT NULL,
    latency_ms       INTEGER,
    response_excerpt TEXT,
    ts               INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_push_target ON push_receipts(target_id);
CREATE INDEX IF NOT EXISTS idx_push_ts ON push_receipts(ts);

CREATE TABLE IF NOT EXISTS audit_events (
    id         TEXT PRIMARY KEY,
    kind       TEXT NOT NULL,
    turn_id    TEXT,
    session_id TEXT,
    payload    TEXT NOT NULL DEFAULT '{}',
    ts         INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_audit_kind ON audit_events(kind);
CREATE INDEX IF NOT EXISTS idx_audit_ts ON audit_events(ts);

CREATE TABLE IF NOT EXISTS turn_index (
    id                  TEXT PRIMARY KEY,
    session_id          TEXT NOT NULL,
    turn_idx            INTEGER NOT NULL,
    turn_class          TEXT NOT NULL,
    coding_mode         TEXT,
    provider            TEXT,
    model               TEXT,
    final_reward        REAL NOT NULL DEFAULT 0,
    has_reward          INTEGER NOT NULL DEFAULT 0,
    reward_thumbs       REAL,
    reward_next_state   REAL,
    reward_tool         REAL,
    reward_verification REAL,
    reward_cost         REAL,
    cost_usd            REAL NOT NULL DEFAULT 0,
    total_tokens        INTEGER NOT NULL DEFAULT 0,
    ts                  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_turn_index_session ON turn_index(session_id);
CREATE INDEX IF NOT EXISTS idx_turn_index_ts ON turn_index(ts);
CREATE INDEX IF NOT EXISTS idx_turn_index_mode ON turn_index(coding_mode);
";

pub struct Store {
    base_dir: PathBuf,
    turns_jsonl: PathBuf,
    events_jsonl: PathBuf,
    exports_dir: PathBuf,
    db: Arc<Mutex<Connection>>,
    persist_training_data: AtomicBool,
}

impl Store {
    pub fn open(base_dir: PathBuf, persist_training_data: bool) -> Result<Self> {
        std::fs::create_dir_all(&base_dir)
            .with_context(|| format!("create evolution dir {}", base_dir.display()))?;
        let exports_dir = base_dir.join("exports");
        std::fs::create_dir_all(&exports_dir)
            .with_context(|| format!("create evolution exports dir {}", exports_dir.display()))?;
        let turns_jsonl = base_dir.join("turns.jsonl");
        let events_jsonl = base_dir.join("events.jsonl");
        let db_path = base_dir.join("evolution.db");
        let conn = Connection::open(&db_path)
            .with_context(|| format!("open evolution.db at {}", db_path.display()))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous  = NORMAL;
             PRAGMA busy_timeout = 5000;
             PRAGMA temp_store   = MEMORY;",
        )
        .context("apply pragma to evolution.db")?;
        conn.execute_batch(SCHEMA_BOOTSTRAP)
            .context("bootstrap evolution.db schema")?;
        ensure_turn_index_columns(&conn).context("ensure turn_index reward columns")?;
        Ok(Self {
            base_dir,
            turns_jsonl,
            events_jsonl,
            exports_dir,
            db: Arc::new(Mutex::new(conn)),
            persist_training_data: AtomicBool::new(persist_training_data),
        })
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    pub fn exports_dir(&self) -> &Path {
        &self.exports_dir
    }

    pub fn shared_connection(&self) -> Arc<Mutex<Connection>> {
        Arc::clone(&self.db)
    }

    pub fn turns_path(&self) -> &Path {
        &self.turns_jsonl
    }

    pub fn events_path(&self) -> &Path {
        &self.events_jsonl
    }

    pub fn persist_training_data(&self) -> bool {
        self.persist_training_data.load(Ordering::Relaxed)
    }

    pub fn set_persist_training_data(&self, value: bool) {
        self.persist_training_data.store(value, Ordering::Relaxed);
    }

    pub fn append_turn(&self, turn: &TurnRecord) -> Result<()> {
        {
            let conn = self.db.lock();
            upsert_turn_index(&conn, turn)?;
        }
        if !self.persist_training_data() {
            return Ok(());
        }
        let line = serde_json::to_string(turn).context("serialise TurnRecord")?;
        append_line(&self.turns_jsonl, &line)?;
        Ok(())
    }

    pub fn update_turn_reward(&self, turn_id: &str, reward: &super::types::Reward) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "UPDATE turn_index SET
                final_reward        = ?1,
                has_reward          = 1,
                reward_thumbs       = ?2,
                reward_next_state   = ?3,
                reward_tool         = ?4,
                reward_verification = ?5,
                reward_cost         = ?6
             WHERE id = ?7",
            params![
                f64::from(reward.final_score),
                reward.thumbs.map(f64::from),
                reward.next_state.map(f64::from),
                reward.tool.map(f64::from),
                reward.verification.map(f64::from),
                reward.cost.map(f64::from),
                turn_id,
            ],
        )?;
        Ok(())
    }

    pub fn load_turn_reward(&self, turn_id: &str) -> Result<Option<super::types::Reward>> {
        let conn = self.db.lock();
        let row = conn
            .query_row(
                "SELECT final_reward, has_reward, reward_thumbs, reward_next_state,
                        reward_tool, reward_verification, reward_cost
                 FROM turn_index WHERE id = ?1",
                params![turn_id],
                |row| {
                    let final_score: f64 = row.get(0)?;
                    let has_reward: i64 = row.get(1)?;
                    let thumbs: Option<f64> = row.get(2)?;
                    let next_state: Option<f64> = row.get(3)?;
                    let tool: Option<f64> = row.get(4)?;
                    let verification: Option<f64> = row.get(5)?;
                    let cost: Option<f64> = row.get(6)?;
                    #[allow(clippy::cast_possible_truncation)]
                    Ok(super::types::Reward {
                        thumbs: thumbs.map(|v| v as f32),
                        next_state: next_state.map(|v| v as f32),
                        tool: tool.map(|v| v as f32),
                        verification: verification.map(|v| v as f32),
                        cost: cost.map(|v| v as f32),
                        final_score: final_score as f32,
                        loss_mask: u8::from(has_reward != 0),
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    pub fn merge_turn_signal(
        &self,
        turn_id: &str,
        signal: &super::types::SignalScore,
        weights: &super::types::EvolutionSignalWeights,
    ) -> Result<super::types::Reward> {
        let mut reward = self.load_turn_reward(turn_id)?.unwrap_or_default();
        super::reward::merge_signal(&mut reward, signal, weights);
        self.update_turn_reward(turn_id, &reward)?;
        Ok(reward)
    }

    pub fn find_turn_record(&self, turn_id: &str) -> Result<Option<TurnRecord>> {
        if !self.turns_jsonl.is_file() {
            return Ok(None);
        }
        let file = std::fs::File::open(&self.turns_jsonl)
            .with_context(|| format!("open {}", self.turns_jsonl.display()))?;
        let reader = std::io::BufReader::new(file);
        use std::io::BufRead as _;
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let candidate: TurnRecord = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if candidate.id == turn_id {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }

    pub fn find_turns_for_session(&self, session_id: &str, limit: usize) -> Result<Vec<TurnRecord>> {
        if !self.turns_jsonl.is_file() {
            return Ok(Vec::new());
        }
        let file = std::fs::File::open(&self.turns_jsonl)
            .with_context(|| format!("open {}", self.turns_jsonl.display()))?;
        let reader = std::io::BufReader::new(file);
        use std::io::BufRead as _;
        let mut matched: Vec<TurnRecord> = Vec::new();
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let candidate: TurnRecord = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if candidate.session_id == session_id {
                matched.push(candidate);
            }
        }
        matched.sort_by(|a, b| b.ts.cmp(&a.ts));
        matched.truncate(limit);
        Ok(matched)
    }

    pub fn find_recent_turns(&self, limit: usize) -> Result<Vec<TurnRecord>> {
        if !self.turns_jsonl.is_file() {
            return Ok(Vec::new());
        }
        let file = std::fs::File::open(&self.turns_jsonl)
            .with_context(|| format!("open {}", self.turns_jsonl.display()))?;
        let reader = std::io::BufReader::new(file);
        use std::io::BufRead as _;
        let mut all: Vec<TurnRecord> = Vec::new();
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(candidate) = serde_json::from_str::<TurnRecord>(trimmed) {
                all.push(candidate);
            }
        }
        all.sort_by(|a, b| b.ts.cmp(&a.ts));
        all.truncate(limit);
        Ok(all)
    }

    pub fn for_each_turn<F>(&self, mut on_turn: F) -> Result<u64>
    where
        F: FnMut(TurnRecord) -> Result<()>,
    {
        if !self.turns_jsonl.is_file() {
            return Ok(0);
        }
        let file = std::fs::File::open(&self.turns_jsonl)
            .with_context(|| format!("open {}", self.turns_jsonl.display()))?;
        let reader = std::io::BufReader::new(file);
        use std::io::BufRead as _;
        let mut count: u64 = 0;
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let turn: TurnRecord = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(_) => continue,
            };
            on_turn(turn)?;
            count = count.saturating_add(1);
        }
        Ok(count)
    }

    pub fn lesson_exists_by_title(
        &self,
        coding_mode: Option<&str>,
        title: &str,
    ) -> Result<bool> {
        let conn = self.db.lock();
        let normalized = title.trim().to_lowercase();
        let count: i64 = match coding_mode {
            Some(mode) => conn.query_row(
                "SELECT COUNT(*) FROM lessons
                 WHERE LOWER(TRIM(title)) = ?1
                   AND (coding_mode IS ?2 OR LOWER(coding_mode) = LOWER(?2))",
                params![normalized, mode],
                |row| row.get(0),
            )?,
            None => conn.query_row(
                "SELECT COUNT(*) FROM lessons
                 WHERE LOWER(TRIM(title)) = ?1 AND coding_mode IS NULL",
                params![normalized],
                |row| row.get(0),
            )?,
        };
        Ok(count > 0)
    }

    pub fn append_audit(&self, event: &AuditEvent) -> Result<()> {
        {
            let conn = self.db.lock();
            conn.execute(
                "INSERT OR REPLACE INTO audit_events (id, kind, turn_id, session_id, payload, ts)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    event.id,
                    event.kind,
                    event.turn_id,
                    event.session_id,
                    event.payload.to_string(),
                    event.ts.timestamp_millis(),
                ],
            )?;
        }
        if !self.persist_training_data() {
            return Ok(());
        }
        let line = serde_json::to_string(event).context("serialise AuditEvent")?;
        append_line(&self.events_jsonl, &line)?;
        Ok(())
    }

    pub fn upsert_lesson(&self, lesson: &Lesson) -> Result<()> {
        let conn = self.db.lock();
        let tags = serde_json::to_string(&lesson.tags).unwrap_or_else(|_| "[]".into());
        let source = serde_json::to_string(&lesson.source_turn_ids).unwrap_or_else(|_| "[]".into());
        conn.execute(
            "INSERT INTO lessons (id, title, body, tags, coding_mode, source_turn_ids, hits, enabled, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                body = excluded.body,
                tags = excluded.tags,
                coding_mode = excluded.coding_mode,
                source_turn_ids = excluded.source_turn_ids,
                enabled = excluded.enabled,
                updated_at = excluded.updated_at",
            params![
                lesson.id,
                lesson.title,
                lesson.body,
                tags,
                lesson.coding_mode,
                source,
                i64::try_from(lesson.hits).unwrap_or(i64::MAX),
                i64::from(lesson.enabled),
                lesson.created_at.timestamp_millis(),
                lesson.updated_at.timestamp_millis(),
            ],
        )?;
        Ok(())
    }

    pub fn delete_lesson(&self, id: &str) -> Result<bool> {
        let conn = self.db.lock();
        let n = conn.execute("DELETE FROM lessons WHERE id = ?1", params![id])?;
        Ok(n > 0)
    }

    pub fn list_lessons(&self, only_enabled: bool) -> Result<Vec<Lesson>> {
        let conn = self.db.lock();
        let mut stmt = if only_enabled {
            conn.prepare(
                "SELECT id, title, body, tags, coding_mode, source_turn_ids, hits, enabled, created_at, updated_at
                 FROM lessons WHERE enabled = 1 ORDER BY updated_at DESC",
            )?
        } else {
            conn.prepare(
                "SELECT id, title, body, tags, coding_mode, source_turn_ids, hits, enabled, created_at, updated_at
                 FROM lessons ORDER BY updated_at DESC",
            )?
        };
        let rows = stmt
            .query_map([], |row| {
                let tags_raw: String = row.get(3)?;
                let source_raw: String = row.get(5)?;
                let created_ms: i64 = row.get(8)?;
                let updated_ms: i64 = row.get(9)?;
                Ok(Lesson {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    body: row.get(2)?,
                    tags: serde_json::from_str(&tags_raw).unwrap_or_default(),
                    coding_mode: row.get(4)?,
                    source_turn_ids: serde_json::from_str(&source_raw).unwrap_or_default(),
                    hits: u64::try_from(row.get::<_, i64>(6)?).unwrap_or(0),
                    enabled: row.get::<_, i64>(7)? != 0,
                    created_at: chrono::DateTime::from_timestamp_millis(created_ms)
                        .unwrap_or_else(Utc::now),
                    updated_at: chrono::DateTime::from_timestamp_millis(updated_ms)
                        .unwrap_or_else(Utc::now),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn bump_lesson_hits(&self, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let conn = self.db.lock();
        let mut stmt = conn.prepare("UPDATE lessons SET hits = hits + 1 WHERE id = ?1")?;
        for id in ids {
            stmt.execute(params![id])?;
        }
        Ok(())
    }

    pub fn upsert_playbook(&self, playbook: &Playbook) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "INSERT INTO playbooks (id, title, body, coding_mode, hits, enabled, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                body = excluded.body,
                coding_mode = excluded.coding_mode,
                enabled = excluded.enabled",
            params![
                playbook.id,
                playbook.title,
                playbook.body,
                playbook.coding_mode,
                i64::try_from(playbook.hits).unwrap_or(i64::MAX),
                i64::from(playbook.enabled),
                playbook.created_at.timestamp_millis(),
            ],
        )?;
        Ok(())
    }

    pub fn list_playbooks(&self) -> Result<Vec<Playbook>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(
            "SELECT id, title, body, coding_mode, hits, enabled, created_at FROM playbooks ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                let created_ms: i64 = row.get(6)?;
                Ok(Playbook {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    body: row.get(2)?,
                    coding_mode: row.get(3)?,
                    hits: u64::try_from(row.get::<_, i64>(4)?).unwrap_or(0),
                    enabled: row.get::<_, i64>(5)? != 0,
                    created_at: chrono::DateTime::from_timestamp_millis(created_ms)
                        .unwrap_or_else(Utc::now),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn record_thumb(&self, vote: &ThumbVote) -> Result<()> {
        if !self.persist_training_data() {
            self.append_audit(&AuditEvent {
                id: format!("ev_{}", uuid::Uuid::new_v4().simple()),
                kind: "thumb_skipped".into(),
                turn_id: Some(vote.turn_id.clone()),
                session_id: Some(vote.session_id.clone()),
                payload: serde_json::json!({
                    "score": vote.score,
                    "reason": "persist_training_data_disabled"
                }),
                ts: Utc::now(),
            })?;
            return Ok(());
        }
        let conn = self.db.lock();
        conn.execute(
            "INSERT OR REPLACE INTO thumbs (id, session_id, turn_id, score, comment, ts)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                vote.id,
                vote.session_id,
                vote.turn_id,
                i64::from(vote.score),
                vote.comment,
                vote.ts.timestamp_millis(),
            ],
        )?;
        Ok(())
    }

    pub fn upsert_export(&self, record: &ExportRecord) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "INSERT OR REPLACE INTO exports
             (id, format, path, sample_count, size_bytes, md5, time_window_start, time_window_end, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                record.id,
                record.format.as_str(),
                record.path,
                i64::try_from(record.sample_count).unwrap_or(i64::MAX),
                i64::try_from(record.size_bytes).unwrap_or(i64::MAX),
                record.md5,
                record.time_window_start.map(|t| t.timestamp_millis()),
                record.time_window_end.map(|t| t.timestamp_millis()),
                record.created_at.timestamp_millis(),
            ],
        )?;
        Ok(())
    }

    pub fn list_exports(&self) -> Result<Vec<ExportRecord>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(
            "SELECT id, format, path, sample_count, size_bytes, md5, time_window_start, time_window_end, created_at
             FROM exports ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                let format_raw: String = row.get(1)?;
                let format = EvolutionExportFormat::parse(&format_raw)
                    .unwrap_or(EvolutionExportFormat::OpenaiSft);
                let start_ms: Option<i64> = row.get(6)?;
                let end_ms: Option<i64> = row.get(7)?;
                let created_ms: i64 = row.get(8)?;
                Ok(ExportRecord {
                    id: row.get(0)?,
                    format,
                    path: row.get(2)?,
                    sample_count: u64::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
                    size_bytes: u64::try_from(row.get::<_, i64>(4)?).unwrap_or(0),
                    md5: row.get(5)?,
                    time_window_start: start_ms
                        .and_then(chrono::DateTime::<Utc>::from_timestamp_millis),
                    time_window_end: end_ms
                        .and_then(chrono::DateTime::<Utc>::from_timestamp_millis),
                    created_at: chrono::DateTime::from_timestamp_millis(created_ms)
                        .unwrap_or_else(Utc::now),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_export(&self, id: &str) -> Result<Option<ExportRecord>> {
        let conn = self.db.lock();
        conn.query_row(
            "SELECT id, format, path, sample_count, size_bytes, md5, time_window_start, time_window_end, created_at
             FROM exports WHERE id = ?1",
            params![id],
            |row| {
                let format_raw: String = row.get(1)?;
                let format = EvolutionExportFormat::parse(&format_raw)
                    .unwrap_or(EvolutionExportFormat::OpenaiSft);
                let start_ms: Option<i64> = row.get(6)?;
                let end_ms: Option<i64> = row.get(7)?;
                let created_ms: i64 = row.get(8)?;
                Ok(ExportRecord {
                    id: row.get(0)?,
                    format,
                    path: row.get(2)?,
                    sample_count: u64::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
                    size_bytes: u64::try_from(row.get::<_, i64>(4)?).unwrap_or(0),
                    md5: row.get(5)?,
                    time_window_start: start_ms
                        .and_then(chrono::DateTime::<Utc>::from_timestamp_millis),
                    time_window_end: end_ms
                        .and_then(chrono::DateTime::<Utc>::from_timestamp_millis),
                    created_at: chrono::DateTime::from_timestamp_millis(created_ms)
                        .unwrap_or_else(Utc::now),
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn delete_export(&self, id: &str) -> Result<u64> {
        let record = match self.get_export(id)? {
            Some(r) => r,
            None => return Ok(0),
        };
        let mut freed = 0u64;
        let path = PathBuf::from(&record.path);
        if path.is_file() {
            if let Ok(meta) = std::fs::metadata(&path) {
                freed = freed.saturating_add(meta.len());
            }
            let _ = std::fs::remove_file(&path);
        }
        {
            let conn = self.db.lock();
            conn.execute("DELETE FROM exports WHERE id = ?1", params![id])?;
            conn.execute("DELETE FROM push_receipts WHERE export_id = ?1", params![id])?;
        }
        Ok(freed)
    }

    pub fn upsert_cloud_target(&self, target: &CloudTarget) -> Result<()> {
        let conn = self.db.lock();
        let headers = serde_json::to_string(&target.headers).unwrap_or_else(|_| "{}".into());
        conn.execute(
            "INSERT INTO cloud_targets (id, name, kind, endpoint, headers, secret_ref, default_format, enabled, auto_push, auto_push_min_samples, auto_push_min_interval_hours, last_pushed_at, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                kind = excluded.kind,
                endpoint = excluded.endpoint,
                headers = excluded.headers,
                secret_ref = excluded.secret_ref,
                default_format = excluded.default_format,
                enabled = excluded.enabled,
                auto_push = excluded.auto_push,
                auto_push_min_samples = excluded.auto_push_min_samples,
                auto_push_min_interval_hours = excluded.auto_push_min_interval_hours,
                last_pushed_at = excluded.last_pushed_at",
            params![
                target.id,
                target.name,
                target.kind.as_str(),
                target.endpoint,
                headers,
                target.secret_ref,
                target.default_format.as_str(),
                i64::from(target.enabled),
                i64::from(target.auto_push),
                i64::from(target.auto_push_min_samples),
                i64::from(target.auto_push_min_interval_hours),
                target.last_pushed_at.map(|t| t.timestamp_millis()),
                target.created_at.timestamp_millis(),
            ],
        )?;
        Ok(())
    }

    pub fn list_cloud_targets(&self) -> Result<Vec<CloudTarget>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, endpoint, headers, secret_ref, default_format, enabled, auto_push, auto_push_min_samples, auto_push_min_interval_hours, last_pushed_at, created_at
             FROM cloud_targets ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                let kind_raw: String = row.get(2)?;
                let kind = CloudTargetKind::parse(&kind_raw).unwrap_or(CloudTargetKind::Webhook);
                let headers_raw: String = row.get(4)?;
                let format_raw: String = row.get(6)?;
                let format = EvolutionExportFormat::parse(&format_raw)
                    .unwrap_or(EvolutionExportFormat::OpenaiSft);
                let last_ms: Option<i64> = row.get(11)?;
                let created_ms: i64 = row.get(12)?;
                Ok(CloudTarget {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    kind,
                    endpoint: row.get(3)?,
                    headers: serde_json::from_str(&headers_raw).unwrap_or_default(),
                    secret_ref: row.get(5)?,
                    default_format: format,
                    enabled: row.get::<_, i64>(7)? != 0,
                    auto_push: row.get::<_, i64>(8)? != 0,
                    auto_push_min_samples: u32::try_from(row.get::<_, i64>(9)?).unwrap_or(0),
                    auto_push_min_interval_hours: u32::try_from(row.get::<_, i64>(10)?)
                        .unwrap_or(0),
                    last_pushed_at: last_ms
                        .and_then(chrono::DateTime::<Utc>::from_timestamp_millis),
                    created_at: chrono::DateTime::from_timestamp_millis(created_ms)
                        .unwrap_or_else(Utc::now),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_cloud_target(&self, id: &str) -> Result<Option<CloudTarget>> {
        Ok(self.list_cloud_targets()?.into_iter().find(|t| t.id == id))
    }

    pub fn delete_cloud_target(&self, id: &str) -> Result<bool> {
        let conn = self.db.lock();
        let n = conn.execute("DELETE FROM cloud_targets WHERE id = ?1", params![id])?;
        Ok(n > 0)
    }

    pub fn set_target_last_pushed(
        &self,
        id: &str,
        ts: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "UPDATE cloud_targets SET last_pushed_at = ?1 WHERE id = ?2",
            params![ts.timestamp_millis(), id],
        )?;
        Ok(())
    }

    pub fn record_push_receipt(&self, receipt: &PushReceipt) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "INSERT OR REPLACE INTO push_receipts (id, export_id, target_id, status, latency_ms, response_excerpt, ts)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                receipt.id,
                receipt.export_id,
                receipt.target_id,
                receipt.status,
                receipt.latency_ms.map(|v| i64::try_from(v).unwrap_or(i64::MAX)),
                receipt.response_excerpt,
                receipt.ts.timestamp_millis(),
            ],
        )?;
        Ok(())
    }

    pub fn list_push_receipts(&self, limit: usize) -> Result<Vec<PushReceipt>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(
            "SELECT id, export_id, target_id, status, latency_ms, response_excerpt, ts FROM push_receipts ORDER BY ts DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![i64::try_from(limit).unwrap_or(100)], |row| {
                let latency_ms: Option<i64> = row.get(4)?;
                let ts_ms: i64 = row.get(6)?;
                Ok(PushReceipt {
                    id: row.get(0)?,
                    export_id: row.get(1)?,
                    target_id: row.get(2)?,
                    status: row.get(3)?,
                    latency_ms: latency_ms.and_then(|v| u64::try_from(v).ok()),
                    response_excerpt: row.get(5)?,
                    ts: chrono::DateTime::from_timestamp_millis(ts_ms).unwrap_or_else(Utc::now),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn persistence_status(&self) -> Result<PersistenceStatus> {
        let turns_file_size = file_size_or_zero(&self.turns_jsonl);
        let events_file_size = file_size_or_zero(&self.events_jsonl);
        let conn = self.db.lock();
        let turns_count: u64 = conn
            .query_row("SELECT COUNT(*) FROM turn_index", [], |row| row.get(0))
            .map(|v: i64| u64::try_from(v).unwrap_or(0))
            .unwrap_or(0);
        let exports_count: u64 = conn
            .query_row("SELECT COUNT(*) FROM exports", [], |row| row.get(0))
            .map(|v: i64| u64::try_from(v).unwrap_or(0))
            .unwrap_or(0);
        let exports_total_bytes: u64 = conn
            .query_row("SELECT COALESCE(SUM(size_bytes), 0) FROM exports", [], |row| {
                row.get(0)
            })
            .map(|v: i64| u64::try_from(v).unwrap_or(0))
            .unwrap_or(0);
        let push_receipts_count: u64 = conn
            .query_row("SELECT COUNT(*) FROM push_receipts", [], |row| row.get(0))
            .map(|v: i64| u64::try_from(v).unwrap_or(0))
            .unwrap_or(0);
        Ok(PersistenceStatus {
            persist_training_data: self.persist_training_data(),
            turns_file_size,
            turns_count,
            events_file_size,
            exports_total_bytes,
            exports_count,
            push_receipts_count,
        })
    }

    pub fn purge(&self, scope: PurgeScope, before_ms: Option<i64>) -> Result<PurgeReport> {
        let mut report = PurgeReport::default();
        let purge_turns = matches!(scope, PurgeScope::Turns | PurgeScope::All);
        let purge_exports = matches!(scope, PurgeScope::Exports | PurgeScope::All);
        let purge_push = matches!(scope, PurgeScope::PushHistory | PurgeScope::All);
        let purge_events = matches!(scope, PurgeScope::Events | PurgeScope::All);

        if purge_turns {
            report.turns = self.purge_turns(before_ms, &mut report.freed_bytes)?;
        }
        if purge_exports {
            report.exports = self.purge_exports(before_ms, &mut report.freed_bytes)?;
        }
        if purge_push {
            report.push_history = self.purge_push_receipts(before_ms)?;
        }
        if purge_events {
            report.events = self.purge_events(before_ms, &mut report.freed_bytes)?;
        }

        let audit = AuditEvent {
            id: format!("ev_{}", uuid::Uuid::new_v4().simple()),
            kind: "purge".into(),
            turn_id: None,
            session_id: None,
            payload: serde_json::json!({
                "scope": match scope {
                    PurgeScope::Turns => "turns",
                    PurgeScope::Exports => "exports",
                    PurgeScope::PushHistory => "push_history",
                    PurgeScope::Events => "events",
                    PurgeScope::All => "all",
                },
                "before_ms": before_ms,
                "report": &report,
            }),
            ts: Utc::now(),
        };
        let _ = self.append_audit(&audit);
        Ok(report)
    }

    fn purge_turns(&self, before_ms: Option<i64>, freed: &mut u64) -> Result<u64> {
        let removed_index = {
            let conn = self.db.lock();
            match before_ms {
                Some(cutoff) => conn.execute(
                    "DELETE FROM turn_index WHERE ts < ?1",
                    params![cutoff],
                )?,
                None => conn.execute("DELETE FROM turn_index", [])?,
            }
        };
        let line_removed = match before_ms {
            Some(cutoff) => rewrite_turns_jsonl_before(&self.turns_jsonl, cutoff, freed)?,
            None => {
                if self.turns_jsonl.exists() {
                    if let Ok(meta) = std::fs::metadata(&self.turns_jsonl) {
                        *freed = freed.saturating_add(meta.len());
                    }
                    let _ = std::fs::remove_file(&self.turns_jsonl);
                }
                0
            }
        };
        Ok(u64::try_from(removed_index).unwrap_or(0).max(line_removed))
    }

    fn purge_events(&self, before_ms: Option<i64>, freed: &mut u64) -> Result<u64> {
        let removed = {
            let conn = self.db.lock();
            match before_ms {
                Some(cutoff) => conn.execute(
                    "DELETE FROM audit_events WHERE ts < ?1",
                    params![cutoff],
                )?,
                None => conn.execute("DELETE FROM audit_events", [])?,
            }
        };
        let line_removed = match before_ms {
            Some(cutoff) => rewrite_events_jsonl_before(&self.events_jsonl, cutoff, freed)?,
            None => {
                if self.events_jsonl.exists() {
                    if let Ok(meta) = std::fs::metadata(&self.events_jsonl) {
                        *freed = freed.saturating_add(meta.len());
                    }
                    let _ = std::fs::remove_file(&self.events_jsonl);
                }
                0
            }
        };
        Ok(u64::try_from(removed).unwrap_or(0).max(line_removed))
    }

    fn purge_exports(&self, before_ms: Option<i64>, freed: &mut u64) -> Result<u64> {
        let records = self.list_exports()?;
        let mut removed: u64 = 0;
        for record in records {
            if let Some(cutoff) = before_ms {
                if record.created_at.timestamp_millis() >= cutoff {
                    continue;
                }
            }
            let path = PathBuf::from(&record.path);
            if path.is_file() {
                if let Ok(meta) = std::fs::metadata(&path) {
                    *freed = freed.saturating_add(meta.len());
                }
                let _ = std::fs::remove_file(&path);
            }
            {
                let conn = self.db.lock();
                conn.execute("DELETE FROM exports WHERE id = ?1", params![record.id])?;
                conn.execute(
                    "DELETE FROM push_receipts WHERE export_id = ?1",
                    params![record.id],
                )?;
            }
            removed = removed.saturating_add(1);
        }
        Ok(removed)
    }

    fn purge_push_receipts(&self, before_ms: Option<i64>) -> Result<u64> {
        let conn = self.db.lock();
        let n = match before_ms {
            Some(cutoff) => conn.execute(
                "DELETE FROM push_receipts WHERE ts < ?1",
                params![cutoff],
            )?,
            None => conn.execute("DELETE FROM push_receipts", [])?,
        };
        Ok(u64::try_from(n).unwrap_or(0))
    }
}

fn upsert_turn_index(conn: &Connection, turn: &TurnRecord) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO turn_index
         (id, session_id, turn_idx, turn_class, coding_mode, provider, model,
          final_reward, has_reward,
          reward_thumbs, reward_next_state, reward_tool, reward_verification, reward_cost,
          cost_usd, total_tokens, ts)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
        params![
            turn.id,
            turn.session_id,
            i64::try_from(turn.turn_idx).unwrap_or(i64::MAX),
            turn.turn_class.as_str(),
            turn.coding_mode,
            turn.provider,
            turn.model,
            f64::from(turn.reward.final_score),
            i64::from(turn.reward.loss_mask != 0 || turn.reward.final_score != 0.0),
            turn.reward.thumbs.map(f64::from),
            turn.reward.next_state.map(f64::from),
            turn.reward.tool.map(f64::from),
            turn.reward.verification.map(f64::from),
            turn.reward.cost.map(f64::from),
            turn.cost.usd,
            i64::try_from(turn.cost.total_tokens).unwrap_or(i64::MAX),
            turn.ts.timestamp_millis(),
        ],
    )?;
    Ok(())
}

fn ensure_turn_index_columns(conn: &Connection) -> Result<()> {
    let mut existing: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut stmt = conn.prepare("PRAGMA table_info(turn_index)")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for col in rows {
        existing.insert(col?.to_ascii_lowercase());
    }
    drop(stmt);
    let needed: &[(&str, &str)] = &[
        ("reward_thumbs", "REAL"),
        ("reward_next_state", "REAL"),
        ("reward_tool", "REAL"),
        ("reward_verification", "REAL"),
        ("reward_cost", "REAL"),
    ];
    for (name, kind) in needed {
        if !existing.contains(*name) {
            conn.execute(
                &format!("ALTER TABLE turn_index ADD COLUMN {name} {kind}"),
                [],
            )?;
        }
    }
    Ok(())
}

fn append_line(path: &Path, line: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open {} for append", path.display()))?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

fn file_size_or_zero(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn rewrite_turns_jsonl_before(path: &Path, cutoff_ms: i64, freed: &mut u64) -> Result<u64> {
    if !path.is_file() {
        return Ok(0);
    }
    let original_size = file_size_or_zero(path);
    let contents = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut kept_lines: Vec<String> = Vec::new();
    let mut removed: u64 = 0;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let keep = match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(v) => v
                .get("ts")
                .and_then(|t| t.as_str())
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.timestamp_millis() >= cutoff_ms)
                .unwrap_or(true),
            Err(_) => true,
        };
        if keep {
            kept_lines.push(trimmed.to_string());
        } else {
            removed = removed.saturating_add(1);
        }
    }
    let tmp = path.with_extension("jsonl.tmp");
    {
        let mut file = std::fs::File::create(&tmp)
            .with_context(|| format!("create {}", tmp.display()))?;
        for line in &kept_lines {
            file.write_all(line.as_bytes())?;
            file.write_all(b"\n")?;
        }
        file.sync_all().ok();
    }
    std::fs::rename(&tmp, path).with_context(|| format!("rename {}", tmp.display()))?;
    let new_size = file_size_or_zero(path);
    *freed = freed.saturating_add(original_size.saturating_sub(new_size));
    Ok(removed)
}

fn rewrite_events_jsonl_before(path: &Path, cutoff_ms: i64, freed: &mut u64) -> Result<u64> {
    rewrite_turns_jsonl_before(path, cutoff_ms, freed)
}
