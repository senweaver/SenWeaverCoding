// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::device::DeviceRegistry;
use super::uf2;
use crate::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

const PORT_WAIT_SECS: u64 = 20;

const PORT_POLL_MS: u64 = 500;

pub struct PicoFlashTool {
    registry: Arc<RwLock<DeviceRegistry>>,
}

impl PicoFlashTool {
    pub fn new(registry: Arc<RwLock<DeviceRegistry>>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Tool for PicoFlashTool {
    fn name(&self) -> &str {
        "pico_flash"
    }

    fn description(&self) -> &str {
        "Flash SenWeaverCoding firmware to a Raspberry Pi Pico in BOOTSEL mode. \
         The Pico must be connected with the BOOTSEL button held (shows as RPI-RP2 drive in Finder). \
         After flashing the Pico reboots, main.py is deployed, and the serial \
         connection is refreshed automatically — no restart needed."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "confirm": {
                    "type": "boolean",
                    "description": "Set to true to confirm flashing the Pico firmware"
                }
            },
            "required": ["confirm"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {

        let confirmed = args
            .get("confirm")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if !confirmed {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    "Set confirm=true to proceed with flashing. \
                     This will overwrite the firmware on the connected Pico."
                        .to_string(),
                ),
            });
        }

        let mount = match uf2::find_rpi_rp2_mount() {
            Some(m) => m,
            None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(
                        "No Pico in BOOTSEL mode found (RPI-RP2 drive not detected). \
                         Hold the BOOTSEL button while plugging the Pico in via USB, \
                         then try again."
                            .to_string(),
                    ),
                });
            }
        };

        tracing::info!(mount = %mount.display(), "RPI-RP2 volume found");

        let firmware_dir = match uf2::ensure_firmware_dir() {
            Ok(d) => d,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("firmware error: {e}")),
                });
            }
        };

        if let Err(e) = uf2::flash_uf2(&mount, &firmware_dir).await {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("flash failed: {e}")),
            });
        }

        let port = uf2::wait_for_serial_port(
            std::time::Duration::from_secs(PORT_WAIT_SECS),
            std::time::Duration::from_millis(PORT_POLL_MS),
        )
        .await;

        let port = match port {
            Some(p) => p,
            None => {

                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "UF2 copied to {} but serial port did not appear within {PORT_WAIT_SECS}s. \
                         Unplug and replug the Pico, then run:\n  \
                         mpremote connect <port> cp ~/.senweavercoding/firmware/pico/main.py :main.py + reset",
                        mount.display()
                    )),
                });
            }
        };

        tracing::info!(port = %port.display(), "Pico serial port online after UF2 flash");

        if let Err(e) = uf2::deploy_main_py(&port, &firmware_dir).await {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("main.py deploy failed: {e}")),
            });
        }

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let final_port = uf2::wait_for_serial_port(
            std::time::Duration::from_secs(PORT_WAIT_SECS),
            std::time::Duration::from_millis(PORT_POLL_MS),
        )
        .await;

        let reconnect_result = match &final_port {
            Some(p) => {
                let port_str = p.to_string_lossy();
                let mut reg = self.registry.write().await;

                match reg.aliases().into_iter().find(|a| a.starts_with("pico")) {
                    Some(a) => {
                        let alias = a.to_string();
                        reg.reconnect(&alias, Some(&port_str)).await
                    }
                    None => Err(anyhow::anyhow!(
                        "no pico alias found in registry; cannot reconnect transport"
                    )),
                }
            }
            None => Err(anyhow::anyhow!("no serial port to reconnect")),
        };

        match final_port {
            Some(p) => {
                let port_str = p.display().to_string();
                let reconnected = reconnect_result.is_ok();
                if reconnected {
                    tracing::info!(port = %port_str, "Pico online with main.py — transport reconnected");
                } else {
                    let err = reconnect_result.unwrap_err();
                    tracing::warn!(port = %port_str, err = %err, "Pico online but reconnect failed");
                }
                let suffix = if reconnected {
                    "pico0 is ready — you can use gpio_write immediately."
                } else {
                    "Restart SenWeaverCoding to reconnect as pico0."
                };
                Ok(ToolResult {
                    success: true,
                    output: format!(
                        "Pico flashed and main.py deployed successfully. \
                         Firmware is online at {port_str}. {suffix}"
                    ),
                    error: None,
                })
            }
            None => Ok(ToolResult {
                success: true,
                output: format!(
                    "Pico flashed and main.py deployed. \
                         Serial port did not reappear within {PORT_WAIT_SECS}s after reset — \
                         unplug and replug the Pico, then restart SenWeaverCoding to connect as pico0."
                ),
                error: None,
            }),
        }
    }
}
