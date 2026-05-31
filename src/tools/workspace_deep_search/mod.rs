// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod chunker;
pub mod planner;
pub mod ranker;
pub mod recall;
pub mod reflect;
pub mod tracer;

use crate::security::SecurityPolicy;
use crate::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct WorkspaceDeepSearchTool {
    security: Arc<SecurityPolicy>,
}

impl WorkspaceDeepSearchTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for WorkspaceDeepSearchTool {
    fn name(&self) -> &str {
        "workspace_deep_search"
    }

    fn description(&self) -> &str {
        "Local workspace DeepSearch: query planner → multi-route recall (BM25-style token frequency via ripgrep + structural / fuzzy path matching) → paragraph & code chunker → blended rerank → reflection (relaxed re-query for uncovered tokens) → traced output with path:lineStart-lineEnd citations. \
         Use this when you need explanatory or design-level answers from the workspace rather than a single regex hit. Prefer content_search/grep when you have an exact pattern."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Natural-language question or topic to investigate inside the workspace." },
                "scope": {
                    "type": "string",
                    "description": "Optional sub-path (relative to workspace root) to restrict the search. Defaults to '.'.",
                    "default": "."
                },
                "include_globs": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional ripgrep --glob include patterns."
                },
                "exclude_globs": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional ripgrep --glob '!pat' exclude patterns."
                },
                "max_results": { "type": "integer", "minimum": 1, "maximum": 30, "description": "Maximum traced chunks to return (default 8)." },
                "context_lines": { "type": "integer", "minimum": 0, "maximum": 20, "description": "Lines of context kept around matches (default 4)." },
                "enable_reflection": { "type": "boolean", "description": "Run a relaxed re-query for query tokens that produced no chunks (default true)." },
                "languages": { "type": "array", "items": { "type": "string" }, "description": "Optional file-type filters (rust|ts|js|py|md|toml|json|...)." }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("workspace_deep_search requires non-empty 'query'"))?
            .to_string();
        let scope = args
            .get("scope")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| ".".to_string());
        let include_globs = collect_string_array(args.get("include_globs"));
        let exclude_globs = collect_string_array(args.get("exclude_globs"));
        let languages = collect_string_array(args.get("languages"));
        let max_results = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(8)
            .clamp(1, 30);
        let context_lines = args
            .get("context_lines")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(4)
            .min(20);
        let enable_reflection = args
            .get("enable_reflection")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let workspace_root = self.security.workspace_dir();
        let scope_path = workspace_root.join(&scope);
        if !scope_path.exists() {
            anyhow::bail!(
                "Scope path '{}' does not exist (resolved {})",
                scope,
                scope_path.display()
            );
        }
        if !scope_path.starts_with(&workspace_root) && scope_path.canonicalize().ok().is_none_or(|p| !p.starts_with(&workspace_root)) {
            anyhow::bail!(
                "Scope '{}' escapes workspace root {}",
                scope,
                workspace_root.display()
            );
        }

        let plan = planner::plan_query(&query, &languages);
        tracing::info!(
            "workspace_deep_search query='{}' tokens={} scope='{}'",
            query,
            plan.tokens.len(),
            scope_path.display()
        );

        let recall = recall::run_recall(
            &workspace_root,
            &scope_path,
            &plan,
            &include_globs,
            &exclude_globs,
            context_lines,
        )
        .await?;
        let mut chunks = chunker::build_chunks(&workspace_root, &recall, context_lines).await;
        let plan_for_rank = plan.clone();
        let mut chunks = tokio::task::spawn_blocking(move || {
            ranker::rerank(&mut chunks, &plan_for_rank);
            chunks
        })
        .await?;

        let coverage = reflect::coverage(&chunks, &plan);
        let reflection_report = if enable_reflection && coverage.missing.iter().any(|m| !m.is_empty()) {
            let relaxed = recall::run_recall(
                &workspace_root,
                &scope_path,
                &plan.relaxed_for(&coverage.missing),
                &include_globs,
                &exclude_globs,
                context_lines,
            )
            .await
            .unwrap_or_default();
            let mut extra =
                chunker::build_chunks(&workspace_root, &relaxed, context_lines).await;
            let plan_for_rank = plan.clone();
            let extra = tokio::task::spawn_blocking(move || {
                ranker::rerank(&mut extra, &plan_for_rank);
                extra
            })
            .await?;
            let merged_added = ranker::merge_into(&mut chunks, extra);
            Some(reflect::format_report(&coverage, merged_added))
        } else {
            Some(reflect::format_report(&coverage, 0))
        };

        let top = ranker::take_top(chunks, max_results);
        let body = tracer::render(&query, &plan, &top, reflection_report.as_deref());
        Ok(ToolResult {
            success: true,
            output: body,
            error: None,
        })
    }
}

fn collect_string_array(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.trim().to_string())
                .collect()
        })
        .unwrap_or_default()
}
