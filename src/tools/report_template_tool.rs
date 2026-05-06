// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Report template tool — standalone access to template engine.
//!
//! Exposes the report template engine directly so agents can render
//! templates with custom variable maps without going through ProjectIntelTool.

use super::report_templates;
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;

pub struct ReportTemplateTool;

impl ReportTemplateTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReportTemplateTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ReportTemplateTool {
    fn name(&self) -> &str {
        "report_template"
    }

    fn description(&self) -> &str {
        "Render a report template with custom variables. Supports weekly_status, sprint_review, risk_register, milestone_report in en/de/fr/it."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "template": {
                    "type": "string",
                    "enum": ["weekly_status", "sprint_review", "risk_register", "milestone_report"],
                    "description": "Template name"
                },
                "language": {
                    "type": "string",
                    "enum": ["en", "de", "fr", "it"],
                    "default": "en",
                    "description": "Language code"
                },
                "variables": {
                    "type": "object",
                    "description": "Map of placeholder names to values (e.g., {\"project_name\": \"Acme\"})"
                }
            },
            "required": ["template", "variables"]
        })
    }

    async fn execute(&self, params: serde_json::Value) -> anyhow::Result<ToolResult> {
        let template = params
            .get("template")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing template"))?;

        let language = params
            .get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("en");

        let variables = params
            .get("variables")
            .and_then(|v| v.as_object())
            .ok_or_else(|| anyhow::anyhow!("variables must be object"))?;

        let var_map: HashMap<String, String> = variables
            .iter()
            .map(|(k, v)| {
                let value_str = match v {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    serde_json::Value::Null
                    | serde_json::Value::Array(_)
                    | serde_json::Value::Object(_) => String::new(),
                };
                (k.clone(), value_str)
            })
            .collect();

        let rendered = report_templates::render_template(template, language, &var_map)?;

        Ok(ToolResult {
            success: true,
            output: rendered,
            error: None,
        })
    }
}
