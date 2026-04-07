// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Guardrails type re-exports and supplementary types.

use serde::{Deserialize, Serialize};

/// Summary of a guardrail check for logging/observability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardrailCheckRecord {
    /// Tool name that was checked.
    pub tool_name: String,
    /// Whether the call was allowed.
    pub allowed: bool,
    /// Policy that determined the outcome.
    pub policy: String,
    /// Reason for the decision.
    pub reason: String,
    /// Timestamp (ISO-8601).
    pub timestamp: String,
    /// Session or context identifier.
    pub context: Option<String>,
}

impl GuardrailCheckRecord {
    pub fn new(
        tool_name: impl Into<String>,
        allowed: bool,
        policy: impl Into<String>,
        reason: impl Into<String>,
        context: Option<String>,
    ) -> Self {
        Self {
            tool_name: tool_name.into(),
            allowed,
            policy: policy.into(),
            reason: reason.into(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            context,
        }
    }
}
