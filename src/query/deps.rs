// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Query dependencies — mirrors claude-code-typescript-src`query/deps.ts`.
// Bundles the injectable dependencies that a query needs at execution time.

use std::path::PathBuf;
use std::sync::Arc;

use super::config::QueryConfig;

/// Dependencies injected into a query execution.
#[derive(Clone)]
pub struct QueryDeps {
    /// The working directory for tool execution.
    pub cwd: PathBuf,
    /// Model configuration for this query.
    pub config: QueryConfig,
    /// Abort signal — when set to `true` the query should stop.
    pub abort: Arc<tokio::sync::watch::Receiver<bool>>,
    /// Session ID for correlation.
    pub session_id: String,
    /// Agent ID (for sub-agent queries).
    pub agent_id: Option<String>,
    /// Whether this query is allowed to execute tools.
    pub tools_enabled: bool,
    /// Maximum number of tool-use turns before forcing a response.
    pub max_tool_turns: Option<u32>,
    /// Whether to persist the conversation to session storage.
    pub persist_session: bool,
}

impl QueryDeps {
    pub fn new(cwd: PathBuf, config: QueryConfig, session_id: String) -> Self {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        Self {
            cwd,
            config,
            abort: Arc::new(rx),
            session_id,
            agent_id: None,
            tools_enabled: true,
            max_tool_turns: None,
            persist_session: true,
        }
    }

    /// Check whether the query has been aborted.
    pub fn is_aborted(&self) -> bool {
        *self.abort.borrow()
    }
}
