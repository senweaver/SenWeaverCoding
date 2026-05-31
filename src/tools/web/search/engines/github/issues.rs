// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::common::{build_issue_hit, github_api_get, items_array};
use async_trait::async_trait;

pub struct GitHubIssuesEngine;

#[async_trait]
impl SearchEngine for GitHubIssuesEngine {
    fn id(&self) -> &'static str {
        "github_issues"
    }

    fn label(&self) -> &'static str {
        "GitHub Issues / PRs"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Forum, SearchCategory::Code]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let body = github_api_get(ctx, "issues", None).await?;
        let items = items_array(&body);
        let mut hits = Vec::new();
        for item in items.iter().take(ctx.limit) {
            if let Some(hit) = build_issue_hit(self.id(), item) {
                hits.push(hit);
            }
        }
        Ok(hits)
    }
}
