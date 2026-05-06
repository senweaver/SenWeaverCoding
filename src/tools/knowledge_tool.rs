// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Knowledge management tool for capturing, searching, and reusing expertise.
//!
//! Exposes the knowledge graph to the agent via the `Tool` trait with actions:
//! capture, search, relate, suggest, expert_find, lessons_extract, graph_stats.

use super::traits::{Tool, ToolResult};
use crate::memory::knowledge_graph::{KnowledgeGraph, NodeType, Relation};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct KnowledgeTool {
    graph: Arc<KnowledgeGraph>,
}

impl KnowledgeTool {
    pub fn new(graph: Arc<KnowledgeGraph>) -> Self {
        Self { graph }
    }
}

#[async_trait]
impl Tool for KnowledgeTool {
    fn name(&self) -> &str {
        "knowledge"
    }

    fn description(&self) -> &str {
        "Manage a knowledge graph of architecture decisions, solution patterns, lessons learned, and experts. Actions: capture, search, relate, suggest, expert_find, lessons_extract, graph_stats."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["capture", "search", "relate", "suggest", "expert_find", "lessons_extract", "graph_stats"],
                    "description": "The action to perform"
                },
                "node_type": {
                    "type": "string",
                    "enum": ["pattern", "decision", "lesson", "expert", "technology"],
                    "description": "Type of knowledge node (for capture)"
                },
                "title": {
                    "type": "string",
                    "description": "Title for the knowledge item (for capture)"
                },
                "content": {
                    "type": "string",
                    "description": "Content body (for capture) or text to extract lessons from (for lessons_extract)"
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Tags for filtering and categorization"
                },
                "source_project": {
                    "type": "string",
                    "description": "Source project identifier (for capture)"
                },
                "query": {
                    "type": "string",
                    "description": "Search query text (for search, suggest)"
                },
                "from_id": {
                    "type": "string",
                    "description": "Source node ID (for relate)"
                },
                "to_id": {
                    "type": "string",
                    "description": "Target node ID (for relate)"
                },
                "relation": {
                    "type": "string",
                    "enum": ["uses", "replaces", "extends", "authored_by", "applies_to"],
                    "description": "Relationship type (for relate)"
                },
                "filters": {
                    "type": "object",
                    "properties": {
                        "node_type": { "type": "string" },
                        "tags": { "type": "array", "items": { "type": "string" } },
                        "project": { "type": "string" }
                    },
                    "description": "Optional search filters"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing 'action' parameter"))?;

        match action {
            "capture" => self.handle_capture(&args),
            "search" => self.handle_search(&args),
            "relate" => self.handle_relate(&args),
            "suggest" => self.handle_suggest(&args),
            "expert_find" => self.handle_expert_find(&args),
            "lessons_extract" => self.handle_lessons_extract(&args),
            "graph_stats" => self.handle_graph_stats(),
            other => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("unknown action: {other}")),
            }),
        }
    }
}

impl KnowledgeTool {
    fn handle_capture(&self, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let node_type_str = args
            .get("node_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing 'node_type' for capture"))?;
        let title = args
            .get("title")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing 'title' for capture"))?;
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing 'content' for capture"))?;

        let node_type = NodeType::parse(node_type_str).map_err(|e| anyhow::anyhow!("{e}"))?;

        let tags: Vec<String> = args
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let source_project = args.get("source_project").and_then(|v| v.as_str());

