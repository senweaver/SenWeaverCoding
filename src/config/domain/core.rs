// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! target location for the top-level `Config`
//! aggregation logic extracted from `config::schema`.
//!
//! `schema.rs` currently holds ~14k lines covering both type
//! definitions *and* the glue logic that merges TOML layers into
//! the runtime `Config` struct.  The glue (merge / validate / hot
//! reload) will move here in the follow-up sprint; for now
//! we expose re-exports of the public surface so downstream code
//! can spell the imports as `crate::config::domain::core::*` today.
//!
//! Adding a new helper here MUST NOT duplicate logic that already
//! lives in `schema.rs`; prefer re-exports + thin wrappers until
//! the physical move happens.

#[doc(inline)]
pub use crate::config::schema::{Config, MCP_MAX_TOOL_TIMEOUT_SECS, validate_mcp_config};

#[inline]
pub fn workspace_dir(cfg: &Config) -> &std::path::Path {
    cfg.workspace_dir.as_path()
}
