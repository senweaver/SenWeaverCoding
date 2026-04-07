// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// System prompt assembly — mirrors claude-code-typescript-src`constants/systemPromptSections.ts`
// and `utils/queryContext.ts`. Builds the multi-section system prompt.

use serde::{Deserialize, Serialize};

/// The assembled system prompt, broken into cacheable sections.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SystemPromptParts {
    /// Core identity / role section.
    pub identity: String,
    /// Tool usage instructions.
    pub tool_instructions: String,
    /// Project-specific context (AGENTS.md / CLAUDE.md).
    pub project_context: String,
    /// Memory context.
    pub memory_context: String,
    /// Environment context (OS, shell, git, date).
    pub environment_context: String,
    /// Additional injections (from flags, hooks, etc.).
    pub injections: Vec<String>,
}

impl SystemPromptParts {
    /// Concatenate all parts into the final system prompt string.
    pub fn assemble(&self) -> String {
        let mut parts = Vec::new();

        if !self.identity.is_empty() {
            parts.push(self.identity.clone());
        }
        if !self.tool_instructions.is_empty() {
            parts.push(self.tool_instructions.clone());
        }
        if !self.project_context.is_empty() {
            parts.push(self.project_context.clone());
        }
        if !self.memory_context.is_empty() {
            parts.push(self.memory_context.clone());
        }
        if !self.environment_context.is_empty() {
            parts.push(self.environment_context.clone());
        }
        for injection in &self.injections {
            if !injection.is_empty() {
                parts.push(injection.clone());
            }
        }

        parts.join("\n\n")
    }

    /// Estimate total token count (rough).
    pub fn estimated_tokens(&self) -> u64 {
        let total_chars = self.assemble().len() as f64;
        (total_chars / 3.5).ceil() as u64
    }

    /// Build the default identity section.
    pub fn default_identity(agent_name: &str) -> String {
        format!(
            "You are {agent_name}, an autonomous AI coding agent. You help users with \
             software engineering tasks by reading files, writing code, running commands, \
             and managing project workflows. You operate within the user's development \
             environment and have access to their filesystem and tools."
        )
    }

    /// Build the environment context section.
    pub fn build_environment_context(
        os: &str,
        shell: &str,
        cwd: &str,
        date: &str,
        git_info: Option<&str>,
    ) -> String {
        let mut lines = vec![
            format!("Operating System: {os}"),
            format!("Shell: {shell}"),
            format!("Working Directory: {cwd}"),
            format!("Current Date: {date}"),
        ];
        if let Some(git) = git_info {
            lines.push(format!("Git:\n{git}"));
        }
        format!("<environment>\n{}\n</environment>", lines.join("\n"))
    }
}
