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
    PersistenceStatus, PurgeReport, PurgeScope, PushReceipt, ThumbVote, TurnRecord,
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
    negative_hits INTEGER NOT NULL DEFAULT 0,
    enabled       INTEGER NOT NULL DEFAULT 1,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_lessons_mode ON lessons(coding_mode);
CREATE INDEX IF NOT EXISTS idx_lessons_enabled ON lessons(enabled);

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
    sha256            TEXT NOT NULL DEFAULT '',
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
    injected_lesson_ids TEXT NOT NULL DEFAULT '[]',
    cost_usd            REAL NOT NULL DEFAULT 0,
    total_tokens        INTEGER NOT NULL DEFAULT 0,
    ts                  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_turn_index_session ON turn_index(session_id);
CREATE INDEX IF NOT EXISTS idx_turn_index_ts ON turn_index(ts);
CREATE INDEX IF NOT EXISTS idx_turn_index_mode ON turn_index(coding_mode);

CREATE TABLE IF NOT EXISTS meta_counters (
    key   TEXT PRIMARY KEY,
    value INTEGER NOT NULL DEFAULT 0
);
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
        ensure_lessons_columns(&conn).context("ensure lessons feedback columns")?;
        ensure_exports_columns(&conn).context("ensure exports digest column")?;
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

    pub fn session_turn_count(&self, session_id: &str) -> u64 {
        let conn = self.db.lock();
        conn.query_row(
            "SELECT COUNT(*) FROM turn_index WHERE session_id = ?1",
            params![session_id],
            |row| row.get::<_, i64>(0),
        )
        .map(|v| u64::try_from(v).unwrap_or(0))
        .unwrap_or(0)
    }

    pub fn has_distill_audit(&self, turn_id: &str) -> bool {
        let conn = self.db.lock();
        conn.query_row(
            "SELECT 1 FROM audit_events WHERE kind = 'distill' AND turn_id = ?1 LIMIT 1",
            params![turn_id],
            |_| Ok(()),
        )
        .is_ok()
    }

    pub fn latest_turn_id_for_session(&self, session_id: &str) -> Result<Option<String>> {
        let conn = self.db.lock();
        let id = conn
            .query_row(
                "SELECT id FROM turn_index WHERE session_id = ?1
                 ORDER BY ts DESC, turn_idx DESC LIMIT 1",
                params![session_id],
                |row| row.get::<_, String>(0),
            )
            .ok();
        Ok(id)
    }

    pub fn top_session_turn_ids_by_reward(
        &self,
        session_id: &str,
        min_reward: f64,
        limit: usize,
    ) -> Result<Vec<String>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(
            "SELECT id FROM turn_index
             WHERE session_id = ?1 AND final_reward >= ?2
             ORDER BY final_reward DESC, ts DESC
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(
            params![session_id, min_reward, i64::try_from(limit).unwrap_or(i64::MAX)],
            |row| row.get::<_, String>(0),
        )?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn all_final_rewards(&self) -> Result<std::collections::HashMap<String, f32>> {
        let conn = self.db.lock();
        let mut stmt =
            conn.prepare("SELECT id, final_reward FROM turn_index WHERE has_reward = 1")?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let reward: f64 = row.get(1)?;
            Ok((id, reward))
        })?;
        let mut out = std::collections::HashMap::new();
        for row in rows {
            let (id, reward) = row?;
            #[allow(clippy::cast_possible_truncation)]
            out.insert(id, reward as f32);
        }
        Ok(out)
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
        let mut found: Option<TurnRecord> = None;
        for_each_line_reverse(&self.turns_jsonl, |line| {
            if let Ok(candidate) = serde_json::from_str::<TurnRecord>(line) {
                if candidate.id == turn_id {
                    found = Some(candidate);
                    return false;
                }
            }
            true
        })?;
        Ok(found)
    }

    pub fn find_turns_for_session(&self, session_id: &str, limit: usize) -> Result<Vec<TurnRecord>> {
        if !self.turns_jsonl.is_file() || limit == 0 {
            return Ok(Vec::new());
        }
        let mut matched: Vec<TurnRecord> = Vec::new();
        for_each_line_reverse(&self.turns_jsonl, |line| {
            if let Ok(candidate) = serde_json::from_str::<TurnRecord>(line) {
                if candidate.session_id == session_id {
                    matched.push(candidate);
                }
            }
            matched.len() < limit
        })?;
        matched.sort_by(|a, b| b.ts.cmp(&a.ts));
        Ok(matched)
    }

    pub fn find_recent_turns(&self, limit: usize) -> Result<Vec<TurnRecord>> {
        if !self.turns_jsonl.is_file() || limit == 0 {
            return Ok(Vec::new());
        }
        let mut all: Vec<TurnRecord> = Vec::new();
        for_each_line_reverse(&self.turns_jsonl, |line| {
            if let Ok(candidate) = serde_json::from_str::<TurnRecord>(line) {
                all.push(candidate);
            }
            all.len() < limit
        })?;
        all.sort_by(|a, b| b.ts.cmp(&a.ts));
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

    pub fn lesson_duplicate_exists(&self, title: &str, body: &str) -> Result<bool> {
        let lessons = self.list_lessons(false)?;
        let norm_title = normalize_lesson_text(title);
        if norm_title.is_empty() {
            return Ok(false);
        }
        let title_tokens = lesson_token_set(&norm_title);
        let norm_body_prefix: String = normalize_lesson_text(body).chars().take(120).collect();
        for lesson in &lessons {
            let existing_title = normalize_lesson_text(&lesson.title);
            if existing_title == norm_title {
                return Ok(true);
            }
            let existing_tokens = lesson_token_set(&existing_title);
            if lesson_jaccard(&title_tokens, &existing_tokens) >= 0.8 {
                return Ok(true);
            }
            if !norm_body_prefix.is_empty() {
                let existing_body: String =
                    normalize_lesson_text(&lesson.body).chars().take(120).collect();
                if existing_body == norm_body_prefix {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    pub fn lessons_created_since(&self, ts_ms: i64) -> Result<u64> {
        let conn = self.db.lock();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM lessons WHERE created_at >= ?1",
                params![ts_ms],
                |row| row.get(0),
            )
            .unwrap_or(0);
        Ok(u64::try_from(count).unwrap_or(0))
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
        drop(conn);
        super::injector::invalidate_lesson_cache();
        Ok(())
    }

    pub fn delete_lesson(&self, id: &str) -> Result<bool> {
        let conn = self.db.lock();
        let n = conn.execute("DELETE FROM lessons WHERE id = ?1", params![id])?;
        drop(conn);
        super::injector::invalidate_lesson_cache();
        Ok(n > 0)
    }

    pub fn list_lessons(&self, only_enabled: bool) -> Result<Vec<Lesson>> {
        let conn = self.db.lock();
        let mut stmt = if only_enabled {
            conn.prepare(
                "SELECT id, title, body, tags, coding_mode, source_turn_ids, hits, enabled, created_at, updated_at, negative_hits
                 FROM lessons WHERE enabled = 1 ORDER BY updated_at DESC",
            )?
        } else {
            conn.prepare(
                "SELECT id, title, body, tags, coding_mode, source_turn_ids, hits, enabled, created_at, updated_at, negative_hits
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
                    negative_hits: u64::try_from(row.get::<_, i64>(10)?).unwrap_or(0),
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

    pub fn injected_lessons_for_turn(&self, turn_id: &str) -> Result<Vec<String>> {
        let conn = self.db.lock();
        let raw: Option<String> = conn
            .query_row(
                "SELECT injected_lesson_ids FROM turn_index WHERE id = ?1",
                params![turn_id],
                |row| row.get(0),
            )
            .ok();
        Ok(raw
            .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
            .unwrap_or_default())
    }

    pub fn record_lesson_negative_feedback(&self, ids: &[String]) -> Result<Vec<String>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        const MIN_HITS_FOR_AUTO_DISABLE: i64 = 5;
        let conn = self.db.lock();
        let mut disabled: Vec<String> = Vec::new();
        {
            let mut bump =
                conn.prepare("UPDATE lessons SET negative_hits = negative_hits + 1 WHERE id = ?1")?;
            let mut disable = conn.prepare(
                "UPDATE lessons SET enabled = 0, updated_at = ?2
                 WHERE id = ?1 AND enabled = 1
                   AND hits >= ?3 AND negative_hits * 2 >= hits",
            )?;
            let now_ms = Utc::now().timestamp_millis();
            for id in ids {
                bump.execute(params![id])?;
                let changed = disable.execute(params![id, now_ms, MIN_HITS_FOR_AUTO_DISABLE])?;
                if changed > 0 {
                    disabled.push(id.clone());
                }
            }
        }
        Ok(disabled)
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
             (id, format, path, sample_count, size_bytes, sha256, time_window_start, time_window_end, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                record.id,
                record.format.as_str(),
                record.path,
                i64::try_from(record.sample_count).unwrap_or(i64::MAX),
                i64::try_from(record.size_bytes).unwrap_or(i64::MAX),
                record.sha256,
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
            "SELECT id, format, path, sample_count, size_bytes, sha256, time_window_start, time_window_end, created_at
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
                    sha256: row.get(5)?,
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
            "SELECT id, format, path, sample_count, size_bytes, sha256, time_window_start, time_window_end, created_at
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
                    sha256: row.get(5)?,
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

    pub fn bump_counter(&self, key: &str) {
        let conn = self.db.lock();
        let _ = conn.execute(
            "INSERT INTO meta_counters (key, value) VALUES (?1, 1)
             ON CONFLICT(key) DO UPDATE SET value = value + 1",
            params![key],
        );
    }

    pub fn counter(&self, key: &str) -> u64 {
        let conn = self.db.lock();
        conn.query_row(
            "SELECT value FROM meta_counters WHERE key = ?1",
            params![key],
            |row| row.get::<_, i64>(0),
        )
        .map(|v| u64::try_from(v).unwrap_or(0))
        .unwrap_or(0)
    }

    pub fn count_turns_jsonl_lines(&self) -> u64 {
        if !self.turns_jsonl.is_file() {
            return 0;
        }
        let Ok(file) = std::fs::File::open(&self.turns_jsonl) else {
            return 0;
        };
        use std::io::BufRead as _;
        let reader = std::io::BufReader::new(file);
        reader
            .lines()
            .filter(|l| l.as_ref().map(|s| !s.trim().is_empty()).unwrap_or(false))
            .count() as u64
    }

    pub fn persistence_status(&self) -> Result<PersistenceStatus> {
        let turns_file_size = file_size_or_zero(&self.turns_jsonl);
        let events_file_size = file_size_or_zero(&self.events_jsonl);
        let db_file_size = file_size_or_zero(&self.base_dir.join("evolution.db"));
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
            db_file_size,
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
    let injected =
        serde_json::to_string(&turn.injected_lesson_ids).unwrap_or_else(|_| "[]".into());
    conn.execute(
        "INSERT OR REPLACE INTO turn_index
         (id, session_id, turn_idx, turn_class, coding_mode, provider, model,
          final_reward, has_reward,
          reward_thumbs, reward_next_state, reward_tool, reward_verification, reward_cost,
          cost_usd, total_tokens, ts, injected_lesson_ids)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
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
            injected,
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
        ("injected_lesson_ids", "TEXT NOT NULL DEFAULT '[]'"),
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

fn ensure_exports_columns(conn: &Connection) -> Result<()> {
    let mut existing: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut stmt = conn.prepare("PRAGMA table_info(exports)")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for col in rows {
        existing.insert(col?.to_ascii_lowercase());
    }
    drop(stmt);
    if existing.contains("md5") && !existing.contains("sha256") {
        conn.execute("ALTER TABLE exports RENAME COLUMN md5 TO sha256", [])?;
    }
    Ok(())
}

fn ensure_lessons_columns(conn: &Connection) -> Result<()> {
    let mut existing: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut stmt = conn.prepare("PRAGMA table_info(lessons)")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for col in rows {
        existing.insert(col?.to_ascii_lowercase());
    }
    drop(stmt);
    if !existing.contains("negative_hits") {
        conn.execute(
            "ALTER TABLE lessons ADD COLUMN negative_hits INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    Ok(())
}

pub const DAILY_LESSON_INTAKE_CAP: u64 = 12;

fn for_each_line_reverse<F>(path: &Path, mut on_line: F) -> Result<()>
where
    F: FnMut(&str) -> bool,
{
    use std::io::{Read as _, Seek as _, SeekFrom};
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Ok(()),
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if len == 0 {
        return Ok(());
    }
    const CHUNK: u64 = 256 * 1024;
    let mut end = len;
    let mut pending_head: Vec<u8> = Vec::new();
    while end > 0 {
        let start = end.saturating_sub(CHUNK);
        file.seek(SeekFrom::Start(start))
            .with_context(|| format!("seek {}", path.display()))?;
        let mut buf = vec![0u8; usize::try_from(end - start).unwrap_or(0)];
        file.read_exact(&mut buf)
            .with_context(|| format!("read {}", path.display()))?;
        buf.extend_from_slice(&pending_head);
        pending_head.clear();
        let mut slice_end = buf.len();
        while slice_end > 0 {
            match buf[..slice_end].iter().rposition(|&b| b == b'\n') {
                Some(nl) => {
                    let line = &buf[nl + 1..slice_end];
                    if let Ok(text) = std::str::from_utf8(line) {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() && !on_line(trimmed) {
                            return Ok(());
                        }
                    }
                    slice_end = nl;
                }
                None => break,
            }
        }
        if start == 0 {
            if let Ok(text) = std::str::from_utf8(&buf[..slice_end]) {
                let trimmed = text.trim();
                if !trimmed.is_empty() && !on_line(trimmed) {
                    return Ok(());
                }
            }
        } else {
            pending_head = buf[..slice_end].to_vec();
        }
        end = start;
    }
    Ok(())
}

fn normalize_lesson_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_space = true;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    out.trim_end().to_string()
}

fn lesson_token_set(normalized: &str) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    for word in normalized.split_whitespace() {
        super::text_match::push_segment_tokens(word, 1, &[], &mut out);
    }
    out
}

fn lesson_jaccard(
    a: &std::collections::HashSet<String>,
    b: &std::collections::HashSet<String>,
) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 {
        0.0
    } else {
        intersection as f32 / union as f32
    }
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
