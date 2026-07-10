// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use crate::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::Value;

const WASM_CALL_TIMEOUT_MS: u64 = 60_000;

type PluginCache = parking_lot::Mutex<HashMap<String, Arc<parking_lot::Mutex<extism::Plugin>>>>;

fn plugin_cache() -> &'static PluginCache {
    static CACHE: OnceLock<PluginCache> = OnceLock::new();
    CACHE.get_or_init(|| parking_lot::Mutex::new(HashMap::new()))
}

fn load_plugin(plugin_path: &str) -> anyhow::Result<Arc<parking_lot::Mutex<extism::Plugin>>> {
    if let Some(plugin) = plugin_cache().lock().get(plugin_path).cloned() {
        return Ok(plugin);
    }
    let manifest = extism::Manifest::new([extism::Wasm::file(plugin_path)])
        .with_timeout(std::time::Duration::from_millis(WASM_CALL_TIMEOUT_MS));
    let plugin = extism::Plugin::new(manifest, [], true)
        .map_err(|e| anyhow::anyhow!("Failed to load WASM plugin '{plugin_path}': {e}"))?;
    let plugin = Arc::new(parking_lot::Mutex::new(plugin));
    plugin_cache()
        .lock()
        .insert(plugin_path.to_string(), plugin.clone());
    Ok(plugin)
}

fn evict_plugin(plugin_path: &str) {
    plugin_cache().lock().remove(plugin_path);
}

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
        let plugin_path = self.plugin_name.clone();
        let function_name = self.function_name.clone();

        tokio::task::spawn_blocking(move || {
            let plugin = load_plugin(&plugin_path)?;
            let mut guard = plugin.lock();
            match guard.call::<&[u8], &[u8]>(&function_name, &input) {
                Ok(output) => {
                    let output_str = String::from_utf8_lossy(output).to_string();
                    Ok(ToolResult {
                        success: true,
                        output: output_str,
                        error: None,
                    })
                }
                Err(e) => {
                    drop(guard);
                    evict_plugin(&plugin_path);
                    Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("WASM execution error: {e}")),
                    })
                }
            }
        })
        .await
        .map_err(|e| anyhow::anyhow!("WASM execution task failed: {e}"))?
    }
}
