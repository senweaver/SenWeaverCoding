// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::config::PeripheralBoardConfig;
use crate::peripherals::Peripheral;
use crate::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use portable_atomic::{AtomicU64, Ordering};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio_serial::{SerialPortBuilderExt, SerialStream};

const ALLOWED_PATH_PREFIXES: &[&str] = &[
    "/dev/ttyACM",
    "/dev/ttyUSB",
    "/dev/tty.usbmodem",
    "/dev/cu.usbmodem",
    "/dev/tty.usbserial",
    "/dev/cu.usbserial",
    "COM",
];

fn is_path_allowed(path: &str) -> bool {
    ALLOWED_PATH_PREFIXES.iter().any(|p| path.starts_with(p))
}

async fn send_request(port: &mut SerialStream, cmd: &str, args: Value) -> anyhow::Result<Value> {
    static ID: AtomicU64 = AtomicU64::new(0);
    let id = ID.fetch_add(1, Ordering::Relaxed);
    let id_str = id.to_string();

    let req = json!({
        "id": id_str,
        "cmd": cmd,
        "args": args
    });
    let line = format!("{}\n", req);

    port.write_all(line.as_bytes()).await?;
    port.flush().await?;

    const MAX_RESPONSE_SIZE: usize = 16_384;
    let mut buf = Vec::with_capacity(256);
    let mut b = [0u8; 1];
    let mut bytes_read = 0;
    while bytes_read < MAX_RESPONSE_SIZE && port.read_exact(&mut b).await.is_ok() {
        if b[0] == b'\n' {
            break;
        }
        buf.push(b[0]);
        bytes_read += 1;
    }
    if bytes_read >= MAX_RESPONSE_SIZE {
        tracing::warn!(
            "Serial response truncated at {} bytes (limit)",
            MAX_RESPONSE_SIZE
        );
    }
    let line_str = String::from_utf8_lossy(&buf);
    let resp: Value = serde_json::from_str(line_str.trim())?;
    let resp_id = resp["id"].as_str().unwrap_or("");
    if resp_id != id_str {
        anyhow::bail!("Response id mismatch: expected {}, got {}", id_str, resp_id);
    }
    Ok(resp)
}

pub(crate) struct SerialTransport {
    port: Mutex<SerialStream>,
}

const SERIAL_TIMEOUT_SECS: u64 = 5;

impl SerialTransport {
    async fn request(&self, cmd: &str, args: Value) -> anyhow::Result<ToolResult> {
        let mut port = self.port.lock().await;
        let resp = tokio::time::timeout(
            std::time::Duration::from_secs(SERIAL_TIMEOUT_SECS),
            send_request(&mut port, cmd, args),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!("Serial request timed out after {}s", SERIAL_TIMEOUT_SECS)
        })??;

        let ok = resp["ok"].as_bool().unwrap_or(false);
        let result = resp["result"]
            .as_str()
            .map(String::from)
            .unwrap_or_else(|| resp["result"].to_string());
        let error = resp["error"].as_str().map(String::from);

        Ok(ToolResult {
            success: ok,
            output: result,
            error,
        })
    }

    pub async fn capabilities(&self) -> anyhow::Result<ToolResult> {
        self.request("capabilities", json!({})).await
    }
}

pub struct SerialPeripheral {
    name: String,
    board_type: String,
    transport: Arc<SerialTransport>,
}

impl SerialPeripheral {

    #[allow(clippy::unused_async)]
    pub async fn connect(config: &PeripheralBoardConfig) -> anyhow::Result<Self> {
        let path = config
            .path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Serial peripheral requires path"))?;

        if !is_path_allowed(path) {
            anyhow::bail!(
                "Serial path not allowed: {}. Allowed: /dev/ttyACM*, /dev/ttyUSB*, /dev/tty.usbmodem*, /dev/cu.usbmodem*",
                path
            );
        }

        let port = tokio_serial::new(path, config.baud)
            .open_native_async()
            .map_err(|e| anyhow::anyhow!("Failed to open {}: {}", path, e))?;

        let name = format!("{}-{}", config.board, path.replace('/', "_"));
        let transport = Arc::new(SerialTransport {
            port: Mutex::new(port),
        });

        Ok(Self {
            name: name.clone(),
            board_type: config.board.clone(),
            transport,
        })
    }
}

#[async_trait]
impl Peripheral for SerialPeripheral {
    fn name(&self) -> &str {
        &self.name
    }

    fn board_type(&self) -> &str {
        &self.board_type
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn health_check(&self) -> bool {
        self.transport
            .request("ping", json!({}))
            .await
            .map(|r| r.success)
            .unwrap_or(false)
    }

    fn tools(&self) -> Vec<Box<dyn Tool>> {
        vec![
            Box::new(GpioReadTool {
                transport: self.transport.clone(),
            }),
            Box::new(GpioWriteTool {
                transport: self.transport.clone(),
            }),
        ]
    }
}

impl SerialPeripheral {

    pub(crate) fn transport(&self) -> Arc<SerialTransport> {
        self.transport.clone()
    }
}

struct GpioReadTool {
    transport: Arc<SerialTransport>,
}

#[async_trait]
impl Tool for GpioReadTool {
    fn name(&self) -> &str {
        "gpio_read"
    }

    fn description(&self) -> &str {
        "Read the value (0 or 1) of a GPIO pin on a connected peripheral (e.g. STM32 Nucleo)"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pin": {
                    "type": "integer",
                    "description": "GPIO pin number (e.g. 13 for LED on Nucleo)"
                }
            },
            "required": ["pin"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let pin = args
            .get("pin")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing 'pin' parameter"))?;
        self.transport
            .request("gpio_read", json!({ "pin": pin }))
            .await
    }
}

struct GpioWriteTool {
    transport: Arc<SerialTransport>,
}

#[async_trait]
impl Tool for GpioWriteTool {
    fn name(&self) -> &str {
        "gpio_write"
    }

    fn description(&self) -> &str {
        "Set a GPIO pin high (1) or low (0) on a connected peripheral (e.g. turn on/off LED)"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pin": {
                    "type": "integer",
                    "description": "GPIO pin number"
                },
                "value": {
                    "type": "integer",
                    "description": "0 for low, 1 for high"
                }
            },
            "required": ["pin", "value"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let pin = args
            .get("pin")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing 'pin' parameter"))?;
        let value = args
            .get("value")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing 'value' parameter"))?;
        if value > 1 {
            return Ok(ToolResult {
                success: false,
                output: format!("Invalid value {}: must be 0 (LOW) or 1 (HIGH)", value),
                error: Some("value must be 0 or 1".into()),
            });
        }
        self.transport
            .request("gpio_write", json!({ "pin": pin, "value": value }))
            .await
    }
}
