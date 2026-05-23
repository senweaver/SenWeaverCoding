// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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
