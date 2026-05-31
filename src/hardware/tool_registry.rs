// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::device::DeviceRegistry;
use super::gpio::gpio_tools;
use super::loader::scan_plugin_dir;
use crate::tools::traits::{Tool, ToolResult};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Debug, Error)]
pub enum ToolError {

    #[error("unknown tool: '{0}'")]
    UnknownTool(String),

    #[error("tool execution failed: {0}")]
    ExecutionFailed(String),
}

pub struct ToolRegistry {

    tools: HashMap<String, Box<dyn Tool>>,

    device_registry: Arc<RwLock<DeviceRegistry>>,
}

impl ToolRegistry {

    pub async fn load(devices: Arc<RwLock<DeviceRegistry>>) -> anyhow::Result<Self> {
        let mut tools: HashMap<String, Box<dyn Tool>> = HashMap::new();

        for tool in gpio_tools(devices.clone()) {
            let name = tool.name().to_string();
            if tools.contains_key(&name) {
                anyhow::bail!("duplicate built-in tool name: '{}'", name);
            }
            println!("[registry] loaded built-in: {}", name);
            tools.insert(name, tool);
        }

        #[cfg(feature = "hardware")]
        {
            let tool: Box<dyn Tool> =
                Box::new(super::pico::flash::PicoFlashTool::new(devices.clone()));
            let name = tool.name().to_string();
            if tools.contains_key(&name) {
                anyhow::bail!("duplicate built-in tool name: '{}'", name);
            }
            println!("[registry] loaded built-in: {}", name);
            tools.insert(name, tool);
        }

        #[cfg(feature = "hardware")]
        {
            for tool in super::pico::code::device_code_tools(devices.clone()) {
                let name = tool.name().to_string();
                if tools.contains_key(&name) {
                    anyhow::bail!("duplicate built-in tool name: '{}'", name);
                }
                println!("[registry] loaded built-in: {}", name);
                tools.insert(name, tool);
            }
        }

        #[cfg(feature = "hardware")]
        {
            let has_aardvark = {
                let reg = devices.read().await;
                reg.has_aardvark()
            };
            if has_aardvark {
                for tool in super::aardvark::tools::aardvark_tools(devices.clone()) {
                    let name = tool.name().to_string();
                    if tools.contains_key(&name) {
                        anyhow::bail!("duplicate built-in tool name: '{}'", name);
                    }
                    println!("[registry] loaded built-in: {}", name);
                    tools.insert(name, tool);
                }

                {
                    let tool: Box<dyn Tool> = Box::new(super::datasheet::DatasheetTool::new());
                    let name = tool.name().to_string();
                    if tools.contains_key(&name) {
                        anyhow::bail!("duplicate built-in tool name: '{}'", name);
                    }
                    println!("[registry] loaded built-in: {}", name);
                    tools.insert(name, tool);
                }
            }
        }

        let plugins = scan_plugin_dir();
        for plugin in plugins {
            if tools.contains_key(&plugin.name) {
                anyhow::bail!(
                    "duplicate tool name: plugin '{}' conflicts with an existing tool",
                    plugin.name
                );
            }
            println!(
                "[registry] loaded plugin: {} (v{})",
                plugin.name, plugin.version
            );
            tools.insert(plugin.name, plugin.tool);
        }

        println!("[registry] {} tools available", tools.len());

        {
            let reg = devices.read().await;
            let mut aliases = reg.aliases();
            aliases.sort_unstable();
            for alias in aliases {
                if let Some(device) = reg.get_device(alias) {
                    let port = device.port().unwrap_or("(native)");
                    println!("[registry] {} ready → {}", alias, port);
                }
            }
        }

        Ok(Self {
            tools,
            device_registry: devices,
        })
    }

    pub fn schemas(&self) -> Vec<serde_json::Value> {
        let mut schemas: Vec<serde_json::Value> = self
            .tools
            .values()
            .map(|tool| {
                serde_json::json!({
                    "name": tool.name(),
                    "description": tool.description(),
                    "parameters": tool.parameters_schema(),
                })
            })
            .collect();

        schemas.sort_by(|a, b| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .cmp(b["name"].as_str().unwrap_or(""))
        });

        schemas
    }

    pub async fn dispatch(
        &self,
        name: &str,
        args: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| ToolError::UnknownTool(name.to_string()))?;

        tool.execute(args)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))
    }

    pub fn list(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.tools.keys().map(|s| s.as_str()).collect();
        names.sort_unstable();
        names
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn device_registry(&self) -> Arc<RwLock<DeviceRegistry>> {
        self.device_registry.clone()
    }

    pub fn into_tools(self) -> Vec<Box<dyn Tool>> {
        let mut pairs: Vec<(String, Box<dyn Tool>)> = self.tools.into_iter().collect();
        pairs.sort_by(|(a, _), (b, _)| a.cmp(b));
        pairs.into_iter().map(|(_, tool)| tool).collect()
    }
}
