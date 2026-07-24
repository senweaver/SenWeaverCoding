// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

pub fn render_json_output(value: &Value, verbose: bool) -> String {
    if verbose {
        serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
    } else {
        value.to_string()
    }
}

pub fn json_verbose_flag(args: &Value) -> bool {
    args.get("verbose")
        .and_then(|v| v.as_bool())
        .or_else(|| {
            args.get("format")
                .and_then(|v| v.as_str())
                .map(|f| f.eq_ignore_ascii_case("pretty"))
        })
        .unwrap_or(false)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[async_trait]
pub trait Tool: Send + Sync {

    fn name(&self) -> &str;

    fn description(&self) -> &str;

    fn parameters_schema(&self) -> serde_json::Value;

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult>;

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters_schema(),
        }
    }

    fn spec_with_descriptions(
        &self,
        descriptions: Option<&crate::i18n::ToolDescriptions>,
    ) -> ToolSpec {
        let mut spec = self.spec();
        if let Some(descs) = descriptions {
            if let Some(d) = descs.get(self.name()) {
                let localized = d.trim();
                let builtin = spec.description.trim();
                if !localized.is_empty()
                    && (builtin.is_empty() || localized.chars().count() * 2 >= builtin.chars().count())
                {
                    spec.description = d.to_string();
                }
            }
        }
        spec
    }

    fn validate_args(&self, _args: &Value) -> Result<(), String> {
        Ok(())
    }

    fn preflight_permission(&self, _args: &Value) -> bool {
        true
    }

    fn fingerprint(&self, _args: &Value) -> Option<String> {
        None
    }

    fn cache_ttl_secs(&self) -> u64 {
        300
    }

    fn mcp_safe(&self) -> bool {
        false
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        None
    }
}
