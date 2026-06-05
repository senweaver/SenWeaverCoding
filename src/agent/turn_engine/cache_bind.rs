// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::OnceLock;
use std::time::Duration;

use serde_json::Value;

pub const TOOL_CACHE_SESSION_FALLBACK: &str = "default";

fn unscoped_cache_namespace() -> &'static str {
    static NS: OnceLock<String> = OnceLock::new();
    NS.get_or_init(|| {
        format!(
            "{}__{}__{}",
            TOOL_CACHE_SESSION_FALLBACK,
            std::process::id(),
            uuid::Uuid::new_v4()
        )
    })
}

fn tool_cache_namespace() -> String {
    crate::session::current_session_context()
        .map(|ctx| ctx.session_id)
        .unwrap_or_else(|| unscoped_cache_namespace().to_string())
}

#[derive(Debug, Clone)]
pub struct ToolCacheEntry {
    pub output: String,
}

pub fn try_tool_cache_hit(tool_name: &str, fingerprint: &str) -> Option<ToolCacheEntry> {
    let svc = crate::services::try_get_services()?;
    let namespace = tool_cache_namespace();
    let cached =
        svc.blackboard
            .get_fresh_tool_result(&namespace, tool_name, fingerprint)?;
    let s = cached.as_str()?;
    Some(ToolCacheEntry {
        output: s.to_string(),
    })
}

pub fn write_tool_cache(tool_name: &str, fingerprint: &str, output: String, ttl_secs: u64) {
    let Some(svc) = crate::services::try_get_services() else {
        return;
    };
    let namespace = tool_cache_namespace();
    svc.blackboard.put_tool_result(
        &namespace,
        tool_name,
        fingerprint,
        Value::String(output),
        Duration::from_secs(ttl_secs),
    );
}
