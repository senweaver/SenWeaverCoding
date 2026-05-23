// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

#[doc(inline)]
pub use crate::config::schema::{Config, MCP_MAX_TOOL_TIMEOUT_SECS, validate_mcp_config};

#[inline]
pub fn workspace_dir(cfg: &Config) -> &std::path::Path {
    cfg.workspace_dir.as_path()
}
