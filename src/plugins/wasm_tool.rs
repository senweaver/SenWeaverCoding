// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Bridge between WASM plugins and the Tool trait.

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

    #[cfg(feature = "plugins-wasm")]
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

    #[cfg(not(feature = "plugins-wasm"))]
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let input = serde_json::to_string(&args)?;

        match std::process::Command::new("wasmtime")
            .args(["run", "--invoke", &self.function_name, &self.plugin_name])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(mut child) => {
                if let Some(stdin) = child.stdin.as_mut() {
                    use std::io::Write;
                    let _ = stdin.write_all(input.as_bytes());
                }
                match child.wait_with_output() {
                    Ok(output) => {
                        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                        if output.status.success() {
                            Ok(ToolResult {
                                success: true,
                                output: stdout,
                                error: None,
                            })
                        } else {
                            Ok(ToolResult {
                                success: false,
                                output: stdout,
                                error: Some(stderr),
                            })
                        }
                    }
                    Err(e) => Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("WASM process error: {e}")),
                    }),
                }
            }
            Err(_) => Ok(ToolResult {
                success: false,
                output: format!(
                    "[plugin:{}/{}] Input: {input}",
                    self.plugin_name, self.function_name
                ),
                error: Some(
                    "WASM runtime not found. Install wasmtime or add extism dependency.".into(),
                ),
            }),
        }
    }
}
