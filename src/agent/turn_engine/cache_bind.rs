// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Blackboard tool-result cache binding for the turn engine.
//!
//! `execute_one_tool` (in `agent::loop_`) consults the per-session
//! Blackboard for a fresh cached result *before* running a tool, and
//! writes back on success.  The lookup/write-back logic was inline in
//! ; D2.1 extracts it here so both the legacy path and
//! the forthcoming `turn_engine::tool_exec` share the same helpers.
//!
//! Session scope: we currently use a process-global key
//! (`"default"`) which matches 's behavior.  A real session id
//! will land with the SessionCore wiring (D3.2+).

use std::time::Duration;

use serde_json::Value;

pub const TOOL_CACHE_SESSION: &str = "default";

#[derive(Debug, Clone)]
pub struct ToolCacheEntry {
    pub output: String,
}

pub fn try_tool_cache_hit(tool_name: &str, fingerprint: &str) -> Option<ToolCacheEntry> {
    let svc = crate::services::try_get_services()?;
    let cached =
        svc.blackboard
            .get_fresh_tool_result(TOOL_CACHE_SESSION, tool_name, fingerprint)?;
    let s = cached.as_str()?;
    Some(ToolCacheEntry {
        output: s.to_string(),
    })
}

pub fn write_tool_cache(tool_name: &str, fingerprint: &str, output: String, ttl_secs: u64) {
    let Some(svc) = crate::services::try_get_services() else {
        return;
    };
    svc.blackboard.put_tool_result(
        TOOL_CACHE_SESSION,
        tool_name,
        fingerprint,
        Value::String(output),
        Duration::from_secs(ttl_secs),
    );
}
