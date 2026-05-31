// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::super::engine::{SearchCategory, SearchContext, SearchEngine, SearchHit};
use super::common::{build_user_hit, github_api_get, items_array};
use async_trait::async_trait;

pub struct GitHubUsersEngine;

#[async_trait]
impl SearchEngine for GitHubUsersEngine {
    fn id(&self) -> &'static str {
        "github_users"
    }

    fn label(&self) -> &'static str {
        "GitHub Users"
    }

    fn categories(&self) -> &'static [SearchCategory] {
        &[SearchCategory::Social, SearchCategory::Code]
    }

    async fn search(&self, ctx: &SearchContext) -> anyhow::Result<Vec<SearchHit>> {
        let body = github_api_get(ctx, "users", None).await?;
        let items = items_array(&body);
        let mut hits = Vec::new();
        for item in items.iter().take(ctx.limit) {
            if let Some(hit) = build_user_hit(self.id(), item) {
                hits.push(hit);
            }
        }
        Ok(hits)
    }
}
