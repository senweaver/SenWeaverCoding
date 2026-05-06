// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! GPIO tools — `gpio_read` and `gpio_write` for LLM-driven hardware control.
//!
//! These are the first built-in hardware tools. They implement the standard
//! [`Tool`](crate::tools::Tool) trait so the LLM can call them via function
//! calling, and dispatch commands to physical devices via the
//! [`Transport`](super::Transport) layer.
//!
//! Wire protocol (SenWeaverCoding serial JSON):
//! ```text
//! gpio_write:
//!   Host → Device:  {"cmd":"gpio_write","params":{"pin":25,"value":1}}\n
//!   Device → Host:  {"ok":true,"data":{"pin":25,"value":1,"state":"HIGH"}}\n
//!
//! gpio_read:
//!   Host → Device:  {"cmd":"gpio_read","params":{"pin":25}}\n
//!   Device → Host:  {"ok":true,"data":{"pin":25,"value":1,"state":"HIGH"}}\n
//! ```

use super::device::DeviceRegistry;
use super::protocol::ZcCommand;
use crate::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct GpioWriteTool {
    registry: Arc<RwLock<DeviceRegistry>>,
}

impl GpioWriteTool {
    pub fn new(registry: Arc<RwLock<DeviceRegistry>>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Tool for GpioWriteTool {
    fn name(&self) -> &str {
        "gpio_write"
    }

    fn description(&self) -> &str {
        "Set a GPIO pin HIGH (1) or LOW (0) on a connected hardware device"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "device": {
                    "type": "string",
                    "description": "Device alias e.g. pico0, arduino0"
                },
                "pin": {
                    "type": "integer",
                    "description": "GPIO pin number"
                },
                "value": {
                    "type": "integer",
                    "enum": [0, 1],
                    "description": "1 = HIGH (on), 0 = LOW (off)"
                }
            },
            "required": ["pin", "value"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let pin = match args.get("pin").and_then(|v| v.as_u64()) {
            Some(p) => p,
            None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("missing required parameter: pin".to_string()),
                });
            }
        };
        let value = match args.get("value").and_then(|v| v.as_u64()) {
            Some(v) => v,
            None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("missing required parameter: value".to_string()),
                });
            }
        };

        if value > 1 {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("value must be 0 or 1".to_string()),
            });
        }

        let (device_alias, ctx) = {
            let registry = self.registry.read().await;
            match registry.resolve_gpio_device(&args) {
                Ok(resolved) => resolved,
                Err(msg) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(msg),
                    });
                }
            }

        };

        let cmd = ZcCommand::new("gpio_write", json!({ "pin": pin, "value": value }));

        match ctx.transport.send(&cmd).await {
            Ok(resp) if resp.ok => {
                let state = resp
                    .data
                    .get("state")
                    .and_then(|v| v.as_str())
                    .unwrap_or(if value == 1 { "HIGH" } else { "LOW" });
                Ok(ToolResult {
                    success: true,
                    output: format!("GPIO {} set {} on {}", pin, state, device_alias),
                    error: None,
                })
            }
            Ok(resp) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    resp.error
                        .unwrap_or_else(|| "device returned ok:false".to_string()),
                ),
            }),
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("transport error: {}", e)),
            }),
        }
    }
}

pub struct GpioReadTool {
    registry: Arc<RwLock<DeviceRegistry>>,
}

impl GpioReadTool {
    pub fn new(registry: Arc<RwLock<DeviceRegistry>>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Tool for GpioReadTool {
    fn name(&self) -> &str {
        "gpio_read"
    }

    fn description(&self) -> &str {
        "Read the current HIGH/LOW state of a GPIO pin on a connected device"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "device": {
                    "type": "string",
                    "description": "Device alias e.g. pico0, arduino0"
                },
                "pin": {
                    "type": "integer",
                    "description": "GPIO pin number to read"
                }
            },
            "required": ["pin"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let pin = match args.get("pin").and_then(|v| v.as_u64()) {
            Some(p) => p,
            None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("missing required parameter: pin".to_string()),
                });
            }
        };

        let (device_alias, ctx) = {
            let registry = self.registry.read().await;
            match registry.resolve_gpio_device(&args) {
                Ok(resolved) => resolved,
                Err(msg) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(msg),
                    });
                }
            }

        };

        let cmd = ZcCommand::new("gpio_read", json!({ "pin": pin }));

        match ctx.transport.send(&cmd).await {
            Ok(resp) if resp.ok => {
                let value = resp.data.get("value").and_then(|v| v.as_u64()).unwrap_or(0);
                let state = resp
                    .data
                    .get("state")
                    .and_then(|v| v.as_str())
                    .unwrap_or(if value == 1 { "HIGH" } else { "LOW" });
                Ok(ToolResult {
                    success: true,
                    output: format!("GPIO {} is {} ({}) on {}", pin, state, value, device_alias),
                    error: None,
                })
            }
            Ok(resp) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    resp.error
                        .unwrap_or_else(|| "device returned ok:false".to_string()),
                ),
            }),
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("transport error: {}", e)),
            }),
        }
    }
}

pub fn gpio_tools(registry: Arc<RwLock<DeviceRegistry>>) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(GpioWriteTool::new(registry.clone())),
        Box::new(GpioReadTool::new(registry)),
    ]
}

