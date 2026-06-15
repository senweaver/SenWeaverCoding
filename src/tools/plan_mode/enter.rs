// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

fn plan_mode_session_key() -> String {
    crate::session::current_session_context()
        .map(|c| c.session_id)
        .unwrap_or_else(|| "default".to_string())
}

#[derive(Clone, Default)]
pub struct PlanModeFlag {
    inner: Arc<RwLock<HashMap<String, bool>>>,
}

impl PlanModeFlag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&self, active: bool) {
        let key = plan_mode_session_key();
        if active {
            self.inner.write().insert(key, true);
        } else {
            self.inner.write().remove(&key);
        }
    }

    pub fn is_active(&self) -> bool {
        self.inner
            .read()
            .get(&plan_mode_session_key())
            .copied()
            .unwrap_or(false)
    }
}

pub struct EnterPlanModeTool {
    flag: PlanModeFlag,
}

impl EnterPlanModeTool {
    pub fn new(flag: PlanModeFlag) -> Self {
        Self { flag }
    }
}

#[async_trait]
impl Tool for EnterPlanModeTool {
    fn name(&self) -> &str {
        "enter_plan_mode"
    }

    fn description(&self) -> &str {
        "Enter plan mode to create a detailed plan before making changes. In plan mode, only read-only tools are available."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {},
            "required": [],
        })
    }

    async fn execute(&self, _args: serde_json::Value) -> anyhow::Result<ToolResult> {
        self.flag.set(true);
        Ok(ToolResult {
            success: true,
            output: "Entered plan mode. Only read-only tools are now available.".to_string(),
            error: None,
        })
    }
}
