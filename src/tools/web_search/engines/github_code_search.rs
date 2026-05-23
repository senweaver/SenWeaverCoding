// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::github_common::{build_code_hit, github_api_get, items_array};
use async_trait::async_trait;

pub struct GitHubCodeSearchEngine;

#[async_trait]
impl SearchEngine for GitHubCodeSearchEngine {
    fn id(&self) -> &'static str {
        "github_code_search"
    }

    fn label(&self) -> &'static str {
        "GitHub Code"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Code]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let body = github_api_get(
            ctx,
            "code",
            Some("application/vnd.github.v3.text-match+json"),
        )
        .await?;
        let items = items_array(&body);
        let mut hits = Vec::new();
        for item in items.iter().take(ctx.limit) {
            if let Some(hit) = build_code_hit(self.id(), item) {
                hits.push(hit);
            }
        }
        Ok(hits)
    }
}
