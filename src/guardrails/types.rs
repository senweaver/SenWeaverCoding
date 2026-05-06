// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Guardrails type re-exports and supplementary types.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardrailCheckRecord {

    pub tool_name: String,

    pub allowed: bool,

    pub policy: String,

    pub reason: String,

    pub timestamp: String,

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
