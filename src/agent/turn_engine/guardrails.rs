// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Pre-execution guardrail + RBAC checks for tool calls.
//!
//! `loop_::execute_one_tool` performs two gate checks before it hands
//! the call to `tool.execute()`:
//!
//! 1. RBAC authorization via `RbacEngine::authorize_tool` (only when
//!    the gateway / channel layer has injected an engine + identity).
//! 2. Project-level guardrails via `guardrails::check_tool_guardrails`.
//!
//! Both gates return structured reasons on denial; this module wraps
//! them in a single `GuardrailVerdict` enum so future `turn_engine`
//! call sites can treat them uniformly.

use std::sync::Arc;

use crate::security::rbac::{CallerIdentity, RbacEngine};

#[derive(Debug, Clone)]
pub enum GuardrailVerdict {

    Allow,

    RbacDenied { reason: String },

    GuardrailBlocked { reason: String },
}

pub fn check_rbac(
    rbac_engine: Option<&Arc<RbacEngine>>,
    rbac_identity: Option<&CallerIdentity>,
    tool_name: &str,
) -> Option<String> {
    let (engine, identity) = (rbac_engine?, rbac_identity?);
    let auth = engine.authorize_tool(identity, tool_name);
    if auth.allowed {
        None
    } else {
        Some(
            auth.reason
                .unwrap_or_else(|| "Tool not permitted for this identity".into()),
        )
    }
}

pub fn check_tool_guardrails(tool_name: &str) -> Option<String> {
    let coding_label = crate::services::try_get_services()
        .map(|svc| svc.coding_mode.read().label().to_string());
    let coding_label_lc = coding_label.as_deref().map(str::to_ascii_lowercase);
    let perm_mode_lc =
        crate::gateway::ws_desktop::desktop_runtime_state().permission_mode();
    let tool_lc = tool_name.to_ascii_lowercase();
    let ctx = crate::guardrails::GuardrailContext {
        coding_mode: coding_label_lc.as_deref(),
        permission_mode: Some(&perm_mode_lc),
        tool_name: Some(&tool_lc),
    };
    match crate::guardrails::check_tool_guardrails(tool_name, Some(&ctx)) {
        Ok(()) => None,
        Err(reason) => Some(reason),
    }
}

pub fn evaluate(
    rbac_engine: Option<&Arc<RbacEngine>>,
    rbac_identity: Option<&CallerIdentity>,
    tool_name: &str,
) -> GuardrailVerdict {
    if let Some(reason) = check_rbac(rbac_engine, rbac_identity, tool_name) {
        return GuardrailVerdict::RbacDenied { reason };
    }
    if let Some(reason) = check_tool_guardrails(tool_name) {
        return GuardrailVerdict::GuardrailBlocked { reason };
    }
    GuardrailVerdict::Allow
}
