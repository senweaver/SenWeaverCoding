// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Context builder — assembles the full context for an agent query.
// Mirrors claude-code-typescript-src`context.ts` (getUserContext, getSystemContext).

use std::path::PathBuf;

use super::git::GitContext;
use super::memory_files::MemoryFileContext;
use super::system_prompt::SystemPromptParts;

/// Assembled context ready for query construction.
#[derive(Debug, Clone)]
pub struct QueryContext {
    pub system_prompt: SystemPromptParts,
    pub git: Option<GitContext>,
    pub memory: MemoryFileContext,
    pub cwd: PathBuf,
    pub date: String,
    pub additional_instructions: Vec<String>,
}

/// Builds the full context for an agent query by gathering all context sources.
pub struct ContextBuilder {
    cwd: PathBuf,
    additional_dirs: Vec<PathBuf>,
    system_prompt_injection: Option<String>,
}

impl ContextBuilder {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            additional_dirs: Vec::new(),
            system_prompt_injection: None,
        }
    }

    pub fn with_additional_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.additional_dirs = dirs;
        self
    }

    pub fn with_system_prompt_injection(mut self, injection: Option<String>) -> Self {
        self.system_prompt_injection = injection;
        self
    }

    /// Build the full query context.
    pub async fn build(&self) -> anyhow::Result<QueryContext> {
        // 1. Gather git context
        let git = GitContext::gather(&self.cwd).await.ok();

        // 2. Load AGENTS.md / CLAUDE.md files from cwd + additional dirs
        let mut search_dirs = vec![self.cwd.clone()];
        search_dirs.extend(self.additional_dirs.clone());
        let memory = MemoryFileContext::load(&search_dirs).await;

        // 3. Build system prompt parts
        let mut system_prompt = SystemPromptParts::default();
        if let Some(ref injection) = self.system_prompt_injection {
            system_prompt.injections.push(injection.clone());
        }

        // 4. Get current date
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();

        Ok(QueryContext {
            system_prompt,
            git,
            memory,
            cwd: self.cwd.clone(),
            date,
            additional_instructions: Vec::new(),
        })
    }
}
