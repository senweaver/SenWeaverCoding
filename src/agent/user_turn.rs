// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};

use crate::agent::thinking::{ThinkingLevel, parse_thinking_directive};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserTurn {

    pub expanded: String,

    pub raw: String,

    pub context_prefix: String,

    pub thinking_level: Option<ThinkingLevel>,

    pub excluded_tools: Vec<String>,
}

impl UserTurn {

    pub fn is_empty(&self) -> bool {
        self.expanded.trim().is_empty()
    }

    pub fn payload(&self) -> String {
        if self.context_prefix.is_empty() {
            self.expanded.clone()
        } else {
            format!("{}\n\n{}", self.context_prefix, self.expanded)
        }
    }
}

#[allow(async_fn_in_trait)]
pub trait UserTurnDeps {

    fn workspace_dir(&self) -> &Path;

    async fn build_context(&self, message: &str) -> String;

    fn compute_excluded_tools(&self, message: &str) -> Vec<String>;
}

pub async fn prepare_user_turn(raw: String, deps: &impl UserTurnDeps) -> UserTurn {
    let trimmed = raw.trim().to_string();

    let expanded = crate::agent::context_expansion::expand_input(
        &trimmed,
        deps.workspace_dir(),
        Vec::new(),
        String::new(),
    );

    let (thinking_level, after_thinking) = match parse_thinking_directive(&expanded) {
        Some((level, rest)) => (Some(level), rest),
        None => (None, expanded),
    };

    let context_prefix = deps.build_context(&after_thinking).await;

    let excluded_tools = deps.compute_excluded_tools(&after_thinking);

    UserTurn {
        expanded: after_thinking.to_string(),
        raw: trimmed,
        context_prefix,
        thinking_level,
        excluded_tools,
    }
}

#[derive(Debug, Clone)]
pub struct FakeUserTurnDeps {
    pub workspace: PathBuf,
    pub context: String,
    pub excluded: Vec<String>,
}

impl Default for FakeUserTurnDeps {
    fn default() -> Self {
        Self {
            workspace: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            context: String::new(),
            excluded: Vec::new(),
        }
    }
}

impl UserTurnDeps for FakeUserTurnDeps {
    fn workspace_dir(&self) -> &Path {
        &self.workspace
    }

    async fn build_context(&self, _message: &str) -> String {
        self.context.clone()
    }

    fn compute_excluded_tools(&self, _message: &str) -> Vec<String> {
        self.excluded.clone()
    }
}
