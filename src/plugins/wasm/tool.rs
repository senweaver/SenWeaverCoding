// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::Value;

pub struct WasmTool {
    name: String,
    description: String,
    plugin_name: String,
    function_name: String,
    parameters_schema: Value,
}

impl WasmTool {
    pub fn new(
        name: String,
        description: String,
        plugin_name: String,
        function_name: String,
        parameters_schema: Value,
    ) -> Self {
        Self {
            name,
            description,
            plugin_name,
            function_name,
            parameters_schema,
        }
    }
}

#[async_trait]
impl Tool for WasmTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> Value {
        self.parameters_schema.clone()
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let input = serde_json::to_vec(&args)?;

        let manifest = extism::Manifest::new([extism::Wasm::file(&self.plugin_name)]);
        let mut plugin = extism::Plugin::new(manifest, [], true).map_err(|e| {
            anyhow::anyhow!("Failed to load WASM plugin '{}': {e}", self.plugin_name)
        })?;

        match plugin.call::<&[u8], &[u8]>(&self.function_name, &input) {
            Ok(output) => {
                let output_str = String::from_utf8_lossy(output).to_string();
                Ok(ToolResult {
                    success: true,
                    output: output_str,
                    error: None,
                })
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("WASM execution error: {e}")),
            }),
        }
    }
}
