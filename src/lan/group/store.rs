// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};

use super::op::{GroupOp, GroupOpPayload, GroupRole, Hlc, VersionVector};
use super::state::{
    self, DocumentView, GroupMessageView, GroupSummary, MemberView, PhaseView, TaskView,
};

pub struct GroupStore {
    conn: Mutex<Connection>,
    self_user_id: String,
}

#[derive(Debug, Clone)]
pub struct DocRecord {
    pub doc_id: String,
    pub name: String,
    pub is_dir: bool,
    pub size: i64,
    pub phase_id: String,
    pub uploader: String,
    pub content_hash: String,
    pub version: i64,
    pub note: String,
    pub removed: bool,
}

impl GroupStore {
    pub fn open(sen_dir: &Path, self_user_id: &str) -> Result<Self> {
        let dir = sen_dir.join("lan");
        std::fs::create_dir_all(&dir).context("creating lan group store dir")?;
        let db_path = dir.join("lan_groups.db");
        let conn = Connection::open(&db_path).context("opening lan_groups.db")?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous  = NORMAL;
             PRAGMA busy_timeout = 5000;",
        )?;
        Self::init_schema(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            self_user_id: self_user_id.to_string(),
        })
    }

    fn init_schema(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS group_ops (
                 op_id TEXT PRIMARY KEY,
                 group_id TEXT NOT NULL,
                 hlc_ms INTEGER NOT NULL,
                 hlc_counter INTEGER NOT NULL,
                 seq INTEGER NOT NULL DEFAULT 0,
                 author TEXT NOT NULL,
                 payload TEXT NOT NULL,
                 created_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_group_ops_group
                 ON group_ops(group_id, hlc_ms, hlc_counter);
             CREATE INDEX IF NOT EXISTS idx_group_ops_author
                 ON group_ops(group_id, author, seq);

             CREATE TABLE IF NOT EXISTS groups (
                 group_id TEXT PRIMARY KEY,
                 name TEXT NOT NULL DEFAULT '',
                 description TEXT NOT NULL DEFAULT '',
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL,
                 meta_ms INTEGER NOT NULL DEFAULT 0,
                 meta_counter INTEGER NOT NULL DEFAULT 0,
                 meta_author TEXT NOT NULL DEFAULT ''
             );

             CREATE TABLE IF NOT EXISTS group_members (
                 group_id TEXT NOT NULL,
                 user_id TEXT NOT NULL,
                 nickname TEXT NOT NULL DEFAULT '',
                 role TEXT NOT NULL DEFAULT 'member',
                 status TEXT NOT NULL DEFAULT 'active',
                 joined_at INTEGER NOT NULL DEFAULT 0,
                 lww_ms INTEGER NOT NULL DEFAULT 0,
                 lww_counter INTEGER NOT NULL DEFAULT 0,
                 lww_author TEXT NOT NULL DEFAULT '',
                 PRIMARY KEY (group_id, user_id)
             );

             CREATE TABLE IF NOT EXISTS group_phases (
                 group_id TEXT NOT NULL,
                 phase_id TEXT NOT NULL,
                 name TEXT NOT NULL DEFAULT '',
                 ord INTEGER NOT NULL DEFAULT 0,
                 status TEXT NOT NULL DEFAULT 'not_started',
                 color TEXT NOT NULL DEFAULT '',
                 removed INTEGER NOT NULL DEFAULT 0,
                 lww_ms INTEGER NOT NULL DEFAULT 0,
                 lww_counter INTEGER NOT NULL DEFAULT 0,
                 lww_author TEXT NOT NULL DEFAULT '',
                 PRIMARY KEY (group_id, phase_id)
             );

             CREATE TABLE IF NOT EXISTS group_documents (
                 group_id TEXT NOT NULL,
                 doc_id TEXT NOT NULL,
                 name TEXT NOT NULL DEFAULT '',
                 is_dir INTEGER NOT NULL DEFAULT 0,
                 size INTEGER NOT NULL DEFAULT 0,
                 phase_id TEXT NOT NULL DEFAULT '',
                 uploader TEXT NOT NULL DEFAULT '',
                 content_hash TEXT NOT NULL DEFAULT '',
                 version INTEGER NOT NULL DEFAULT 1,
                 note TEXT NOT NULL DEFAULT '',
                 removed INTEGER NOT NULL DEFAULT 0,
                 updated_at INTEGER NOT NULL DEFAULT 0,
                 lww_ms INTEGER NOT NULL DEFAULT 0,
                 lww_counter INTEGER NOT NULL DEFAULT 0,
                 lww_author TEXT NOT NULL DEFAULT '',
                 PRIMARY KEY (group_id, doc_id)
             );

             CREATE TABLE IF NOT EXISTS group_tasks (
                 group_id TEXT NOT NULL,
                 task_id TEXT NOT NULL,
                 title TEXT NOT NULL DEFAULT '',
                 description TEXT NOT NULL DEFAULT '',
                 phase_id TEXT NOT NULL DEFAULT '',
                 assignee TEXT NOT NULL DEFAULT '',
                 status TEXT NOT NULL DEFAULT 'todo',
                 priority TEXT NOT NULL DEFAULT 'medium',
                 due_ms INTEGER NOT NULL DEFAULT 0,
                 deps TEXT NOT NULL DEFAULT '[]',
                 parent TEXT NOT NULL DEFAULT '',
                 kind TEXT NOT NULL DEFAULT 'task',
                 progress INTEGER NOT NULL DEFAULT 0,
                 removed INTEGER NOT NULL DEFAULT 0,
                 created_at INTEGER NOT NULL DEFAULT 0,
                 updated_at INTEGER NOT NULL DEFAULT 0,
                 lww_ms INTEGER NOT NULL DEFAULT 0,
                 lww_counter INTEGER NOT NULL DEFAULT 0,
                 lww_author TEXT NOT NULL DEFAULT '',
                 PRIMARY KEY (group_id, task_id)
             );

             CREATE TABLE IF NOT EXISTS group_messages (
                 group_id TEXT NOT NULL,
                 msg_id TEXT NOT NULL,
                 author TEXT NOT NULL,
                 body TEXT NOT NULL DEFAULT '',
                 kind TEXT NOT NULL DEFAULT 'text',
                 doc_id TEXT NOT NULL DEFAULT '',
                 ts_ms INTEGER NOT NULL DEFAULT 0,
                 read INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY (group_id, msg_id)
             );
             CREATE INDEX IF NOT EXISTS idx_group_messages_group
                 ON group_messages(group_id, ts_ms);",
        )?;
        Ok(())
    }

    pub fn apply_op(&self, op: &GroupOp) -> Result<bool> {
        let mut guard = self.conn.lock();
        let tx = guard.transaction()?;
        let payload_json = serde_json::to_string(&op.payload)?;
        let created_at = super::op::now_ms_u64() as i64;
        tx.execute(
            "INSERT OR IGNORE INTO group_ops
                (op_id, group_id, hlc_ms, hlc_counter, seq, author, payload, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                op.op_id,
                op.group_id,
                op.hlc.millis as i64,
                op.hlc.counter as i64,
                op.seq as i64,
                op.author,
                payload_json,
                created_at,
            ],
        )?;
        if tx.changes() == 0 {
            return Ok(false);
        }
        ensure_group(&tx, &op.group_id, op.hlc.millis as i64)?;
        apply_payload(&tx, op, &self.self_user_id)?;
        bump_group_updated(&tx, &op.group_id, op.hlc.millis as i64)?;
        tx.commit()?;
        Ok(true)
    }

    pub fn self_user_id(&self) -> &str {
        &self.self_user_id
    }

    pub fn group_exists(&self, group_id: &str) -> bool {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT 1 FROM groups WHERE group_id = ?1",
            params![group_id],
            |_| Ok(()),
        )
        .optional()
        .ok()
        .flatten()
        .is_some()
    }

    pub fn self_role(&self, group_id: &str) -> Option<GroupRole> {
        let conn = self.conn.lock();
        let role: Option<String> = conn
            .query_row(
                "SELECT role FROM group_members
                 WHERE group_id = ?1 AND user_id = ?2 AND status = 'active'",
                params![group_id, self.self_user_id],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten();
        role.and_then(|r| GroupRole::parse(&r))
    }

    pub fn role_of(&self, group_id: &str, user_id: &str) -> Option<GroupRole> {
        let conn = self.conn.lock();
        let role: Option<String> = conn
            .query_row(
                "SELECT role FROM group_members
                 WHERE group_id = ?1 AND user_id = ?2 AND status = 'active'",
                params![group_id, user_id],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten();
        role.and_then(|r| GroupRole::parse(&r))
    }

    pub fn active_member_ids(&self, group_id: &str) -> Vec<String> {
        let conn = self.conn.lock();
        let mut stmt = match conn.prepare(
            "SELECT user_id FROM group_members WHERE group_id = ?1 AND status = 'active'",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(params![group_id], |row| row.get::<_, String>(0));
        match rows {
            Ok(rows) => rows.flatten().collect(),
            Err(_) => Vec::new(),
        }
    }

    pub fn groups_for_member(&self, user_id: &str) -> Vec<String> {
        let conn = self.conn.lock();
        let mut stmt = match conn.prepare(
            "SELECT group_id FROM group_members WHERE user_id = ?1 AND status = 'active'",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(params![user_id], |row| row.get::<_, String>(0));
        match rows {
            Ok(rows) => rows.flatten().collect(),
            Err(_) => Vec::new(),
        }
    }

    pub fn known_group_ids(&self) -> Vec<String> {
        let conn = self.conn.lock();
        let mut stmt = match conn.prepare("SELECT group_id FROM groups") {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map([], |row| row.get::<_, String>(0));
        match rows {
            Ok(rows) => rows.flatten().collect(),
            Err(_) => Vec::new(),
        }
    }

    pub fn document(&self, group_id: &str, doc_id: &str) -> Option<DocRecord> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT doc_id, name, is_dir, size, phase_id, uploader, content_hash, version, note, removed
             FROM group_documents WHERE group_id = ?1 AND doc_id = ?2",
            params![group_id, doc_id],
            |row| {
                Ok(DocRecord {
                    doc_id: row.get(0)?,
                    name: row.get(1)?,
                    is_dir: row.get::<_, i64>(2)? != 0,
                    size: row.get(3)?,
                    phase_id: row.get(4)?,
                    uploader: row.get(5)?,
                    content_hash: row.get(6)?,
                    version: row.get(7)?,
                    note: row.get(8)?,
                    removed: row.get::<_, i64>(9)? != 0,
                })
            },
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn next_doc_version(&self, group_id: &str, doc_id: &str) -> i64 {
        self.document(group_id, doc_id)
            .map(|d| d.version + 1)
            .unwrap_or(1)
    }

    pub fn version_vector(&self, group_id: &str) -> VersionVector {
        let mut vv = VersionVector::default();
        let conn = self.conn.lock();
        let mut by_author: std::collections::BTreeMap<String, Vec<u64>> =
            std::collections::BTreeMap::new();
        if let Ok(mut stmt) =
            conn.prepare("SELECT author, seq FROM group_ops WHERE group_id = ?1")
        {
            if let Ok(rows) = stmt.query_map(params![group_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
            }) {
                for (author, seq) in rows.flatten() {
                    by_author.entry(author).or_default().push(seq);
                }
            }
        }
        for (author, mut seqs) in by_author {
            seqs.sort_unstable();
            seqs.dedup();
            let mut prefix = 0u64;
            for seq in seqs {
                if seq == prefix + 1 {
                    prefix = seq;
                } else if seq <= prefix {
                    continue;
                } else {
                    break;
                }
            }
            vv.set(&author, prefix);
        }
        vv
    }

    pub fn max_self_seq(&self, group_id: &str) -> u64 {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM group_ops WHERE group_id = ?1 AND author = ?2",
            params![group_id, self.self_user_id],
            |row| row.get::<_, i64>(0),
        )
        .map(|v| v as u64)
        .unwrap_or(0)
    }

    pub fn ops_for_group(&self, group_id: &str) -> Vec<GroupOp> {
        self.load_ops(group_id, None)
    }

    pub fn ops_since(&self, group_id: &str, have: &VersionVector) -> Vec<GroupOp> {
        self.load_ops(group_id, Some(have))
    }

    fn load_ops(&self, group_id: &str, have: Option<&VersionVector>) -> Vec<GroupOp> {
        let conn = self.conn.lock();
        let mut stmt = match conn.prepare(
            "SELECT op_id, group_id, hlc_ms, hlc_counter, seq, author, payload
             FROM group_ops WHERE group_id = ?1 ORDER BY hlc_ms, hlc_counter, author, op_id",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(params![group_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)? as u64,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        });
        let mut out = Vec::new();
        let Ok(rows) = rows else {
            return out;
        };
        for row in rows.flatten() {
            let hlc = Hlc {
                millis: row.2 as u64,
                counter: row.3 as u32,
            };
            let seq = row.4;
            if let Some(have) = have {
                if have.covers(&row.5, seq) {
                    continue;
                }
            }
            let payload: GroupOpPayload = match serde_json::from_str(&row.6) {
                Ok(p) => p,
                Err(_) => continue,
            };
            out.push(GroupOp {
                op_id: row.0,
                group_id: row.1,
                hlc,
                seq,
                author: row.5,
                payload,
            });
        }
        out
    }

    pub fn list_groups(&self) -> Vec<GroupSummary> {
        let ids = {
            let conn = self.conn.lock();
            let mut stmt = match conn.prepare(
                "SELECT group_id FROM group_members
                 WHERE user_id = ?1 AND status = 'active'",
            ) {
                Ok(stmt) => stmt,
                Err(_) => return Vec::new(),
            };
            let rows = stmt.query_map(params![self.self_user_id], |row| row.get::<_, String>(0));
            match rows {
                Ok(rows) => rows.flatten().collect::<Vec<String>>(),
                Err(_) => return Vec::new(),
            }
        };
        let mut out = Vec::new();
        for id in ids {
            if let Some(summary) = self.group_summary(&id) {
                out.push(summary);
            }
        }
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        out
    }

    pub fn group_summary(&self, group_id: &str) -> Option<GroupSummary> {
        let conn = self.conn.lock();
        let base = conn
            .query_row(
                "SELECT name, description, created_at, updated_at FROM groups WHERE group_id = ?1",
                params![group_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()
            .ok()
            .flatten()?;
        let role: Option<String> = conn
            .query_row(
                "SELECT role FROM group_members
                 WHERE group_id = ?1 AND user_id = ?2 AND status = 'active'",
                params![group_id, self.self_user_id],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten();
        let member_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM group_members WHERE group_id = ?1 AND status = 'active'",
                params![group_id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let doc_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM group_documents WHERE group_id = ?1 AND removed = 0",
                params![group_id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let phase_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM group_phases WHERE group_id = ?1 AND removed = 0",
                params![group_id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let task_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM group_tasks WHERE group_id = ?1 AND removed = 0",
                params![group_id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let done_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM group_tasks
                 WHERE group_id = ?1 AND removed = 0 AND status = 'done'",
                params![group_id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let unread: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM group_messages
                 WHERE group_id = ?1 AND read = 0 AND author <> ?2",
                params![group_id, self.self_user_id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let progress = if task_count > 0 {
            (done_count as f64 / task_count as f64) * 100.0
        } else {
            self.phase_progress_avg(&conn, group_id)
        };
        Some(GroupSummary {
            id: group_id.to_string(),
            name: base.0,
            description: base.1,
            role: role.unwrap_or_else(|| "viewer".to_string()),
            member_count,
            doc_count,
            task_count,
            open_task_count: task_count - done_count,
            phase_count,
            progress: round1(progress),
            unread,
            created_at: base.2,
            updated_at: base.3,
        })
    }

    fn phase_progress_avg(&self, conn: &Connection, group_id: &str) -> f64 {
        let mut stmt = match conn
            .prepare("SELECT status FROM group_phases WHERE group_id = ?1 AND removed = 0")
        {
            Ok(stmt) => stmt,
            Err(_) => return 0.0,
        };
        let rows = match stmt.query_map(params![group_id], |row| row.get::<_, String>(0)) {
            Ok(rows) => rows,
            Err(_) => return 0.0,
        };
        let mut total = 0.0;
        let mut count = 0.0;
        for status in rows.flatten() {
            total += state::phase_status_base_percent(&status);
            count += 1.0;
        }
        if count == 0.0 {
            0.0
        } else {
            total / count
        }
    }

    pub fn members(&self, group_id: &str, online: &HashSet<String>) -> Vec<MemberView> {
        let conn = self.conn.lock();
        let mut stmt = match conn.prepare(
            "SELECT user_id, nickname, role, joined_at FROM group_members
             WHERE group_id = ?1 AND status = 'active'",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(params![group_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        });
        let mut out = Vec::new();
        if let Ok(rows) = rows {
            for row in rows.flatten() {
                let online_now = row.0 == self.self_user_id || online.contains(&row.0);
                out.push(MemberView {
                    user_id: row.0,
                    nickname: row.1,
                    role: row.2,
                    online: online_now,
                    joined_at: row.3,
                });
            }
        }
        out.sort_by(|a, b| {
            let ar = GroupRole::parse(&a.role).map(|r| r.rank()).unwrap_or(0);
            let br = GroupRole::parse(&b.role).map(|r| r.rank()).unwrap_or(0);
            br.cmp(&ar)
                .then_with(|| a.nickname.to_lowercase().cmp(&b.nickname.to_lowercase()))
        });
        out
    }

    pub fn phases(&self, group_id: &str) -> Vec<PhaseView> {
        let conn = self.conn.lock();
        let mut stmt = match conn.prepare(
            "SELECT phase_id, name, ord, status, color FROM group_phases
             WHERE group_id = ?1 AND removed = 0 ORDER BY ord, name",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(params![group_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        });
        let mut out = Vec::new();
        let Ok(rows) = rows else {
            return out;
        };
        for row in rows.flatten() {
            let doc_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM group_documents
                     WHERE group_id = ?1 AND phase_id = ?2 AND removed = 0",
                    params![group_id, row.0],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            let task_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM group_tasks
                     WHERE group_id = ?1 AND phase_id = ?2 AND removed = 0",
                    params![group_id, row.0],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            let percent = phase_percent(&conn, group_id, &row.0, &row.3, task_count);
            out.push(PhaseView {
                id: row.0,
                name: row.1,
                order: row.2,
                status: row.3,
                color: row.4,
                percent: round1(percent),
                doc_count,
                task_count,
            });
        }
        out
    }

    pub fn documents(&self, group_id: &str) -> Vec<DocumentView> {
        let conn = self.conn.lock();
        let mut stmt = match conn.prepare(
            "SELECT d.doc_id, d.name, d.is_dir, d.size, d.phase_id, d.uploader,
                    COALESCE(m.nickname, d.uploader) AS uploader_nick,
                    d.content_hash, d.version, d.note, d.updated_at
             FROM group_documents d
             LEFT JOIN group_members m ON m.group_id = d.group_id AND m.user_id = d.uploader
             WHERE d.group_id = ?1 AND d.removed = 0
             ORDER BY d.updated_at DESC",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(params![group_id], |row| {
            Ok(DocumentView {
                id: row.get(0)?,
                name: row.get(1)?,
                is_dir: row.get::<_, i64>(2)? != 0,
                size: row.get(3)?,
                phase_id: row.get(4)?,
                uploader: row.get(5)?,
                uploader_nickname: row.get(6)?,
                content_hash: row.get(7)?,
                version: row.get(8)?,
                note: row.get(9)?,
                available: false,
                updated_at: row.get(10)?,
            })
        });
        match rows {
            Ok(rows) => rows.flatten().collect(),
            Err(_) => Vec::new(),
        }
    }

    pub fn tasks(&self, group_id: &str) -> Vec<TaskView> {
        let conn = self.conn.lock();
        let mut stmt = match conn.prepare(
            "SELECT t.task_id, t.title, t.description, t.phase_id, t.assignee,
                    COALESCE(m.nickname, t.assignee) AS assignee_nick,
                    t.status, t.priority, t.due_ms, t.deps, t.parent, t.kind, t.progress,
                    t.created_at, t.updated_at
             FROM group_tasks t
             LEFT JOIN group_members m ON m.group_id = t.group_id AND m.user_id = t.assignee
             WHERE t.group_id = ?1 AND t.removed = 0
             ORDER BY t.created_at",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(params![group_id], |row| {
            let deps_json: String = row.get(9)?;
            let deps: Vec<String> = serde_json::from_str(&deps_json).unwrap_or_default();
            Ok(TaskView {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                phase_id: row.get(3)?,
                assignee: row.get(4)?,
                assignee_nickname: row.get(5)?,
                status: row.get(6)?,
                priority: row.get(7)?,
                due_ms: row.get(8)?,
                deps,
                parent: row.get(10)?,
                kind: row.get(11)?,
                progress: row.get(12)?,
                created_at: row.get(13)?,
                updated_at: row.get(14)?,
            })
        });
        match rows {
            Ok(rows) => rows.flatten().collect(),
            Err(_) => Vec::new(),
        }
    }

    pub fn messages(&self, group_id: &str, limit: i64) -> Vec<GroupMessageView> {
        let conn = self.conn.lock();
        let mut stmt = match conn.prepare(
            "SELECT g.msg_id, g.author, COALESCE(m.nickname, g.author) AS author_nick,
                    g.body, g.kind, g.doc_id, g.ts_ms
             FROM group_messages g
             LEFT JOIN group_members m ON m.group_id = g.group_id AND m.user_id = g.author
             WHERE g.group_id = ?1 ORDER BY g.ts_ms DESC, g.msg_id DESC LIMIT ?2",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(params![group_id, limit], |row| {
            Ok(GroupMessageView {
                id: row.get(0)?,
                author: row.get(1)?,
                author_nickname: row.get(2)?,
                body: row.get(3)?,
                kind: row.get(4)?,
                doc_id: row.get(5)?,
                ts_ms: row.get(6)?,
            })
        });
        let mut out: Vec<GroupMessageView> = match rows {
            Ok(rows) => rows.flatten().collect(),
            Err(_) => Vec::new(),
        };
        out.reverse();
        out
    }

    pub fn mark_read(&self, group_id: &str) {
        let conn = self.conn.lock();
        let _ = conn.execute(
            "UPDATE group_messages SET read = 1
             WHERE group_id = ?1 AND read = 0 AND author <> ?2",
            params![group_id, self.self_user_id],
        );
    }

    pub fn unread_total(&self) -> i64 {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT COUNT(*) FROM group_messages g
             JOIN group_members m ON m.group_id = g.group_id AND m.user_id = ?1 AND m.status = 'active'
             WHERE g.read = 0 AND g.author <> ?1",
            params![self.self_user_id],
            |row| row.get(0),
        )
        .unwrap_or(0)
    }
}

fn ensure_group(conn: &Connection, group_id: &str, ts: i64) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO groups (group_id, created_at, updated_at) VALUES (?1, ?2, ?2)",
        params![group_id, ts],
    )?;
    Ok(())
}

fn bump_group_updated(conn: &Connection, group_id: &str, ts: i64) -> Result<()> {
    conn.execute(
        "UPDATE groups SET updated_at = MAX(updated_at, ?2) WHERE group_id = ?1",
        params![group_id, ts],
    )?;
    Ok(())
}

fn guard_wins(
    conn: &Connection,
    table: &str,
    group_id: &str,
    key_col: &str,
    key: &str,
    inc: (i64, i64, &str),
) -> bool {
    let sql = format!(
        "SELECT lww_ms, lww_counter, lww_author FROM {table}
         WHERE group_id = ?1 AND {key_col} = ?2"
    );
    let current: Option<(i64, i64, String)> = conn
        .query_row(&sql, params![group_id, key], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get::<_, String>(2)?))
        })
        .optional()
        .ok()
        .flatten();
    match current {
        None => true,
        Some((ms, counter, author)) => (inc.0, inc.1, inc.2) > (ms, counter, author.as_str()),
    }
}

fn active_member_count(conn: &Connection, group_id: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM group_members WHERE group_id = ?1 AND status = 'active'",
        params![group_id],
        |row| row.get(0),
    )
    .optional()
    .ok()
    .flatten()
    .unwrap_or(0)
}

fn member_role_in_tx(conn: &Connection, group_id: &str, user_id: &str) -> Option<String> {
    conn.query_row(
        "SELECT role FROM group_members WHERE group_id = ?1 AND user_id = ?2 AND status = 'active'",
        params![group_id, user_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
}

// A member-management op is authorized when the group is being created (no active
// members yet = genesis/creator), when the author is an active owner, or when it
// is a self-update that does not escalate the caller's own role. This blocks a LAN
// peer from self-escalating to owner or removing/altering other members.
fn membership_upsert_authorized(
    conn: &Connection,
    group_id: &str,
    author: &str,
    target_user: &str,
    requested_role: &str,
) -> bool {
    if active_member_count(conn, group_id) == 0 {
        return true;
    }
    if member_role_in_tx(conn, group_id, author).as_deref() == Some("owner") {
        return true;
    }
    if author == target_user {
        let current = member_role_in_tx(conn, group_id, author);
        // Allow self nickname/no-op updates, but never a self-escalation to owner
        // or to a role the caller does not already hold.
        return requested_role != "owner" && current.as_deref() == Some(requested_role);
    }
    false
}

fn membership_remove_authorized(
    conn: &Connection,
    group_id: &str,
    author: &str,
    target_user: &str,
) -> bool {
    if member_role_in_tx(conn, group_id, author).as_deref() == Some("owner") {
        return true;
    }
    author == target_user
}

fn apply_payload(conn: &Connection, op: &GroupOp, self_user_id: &str) -> Result<()> {
    let inc = (op.hlc.millis as i64, op.hlc.counter as i64, op.author.as_str());
    match &op.payload {
        GroupOpPayload::GroupMeta { name, description } => {
            let current: Option<(i64, i64, String)> = conn
                .query_row(
                    "SELECT meta_ms, meta_counter, meta_author FROM groups WHERE group_id = ?1",
                    params![op.group_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get::<_, String>(2)?)),
                )
                .optional()
                .ok()
                .flatten();
            let wins = match current {
                None => true,
                Some((ms, c, a)) => (inc.0, inc.1, inc.2) > (ms, c, a.as_str()),
            };
            if wins {
                conn.execute(
                    "UPDATE groups SET name = ?2, description = ?3,
                        meta_ms = ?4, meta_counter = ?5, meta_author = ?6
                     WHERE group_id = ?1",
                    params![op.group_id, name, description, inc.0, inc.1, op.author],
                )?;
            }
        }
        GroupOpPayload::MemberUpsert {
            user_id,
            nickname,
            role,
        } => {
            if !membership_upsert_authorized(conn, &op.group_id, &op.author, user_id, role.as_str())
            {
                tracing::warn!(
                    target: "lan.group",
                    group = %op.group_id,
                    author = %op.author,
                    target = %user_id,
                    role = %role.as_str(),
                    "rejecting unauthorized MemberUpsert (author is not an owner / self-escalation)"
                );
            } else if guard_wins(conn, "group_members", &op.group_id, "user_id", user_id, inc) {
                conn.execute(
                    "INSERT INTO group_members
                        (group_id, user_id, nickname, role, status, joined_at,
                         lww_ms, lww_counter, lww_author)
                     VALUES (?1, ?2, ?3, ?4, 'active', ?5, ?6, ?7, ?8)
                     ON CONFLICT(group_id, user_id) DO UPDATE SET
                        nickname = excluded.nickname,
                        role = excluded.role,
                        status = 'active',
                        lww_ms = excluded.lww_ms,
                        lww_counter = excluded.lww_counter,
                        lww_author = excluded.lww_author",
                    params![
                        op.group_id,
                        user_id,
                        nickname,
                        role.as_str(),
                        op.hlc.millis as i64,
                        inc.0,
                        inc.1,
                        op.author,
                    ],
                )?;
            }
        }
        GroupOpPayload::MemberRemove { user_id } => {
            if !membership_remove_authorized(conn, &op.group_id, &op.author, user_id) {
                tracing::warn!(
                    target: "lan.group",
                    group = %op.group_id,
                    author = %op.author,
                    target = %user_id,
                    "rejecting unauthorized MemberRemove (author is not an owner and not self)"
                );
            } else if guard_wins(conn, "group_members", &op.group_id, "user_id", user_id, inc) {
                conn.execute(
                    "INSERT INTO group_members
                        (group_id, user_id, status, lww_ms, lww_counter, lww_author)
                     VALUES (?1, ?2, 'removed', ?3, ?4, ?5)
                     ON CONFLICT(group_id, user_id) DO UPDATE SET
                        status = 'removed',
                        lww_ms = excluded.lww_ms,
                        lww_counter = excluded.lww_counter,
                        lww_author = excluded.lww_author",
                    params![op.group_id, user_id, inc.0, inc.1, op.author],
                )?;
            }
        }
        GroupOpPayload::PhaseUpsert {
            phase_id,
            name,
            order,
            status,
            color,
        } => {
            if guard_wins(conn, "group_phases", &op.group_id, "phase_id", phase_id, inc) {
                let status = state::normalize_phase_status(status);
                conn.execute(
                    "INSERT INTO group_phases
                        (group_id, phase_id, name, ord, status, color, removed,
                         lww_ms, lww_counter, lww_author)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8, ?9)
                     ON CONFLICT(group_id, phase_id) DO UPDATE SET
                        name = excluded.name,
                        ord = excluded.ord,
                        status = excluded.status,
                        color = excluded.color,
                        removed = 0,
                        lww_ms = excluded.lww_ms,
                        lww_counter = excluded.lww_counter,
                        lww_author = excluded.lww_author",
                    params![
                        op.group_id, phase_id, name, order, status, color, inc.0, inc.1, op.author,
                    ],
                )?;
            }
        }
        GroupOpPayload::PhaseRemove { phase_id } => {
            if guard_wins(conn, "group_phases", &op.group_id, "phase_id", phase_id, inc) {
                conn.execute(
                    "INSERT INTO group_phases
                        (group_id, phase_id, removed, lww_ms, lww_counter, lww_author)
                     VALUES (?1, ?2, 1, ?3, ?4, ?5)
                     ON CONFLICT(group_id, phase_id) DO UPDATE SET
                        removed = 1,
                        lww_ms = excluded.lww_ms,
                        lww_counter = excluded.lww_counter,
                        lww_author = excluded.lww_author",
                    params![op.group_id, phase_id, inc.0, inc.1, op.author],
                )?;
            }
        }
        GroupOpPayload::DocUpsert {
            doc_id,
            name,
            is_dir,
            size,
            phase_id,
            uploader,
            content_hash,
            version,
            note,
        } => {
            if guard_wins(conn, "group_documents", &op.group_id, "doc_id", doc_id, inc) {
                conn.execute(
                    "INSERT INTO group_documents
                        (group_id, doc_id, name, is_dir, size, phase_id, uploader,
                         content_hash, version, note, removed, updated_at,
                         lww_ms, lww_counter, lww_author)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, ?11, ?12, ?13, ?14)
                     ON CONFLICT(group_id, doc_id) DO UPDATE SET
                        name = excluded.name,
                        is_dir = excluded.is_dir,
                        size = excluded.size,
                        phase_id = excluded.phase_id,
                        uploader = excluded.uploader,
                        content_hash = excluded.content_hash,
                        version = excluded.version,
                        note = excluded.note,
                        removed = 0,
                        updated_at = excluded.updated_at,
                        lww_ms = excluded.lww_ms,
                        lww_counter = excluded.lww_counter,
                        lww_author = excluded.lww_author",
                    params![
                        op.group_id,
                        doc_id,
                        name,
                        i64::from(*is_dir),
                        size,
                        phase_id,
                        uploader,
                        content_hash,
                        version,
                        note,
                        op.hlc.millis as i64,
                        inc.0,
                        inc.1,
                        op.author,
                    ],
                )?;
            }
        }
        GroupOpPayload::DocRemove { doc_id } => {
            if guard_wins(conn, "group_documents", &op.group_id, "doc_id", doc_id, inc) {
                conn.execute(
                    "INSERT INTO group_documents
                        (group_id, doc_id, removed, updated_at, lww_ms, lww_counter, lww_author)
                     VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6)
                     ON CONFLICT(group_id, doc_id) DO UPDATE SET
                        removed = 1,
                        updated_at = excluded.updated_at,
                        lww_ms = excluded.lww_ms,
                        lww_counter = excluded.lww_counter,
                        lww_author = excluded.lww_author",
                    params![op.group_id, doc_id, op.hlc.millis as i64, inc.0, inc.1, op.author],
                )?;
            }
        }
        GroupOpPayload::TaskUpsert {
            task_id,
            title,
            description,
            phase_id,
            assignee,
            status,
            priority,
            due_ms,
            deps,
            parent,
            kind,
            progress,
        } => {
            if guard_wins(conn, "group_tasks", &op.group_id, "task_id", task_id, inc) {
                let deps_json = serde_json::to_string(deps).unwrap_or_else(|_| "[]".to_string());
                let status = state::normalize_task_status(status);
                let priority = state::normalize_task_priority(priority);
                let kind = state::normalize_task_kind(kind);
                conn.execute(
                    "INSERT INTO group_tasks
                        (group_id, task_id, title, description, phase_id, assignee, status,
                         priority, due_ms, deps, parent, kind, progress, removed,
                         created_at, updated_at, lww_ms, lww_counter, lww_author)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 0,
                             ?14, ?14, ?15, ?16, ?17)
                     ON CONFLICT(group_id, task_id) DO UPDATE SET
                        title = excluded.title,
                        description = excluded.description,
                        phase_id = excluded.phase_id,
                        assignee = excluded.assignee,
                        status = excluded.status,
                        priority = excluded.priority,
                        due_ms = excluded.due_ms,
                        deps = excluded.deps,
                        parent = excluded.parent,
                        kind = excluded.kind,
                        progress = excluded.progress,
                        removed = 0,
                        updated_at = excluded.updated_at,
                        lww_ms = excluded.lww_ms,
                        lww_counter = excluded.lww_counter,
                        lww_author = excluded.lww_author",
                    params![
                        op.group_id,
                        task_id,
                        title,
                        description,
                        phase_id,
                        assignee,
                        status,
                        priority,
                        due_ms,
                        deps_json,
                        parent,
                        kind,
                        (*progress).clamp(0, 100),
                        op.hlc.millis as i64,
                        inc.0,
                        inc.1,
                        op.author,
                    ],
                )?;
            }
        }
        GroupOpPayload::TaskRemove { task_id } => {
            if guard_wins(conn, "group_tasks", &op.group_id, "task_id", task_id, inc) {
                conn.execute(
                    "INSERT INTO group_tasks
                        (group_id, task_id, removed, updated_at, lww_ms, lww_counter, lww_author)
                     VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6)
                     ON CONFLICT(group_id, task_id) DO UPDATE SET
                        removed = 1,
                        updated_at = excluded.updated_at,
                        lww_ms = excluded.lww_ms,
                        lww_counter = excluded.lww_counter,
                        lww_author = excluded.lww_author",
                    params![op.group_id, task_id, op.hlc.millis as i64, inc.0, inc.1, op.author],
                )?;
            }
        }
        GroupOpPayload::ChatPost {
            msg_id,
            body,
            kind,
            doc_id,
            ts_ms,
        } => {
            let read = i64::from(op.author == self_user_id);
            let kind = state::normalize_chat_kind(kind);
            conn.execute(
                "INSERT OR IGNORE INTO group_messages
                    (group_id, msg_id, author, body, kind, doc_id, ts_ms, read)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![op.group_id, msg_id, op.author, body, kind, doc_id, ts_ms, read],
            )?;
        }
    }
    Ok(())
}

fn phase_percent(
    conn: &Connection,
    group_id: &str,
    phase_id: &str,
    status: &str,
    task_count: i64,
) -> f64 {
    if task_count <= 0 {
        return state::phase_status_base_percent(status);
    }
    let mut stmt = match conn.prepare(
        "SELECT status, progress FROM group_tasks
         WHERE group_id = ?1 AND phase_id = ?2 AND removed = 0",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return state::phase_status_base_percent(status),
    };
    let rows = match stmt.query_map(params![group_id, phase_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    }) {
        Ok(rows) => rows,
        Err(_) => return state::phase_status_base_percent(status),
    };
    let mut total = 0.0;
    let mut count = 0.0;
    for row in rows.flatten() {
        total += state::task_progress_value(&row.0, row.1);
        count += 1.0;
    }
    if count == 0.0 {
        state::phase_status_base_percent(status)
    } else {
        total / count
    }
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}
