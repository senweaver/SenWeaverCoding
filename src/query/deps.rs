// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Query dependencies — mirrors claude-code-typescript-src`query/deps.ts`.
// Bundles the injectable dependencies that a query needs at execution time.

use std::path::PathBuf;
use std::sync::Arc;

use super::config::QueryConfig;

#[derive(Clone)]
pub struct QueryDeps {

    pub cwd: PathBuf,

    pub config: QueryConfig,

    pub abort: Arc<tokio::sync::watch::Receiver<bool>>,

    pub session_id: String,

    pub agent_id: Option<String>,

    pub tools_enabled: bool,

    pub max_tool_turns: Option<u32>,

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

    pub fn is_aborted(&self) -> bool {
        *self.abort.borrow()
    }
}
