// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Context;
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    Pattern,
    Decision,
    Lesson,
    Expert,
    Technology,
}

impl NodeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pattern => "pattern",
            Self::Decision => "decision",
            Self::Lesson => "lesson",
            Self::Expert => "expert",
            Self::Technology => "technology",
        }
    }

    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s {
            "pattern" => Ok(Self::Pattern),
            "decision" => Ok(Self::Decision),
            "lesson" => Ok(Self::Lesson),
            "expert" => Ok(Self::Expert),
            "technology" => Ok(Self::Technology),
            other => anyhow::bail!("unknown node type: {other}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    Uses,
    Replaces,
    Extends,
    AuthoredBy,
    AppliesTo,
}

impl Relation {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Uses => "uses",
            Self::Replaces => "replaces",
            Self::Extends => "extends",
            Self::AuthoredBy => "authored_by",
            Self::AppliesTo => "applies_to",
        }
    }

    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s {
            "uses" => Ok(Self::Uses),
            "replaces" => Ok(Self::Replaces),
            "extends" => Ok(Self::Extends),
            "authored_by" => Ok(Self::AuthoredBy),
            "applies_to" => Ok(Self::AppliesTo),
            other => anyhow::bail!("unknown relation: {other}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeNode {
    pub id: String,
    pub node_type: NodeType,
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub source_project: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEdge {
    pub from_id: String,
    pub to_id: String,
    pub relation: Relation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub node: KnowledgeNode,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStats {
    pub total_nodes: usize,
    pub total_edges: usize,
    pub nodes_by_type: HashMap<String, usize>,
    pub top_tags: Vec<(String, usize)>,
}

pub struct KnowledgeGraph {
    conn: Mutex<Connection>,
    #[allow(dead_code)]
    db_path: PathBuf,
    max_nodes: usize,
}

impl KnowledgeGraph {

    pub fn new(db_path: &Path, max_nodes: usize) -> anyhow::Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(db_path).context("failed to open knowledge graph database")?;

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous  = NORMAL;
             PRAGMA foreign_keys = ON;",
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS nodes (
                id TEXT PRIMARY KEY,
                node_type TEXT NOT NULL,
                title TEXT NOT NULL,
                content TEXT NOT NULL,
                tags TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                source_project TEXT
            );

            CREATE TABLE IF NOT EXISTS edges (
                from_id TEXT NOT NULL,
                to_id TEXT NOT NULL,
                relation TEXT NOT NULL,
                PRIMARY KEY (from_id, to_id, relation),
                FOREIGN KEY (from_id) REFERENCES nodes(id) ON DELETE CASCADE,
                FOREIGN KEY (to_id) REFERENCES nodes(id) ON DELETE CASCADE
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS nodes_fts USING fts5(
                title, content, tags, content='nodes', content_rowid='rowid'
            );

            CREATE TRIGGER IF NOT EXISTS nodes_ai AFTER INSERT ON nodes BEGIN
                INSERT INTO nodes_fts(rowid, title, content, tags)
                VALUES (new.rowid, new.title, new.content, new.tags);
            END;

            CREATE TRIGGER IF NOT EXISTS nodes_ad AFTER DELETE ON nodes BEGIN
                INSERT INTO nodes_fts(nodes_fts, rowid, title, content, tags)
                VALUES ('delete', old.rowid, old.title, old.content, old.tags);
            END;

            CREATE TRIGGER IF NOT EXISTS nodes_au AFTER UPDATE ON nodes BEGIN
                INSERT INTO nodes_fts(nodes_fts, rowid, title, content, tags)
                VALUES ('delete', old.rowid, old.title, old.content, old.tags);
                INSERT INTO nodes_fts(rowid, title, content, tags)
                VALUES (new.rowid, new.title, new.content, new.tags);
            END;

            CREATE INDEX IF NOT EXISTS idx_nodes_type ON nodes(node_type);
            CREATE INDEX IF NOT EXISTS idx_nodes_source ON nodes(source_project);
            CREATE INDEX IF NOT EXISTS idx_edges_from ON edges(from_id);
            CREATE INDEX IF NOT EXISTS idx_edges_to ON edges(to_id);",
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
            db_path: db_path.to_path_buf(),
            max_nodes,
        })
    }

    pub fn add_node(
        &self,
        node_type: NodeType,
        title: &str,
        content: &str,
        tags: &[String],
        source_project: Option<&str>,
    ) -> anyhow::Result<String> {
        let conn = self.conn.lock();

        let count: usize = conn.query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))?;
        if count >= self.max_nodes {
            anyhow::bail!(
                "knowledge graph node limit reached ({}/{})",
                count,
                self.max_nodes
            );
        }

        for tag in tags {
            if tag.contains(',') {
                anyhow::bail!(
                    "tag '{}' contains a comma, which is used as the tag separator",
                    tag
                );
            }
        }

        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let tags_str = tags.join(",");

        conn.execute(
            "INSERT INTO nodes (id, node_type, title, content, tags, created_at, updated_at, source_project)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                node_type.as_str(),
                title,
                content,
                tags_str,
                now,
                now,
                source_project,
            ],
        )?;

        Ok(id)
    }

    pub fn add_edge(&self, from_id: &str, to_id: &str, relation: Relation) -> anyhow::Result<()> {
        let conn = self.conn.lock();

        let exists = |id: &str| -> anyhow::Result<bool> {
            let c: usize = conn.query_row(
                "SELECT COUNT(*) FROM nodes WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )?;
            Ok(c > 0)
        };

        if !exists(from_id)? {
            anyhow::bail!("source node not found: {from_id}");
        }
        if !exists(to_id)? {
            anyhow::bail!("target node not found: {to_id}");
        }

        conn.execute(
            "INSERT OR IGNORE INTO edges (from_id, to_id, relation) VALUES (?1, ?2, ?3)",
            params![from_id, to_id, relation.as_str()],
        )?;

        Ok(())
    }

    pub fn get_node(&self, id: &str) -> anyhow::Result<Option<KnowledgeNode>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, node_type, title, content, tags, created_at, updated_at, source_project
             FROM nodes WHERE id = ?1",
        )?;

        let mut rows = stmt.query(params![id])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_node(row)?)),
            None => Ok(None),
        }
    }

    pub fn query_by_tags(&self, tags: &[String]) -> anyhow::Result<Vec<KnowledgeNode>> {
        let conn = self.conn.lock();
        if tags.is_empty() {
            return Ok(Vec::new());
        }
        let like_clauses: Vec<String> = tags
            .iter()
            .enumerate()
            .map(|(i, _)| format!("tags LIKE ?{}", i + 1))
            .collect();
        let sql = format!(
            "SELECT id, node_type, title, content, tags, created_at, updated_at, source_project
             FROM nodes WHERE {} ORDER BY updated_at DESC",
            like_clauses.join(" AND ")
        );
        let mut stmt = conn.prepare(&sql)?;
        let like_params: Vec<String> = tags.iter().map(|t| format!("%{}%", t)).collect();
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = like_params
            .iter()
            .map(|p| p as &dyn rusqlite::types::ToSql)
            .collect();
        let mut rows = stmt.query(param_refs.as_slice())?;
        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            let node = row_to_node(row)?;
            if tags.iter().all(|t| node.tags.contains(t)) {
                results.push(node);
            }
        }
        Ok(results)
    }

    pub fn query_by_similarity(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<SearchResult>> {
        let conn = self.conn.lock();

        let sanitized: String = query
            .split_whitespace()
            .map(|w| format!("\"{}\"", w.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" ");

        if sanitized.is_empty() {
            return Ok(Vec::new());
        }

        let mut stmt = conn.prepare(
            "SELECT n.id, n.node_type, n.title, n.content, n.tags,
                    n.created_at, n.updated_at, n.source_project,
                    rank
             FROM nodes_fts f
             JOIN nodes n ON n.rowid = f.rowid
             WHERE nodes_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;

        let mut results = Vec::new();
        let mut rows = stmt.query(params![sanitized, limit as i64])?;
        while let Some(row) = rows.next()? {
            let node = row_to_node(row)?;
            let rank: f64 = row.get(8)?;
            results.push(SearchResult {
                node,
                score: -rank,
            });
        }
        Ok(results)
    }

    pub fn find_related(&self, node_id: &str) -> anyhow::Result<Vec<(KnowledgeNode, Relation)>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT n.id, n.node_type, n.title, n.content, n.tags,
                    n.created_at, n.updated_at, n.source_project,
                    e.relation
             FROM edges e
             JOIN nodes n ON n.id = e.to_id
             WHERE e.from_id = ?1
             UNION ALL
             SELECT n.id, n.node_type, n.title, n.content, n.tags,
                    n.created_at, n.updated_at, n.source_project,
                    e.relation
             FROM edges e
             JOIN nodes n ON n.id = e.from_id
             WHERE e.to_id = ?1",
        )?;

        let mut results = Vec::new();
        let mut rows = stmt.query(params![node_id])?;
        while let Some(row) = rows.next()? {
            let node = row_to_node(row)?;
            let relation_str: String = row.get(8)?;
            let relation = Relation::parse(&relation_str)?;
            results.push((node, relation));
        }
        Ok(results)
    }

    const MAX_SUBGRAPH_DEPTH: usize = 100;

    pub fn get_subgraph(
        &self,
        root_id: &str,
        depth: usize,
    ) -> anyhow::Result<(Vec<KnowledgeNode>, Vec<KnowledgeEdge>)> {
        if depth == 0 {
            anyhow::bail!("subgraph depth must be greater than 0");
        }
        let depth = depth.min(Self::MAX_SUBGRAPH_DEPTH);
        let conn = self.conn.lock();

        let mut node_stmt = conn.prepare(
            "WITH RECURSIVE reachable(id, depth) AS (
                SELECT ?1, 0
                UNION
                SELECT CASE WHEN e.from_id = r.id THEN e.to_id ELSE e.from_id END, r.depth + 1
                FROM reachable r
                JOIN edges e ON e.from_id = r.id OR e.to_id = r.id
                WHERE r.depth < ?2
             )
             SELECT DISTINCT n.id, n.node_type, n.title, n.content, n.tags,
                    n.created_at, n.updated_at, n.source_project
             FROM reachable rc
             JOIN nodes n ON n.id = rc.id",
        )?;

        let mut nodes = Vec::new();
        let mut node_ids: HashSet<String> = HashSet::new();
        let mut rows = node_stmt.query(params![root_id, depth as i64])?;
        while let Some(row) = rows.next()? {
            let node = row_to_node(row)?;
            node_ids.insert(node.id.clone());
            nodes.push(node);
        }
        drop(rows);

        let mut edges = Vec::new();
        if !node_ids.is_empty() {
            let placeholders: Vec<String> = (1..=node_ids.len()).map(|i| format!("?{i}")).collect();
            let in_clause = placeholders.join(",");
            let edge_sql = format!(
                "SELECT from_id, to_id, relation FROM edges WHERE from_id IN ({}) AND to_id IN ({})",
                in_clause, in_clause
            );
            let mut edge_stmt = conn.prepare(&edge_sql)?;
            let ids_vec: Vec<String> = node_ids.iter().cloned().collect();
            let mut params: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
            for id in &ids_vec {
                params.push(id);
            }
            for id in &ids_vec {
                params.push(id);
            }
            let mut edge_rows = edge_stmt.query(params.as_slice())?;
            while let Some(row) = edge_rows.next()? {
                let from_id: String = row.get(0)?;
                let to_id: String = row.get(1)?;
                let relation_str: String = row.get(2)?;
                let relation = Relation::parse(&relation_str)?;
                edges.push(KnowledgeEdge {
                    from_id,
                    to_id,
                    relation,
                });
            }
        }

        Ok((nodes, edges))
    }

    pub fn find_experts(&self, tags: &[String]) -> anyhow::Result<Vec<SearchResult>> {

        let matching = self.query_by_tags(tags)?;
        let mut expert_scores: HashMap<String, f64> = HashMap::new();

        let conn = self.conn.lock();
        for node in &matching {
            let mut stmt = conn.prepare(
                "SELECT to_id FROM edges WHERE from_id = ?1 AND relation = 'authored_by'",
            )?;
            let mut rows = stmt.query(params![node.id])?;
            while let Some(row) = rows.next()? {
                let expert_id: String = row.get(0)?;
                *expert_scores.entry(expert_id).or_default() += 1.0;
            }
        }
        drop(conn);

        let mut results: Vec<SearchResult> = Vec::new();
        for (eid, score) in expert_scores {
            if let Some(node) = self.get_node(&eid)? {
                if node.node_type == NodeType::Expert {
                    results.push(SearchResult { node, score });
                }
            }
        }

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(results)
    }

    pub fn stats(&self) -> anyhow::Result<GraphStats> {
        let conn = self.conn.lock();

        let total_nodes: usize = conn.query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))?;
        let total_edges: usize = conn.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))?;

        let mut by_type = HashMap::new();
        {
            let mut stmt =
                conn.prepare("SELECT node_type, COUNT(*) FROM nodes GROUP BY node_type")?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let t: String = row.get(0)?;
                let c: usize = row.get(1)?;
                by_type.insert(t, c);
            }
        }

        let mut tag_counts: HashMap<String, usize> = HashMap::new();
        {
            let mut stmt = conn.prepare("SELECT tags FROM nodes WHERE tags != ''")?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let tags_str: String = row.get(0)?;
                for tag in tags_str.split(',') {
                    let tag = tag.trim();
                    if !tag.is_empty() {
                        *tag_counts.entry(tag.to_string()).or_default() += 1;
                    }
                }
            }
        }
        let mut top_tags: Vec<(String, usize)> = tag_counts.into_iter().collect();
        top_tags.sort_by(|a, b| b.1.cmp(&a.1));
        top_tags.truncate(10);

        Ok(GraphStats {
            total_nodes,
            total_edges,
            nodes_by_type: by_type,
            top_tags,
        })
    }
}

fn row_to_node(row: &rusqlite::Row<'_>) -> anyhow::Result<KnowledgeNode> {
    let id: String = row.get(0)?;
    let node_type_str: String = row.get(1)?;
    let title: String = row.get(2)?;
    let content: String = row.get(3)?;
    let tags_str: String = row.get(4)?;
    let created_at_str: String = row.get(5)?;
    let updated_at_str: String = row.get(6)?;
    let source_project: Option<String> = row.get(7)?;

    let tags: Vec<String> = tags_str
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    Ok(KnowledgeNode {
        id,
        node_type: NodeType::parse(&node_type_str)?,
        title,
        content,
        tags,
        created_at,
        updated_at,
        source_project,
    })
}
