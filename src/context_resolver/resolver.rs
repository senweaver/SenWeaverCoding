// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use std::path::{Path, PathBuf};

use super::budget::ContextBudget;
use super::handlers::resolve_tag;
use super::types::{ContextItem, ContextResolveError, ContextTag};

#[async_trait]
pub trait ContextResolver: Send + Sync {
    async fn resolve(
        &self,
        tags: Vec<ContextTag>,
        budget: &ContextBudget,
    ) -> Result<Vec<ContextItem>, ContextResolveError>;
    fn name(&self) -> &'static str;
}

#[derive(Debug, Clone)]
pub struct DefaultResolver {
    pub workspace_root: PathBuf,
    pub recent_files: Vec<PathBuf>,
    pub current_selection: String,
}

impl DefaultResolver {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            recent_files: Vec::new(),
            current_selection: String::new(),
        }
    }

    pub fn with_recent(mut self, files: Vec<PathBuf>) -> Self {
        self.recent_files = files;
        self
    }

    pub fn with_selection(mut self, selection: impl Into<String>) -> Self {
        self.current_selection = selection.into();
        self
    }
}

#[async_trait]
impl ContextResolver for DefaultResolver {
    async fn resolve(
        &self,
        tags: Vec<ContextTag>,
        budget: &ContextBudget,
    ) -> Result<Vec<ContextItem>, ContextResolveError> {
        let mut out = Vec::with_capacity(tags.len());
        for tag in tags {
            let before_used = budget.used();
            let item = resolve_tag(
                &tag,
                &self.workspace_root as &Path,
                &self.recent_files,
                &self.current_selection,
                budget,
            )?;
            crate::observability::subsystem_metrics::incr_context_resolution();

            let consumed = budget.used().saturating_sub(before_used);
            if consumed * 4 < item.body.len() {
                crate::observability::subsystem_metrics::incr_context_budget_clip();
            }
            out.push(item);
        }
        Ok(out)
    }
    fn name(&self) -> &'static str {
        "default"
    }
}