        match self
            .graph
            .add_node(node_type, title, content, &tags, source_project)
        {
            Ok(id) => Ok(ToolResult {
                success: true,
                output: json!({ "node_id": id }).to_string(),
                error: None,
            }),
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("capture failed: {e}")),
            }),
        }
    }

    fn handle_search(&self, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");

        let filter_tags: Vec<String> = args
            .get("filters")
            .and_then(|f| f.get("tags"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let filter_type = args
            .get("filters")
            .and_then(|f| f.get("node_type"))
            .and_then(|v| v.as_str());

        let filter_project = args
            .get("filters")
            .and_then(|f| f.get("project"))
            .and_then(|v| v.as_str());

        let parsed_filter_type = filter_type.and_then(|ft| NodeType::parse(ft).ok());

        let results = if query.is_empty() && !filter_tags.is_empty() {

            let mut nodes = self.graph.query_by_tags(&filter_tags)?;
            if let Some(ref nt) = parsed_filter_type {
                nodes.retain(|n| &n.node_type == nt);
            }
            if let Some(proj) = filter_project {
                nodes.retain(|n| n.source_project.as_deref() == Some(proj));
            }
            nodes
                .into_iter()
                .map(|node| json!({ "id": node.id, "type": node.node_type, "title": node.title, "score": 1.0 }))
                .collect::<Vec<_>>()
        } else if !query.is_empty() {
            let mut search_results = self.graph.query_by_similarity(query, 20)?;

            if let Some(ref nt) = parsed_filter_type {
                search_results.retain(|r| &r.node.node_type == nt);
            }

            if let Some(proj) = filter_project {
                search_results.retain(|r| r.node.source_project.as_deref() == Some(proj));
            }

            if !filter_tags.is_empty() {
                search_results.retain(|r| filter_tags.iter().all(|t| r.node.tags.contains(t)));
            }

            search_results
                .into_iter()
                .map(|r| {
                    json!({
                        "id": r.node.id,
                        "type": r.node.node_type,
                        "title": r.node.title,
                        "score": r.score
                    })
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        Ok(ToolResult {
            success: true,
            output: json!({ "results": results, "count": results.len() }).to_string(),
            error: None,
        })
    }

    fn handle_relate(&self, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let from_id = args
            .get("from_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing 'from_id' for relate"))?;
        let to_id = args
            .get("to_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing 'to_id' for relate"))?;
        let relation_str = args
            .get("relation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing 'relation' for relate"))?;

        let relation = Relation::parse(relation_str).map_err(|e| anyhow::anyhow!("{e}"))?;

        match self.graph.add_edge(from_id, to_id, relation) {
            Ok(()) => Ok(ToolResult {
                success: true,
                output: "relationship created".to_string(),
                error: None,
            }),
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("relate failed: {e}")),
            }),
        }
    }

    fn handle_suggest(&self, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let query = args
            .get("query")
            .or_else(|| args.get("content"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing 'query' or 'content' for suggest"))?;

        let results = self.graph.query_by_similarity(query, 10)?;
        let suggestions: Vec<serde_json::Value> = results
            .into_iter()
            .map(|r| {
                json!({
                    "id": r.node.id,
                    "type": r.node.node_type,
                    "title": r.node.title,
                    "content_preview": truncate_str(&r.node.content, 200),
                    "tags": r.node.tags,
                    "relevance_score": r.score,
                })
            })
            .collect();

        Ok(ToolResult {
            success: true,
            output: json!({ "suggestions": suggestions, "count": suggestions.len() }).to_string(),
            error: None,
        })
    }

    fn handle_expert_find(&self, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let tags: Vec<String> = args
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        if tags.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("missing 'tags' for expert_find".into()),
            });
        }

        let experts = self.graph.find_experts(&tags)?;
        let output: Vec<serde_json::Value> = experts
            .into_iter()
            .map(|r| {
                json!({
                    "id": r.node.id,
                    "name": r.node.title,
                    "contribution_score": r.score,
                    "tags": r.node.tags,
                })
            })
            .collect();

        Ok(ToolResult {
            success: true,
            output: json!({ "experts": output, "count": output.len() }).to_string(),
            error: None,
        })
    }

    fn handle_lessons_extract(&self, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let text = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing 'content' for lessons_extract"))?;

        let signal_words = [
            "learned",
            "lesson",
            "mistake",
            "should have",
            "next time",
            "improvement",
            "better",
            "avoid",
            "risk",
            "issue",
            "root cause",
            "takeaway",
            "insight",
            "recommendation",
            "decision",
        ];

        let sentences: Vec<&str> = text
            .split(&['.', '!', '?', '\n'][..])
            .map(str::trim)
            .filter(|s| s.len() > 10)
            .collect();

        let mut lessons: Vec<serde_json::Value> = Vec::new();
        for sentence in &sentences {
            let lower = sentence.to_ascii_lowercase();
            let score: f64 = signal_words.iter().filter(|w| lower.contains(**w)).count() as f64;
            if score > 0.0 {
                lessons.push(json!({
                    "text": sentence,
                    "confidence": (score / signal_words.len() as f64).min(1.0),
                }));
            }
        }

        lessons.sort_by(|a, b| {
            let sa = a["confidence"].as_f64().unwrap_or(0.0);
            let sb = b["confidence"].as_f64().unwrap_or(0.0);
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        });
        lessons.truncate(10);

        Ok(ToolResult {
            success: true,
            output: json!({ "lessons": lessons, "count": lessons.len() }).to_string(),
            error: None,
        })
    }

    fn handle_graph_stats(&self) -> anyhow::Result<ToolResult> {
        match self.graph.stats() {
            Ok(stats) => Ok(ToolResult {
                success: true,
                output: serde_json::to_string(&stats).unwrap_or_default(),
                error: None,
            }),
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("failed to get stats: {e}")),
            }),
        }
    }
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len).collect();
        format!("{truncated}...")
    }
}
