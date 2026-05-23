// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::device::{DeviceRegistry, DeviceRuntime};
use crate::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

const MPREMOTE_TIMEOUT_SECS: u64 = 30;

const PORT_WAIT_SECS: u64 = 15;

const PORT_POLL_MS: u64 = 200;

async fn resolve_device_port(
    registry: &RwLock<DeviceRegistry>,
    device_alias: Option<&str>,
) -> Result<(String, String, DeviceRuntime), ToolResult> {
    let reg = registry.read().await;

    let alias: String = match device_alias {
        Some(a) => a.to_string(),
        None => {

            let all_aliases: Vec<String> =
                reg.aliases().into_iter().map(|a| a.to_string()).collect();
            match all_aliases.as_slice() {
                [single] => single.clone(),
                [] => {
                    return Err(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some("no device found — is a board connected via USB?".to_string()),
                    });
                }
                multiple => {
                    return Err(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "multiple devices found ({}); specify the \"device\" parameter",
                            multiple.join(", ")
                        )),
                    });
                }
            }
        }
    };

    let device = reg.get_device(&alias).ok_or_else(|| ToolResult {
        success: false,
        output: String::new(),
        error: Some(format!("device '{alias}' not found in registry")),
    })?;

    let runtime = device.runtime;

    let port = device.port().ok_or_else(|| ToolResult {
        success: false,
        output: String::new(),
        error: Some(format!(
            "device '{alias}' has no serial port — is it connected?"
        )),
    })?;

    Ok((alias, port.to_string(), runtime))
}

fn unsupported_runtime(runtime: &DeviceRuntime, tool: &str) -> ToolResult {
    ToolResult {
        success: false,
        output: String::new(),
        error: Some(format!(
            "{runtime} runtime is not yet supported for {tool} — coming soon"
        )),
    }
}

async fn run_mpremote(args: &[&str], timeout_secs: u64) -> Result<(String, String), String> {
    use tokio::time::timeout;

    let result = timeout(
        std::time::Duration::from_secs(timeout_secs),
        crate::util::hidden_async_command("mpremote").args(args).output(),
    )
    .await;

    match result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            if output.status.success() {
                Ok((stdout, stderr))
            } else {
                Err(format!(
                    "mpremote failed (exit {}): {}",
                    output.status,
                    stderr.trim()
                ))
            }
        }
        Ok(Err(e)) => Err(format!(
            "mpremote not found or could not start ({e}). \
             Install it with: pip install mpremote"
        )),
        Err(_) => Err(format!(
            "mpremote timed out after {timeout_secs}s — \
             the device may be unresponsive"
        )),
    }
}

pub struct DeviceReadCodeTool {
    registry: Arc<RwLock<DeviceRegistry>>,
}

impl DeviceReadCodeTool {
    pub fn new(registry: Arc<RwLock<DeviceRegistry>>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Tool for DeviceReadCodeTool {
    fn name(&self) -> &str {
        "device_read_code"
    }

    fn description(&self) -> &str {
        "Read the current program (main.py) running on a connected device. \
         Use this before writing new code so you understand the current state."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "device": {
                    "type": "string",
                    "description": "Device alias e.g. pico0, esp0. Auto-selected if only one device is connected."
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let device_alias = args.get("device").and_then(|v| v.as_str());

        let (alias, port, runtime) = match resolve_device_port(&self.registry, device_alias).await {
            Ok(v) => v,
            Err(tool_result) => return Ok(tool_result),
        };

        match runtime {
            DeviceRuntime::MicroPython | DeviceRuntime::CircuitPython => {}
            other => return Ok(unsupported_runtime(&other, "device_read_code")),
        }

        tracing::info!(alias = %alias, port = %port, runtime = %runtime, "reading main.py from device");

        match run_mpremote(
            &["connect", &port, "cat", ":main.py"],
            MPREMOTE_TIMEOUT_SECS,
        )
        .await
        {
            Ok((stdout, _stderr)) => Ok(ToolResult {
                success: true,
                output: if stdout.trim().is_empty() {
                    format!("main.py on {alias} is empty or not found.")
                } else {
                    format!(
                        "Current main.py on {alias}:\n\n```python\n{}\n```",
                        stdout.trim()
                    )
                },
                error: None,
            }),
            Err(e) => {

                if e.contains("OSError") || e.contains("no such file") || e.contains("ENOENT") {
                    Ok(ToolResult {
                        success: true,
                        output: format!(
                            "No main.py found on {alias} — the device has no program yet."
                        ),
                        error: None,
                    })
                } else {
                    Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Failed to read code from {alias}: {e}")),
                    })
                }
            }
        }
    }
}

pub struct DeviceWriteCodeTool {
    registry: Arc<RwLock<DeviceRegistry>>,
}

impl DeviceWriteCodeTool {
    pub fn new(registry: Arc<RwLock<DeviceRegistry>>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Tool for DeviceWriteCodeTool {
    fn name(&self) -> &str {
        "device_write_code"
    }

    fn description(&self) -> &str {
        "Write a complete program to a device — replaces main.py and restarts the device. \
         Always read the current code first with device_read_code."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "device": {
                    "type": "string",
                    "description": "Device alias e.g. pico0, esp0. Auto-selected if only one device is connected."
                },
                "code": {
                    "type": "string",
                    "description": "Complete program to write as main.py"
                }
            },
            "required": ["code"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let code = match args.get("code").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("missing required parameter: code".to_string()),
                });
            }
        };

        if code.trim().is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("code parameter is empty — provide a program to write".to_string()),
            });
        }

        let device_alias = args.get("device").and_then(|v| v.as_str());

        let (alias, port, runtime) = match resolve_device_port(&self.registry, device_alias).await {
            Ok(v) => v,
            Err(tool_result) => return Ok(tool_result),
        };

        match runtime {
            DeviceRuntime::MicroPython | DeviceRuntime::CircuitPython => {}
            other => return Ok(unsupported_runtime(&other, "device_write_code")),
        }

        tracing::info!(alias = %alias, port = %port, runtime = %runtime, code_len = code.len(), "writing main.py to device");

        let named_tmp = match tokio::task::spawn_blocking(|| {
            tempfile::Builder::new()
                .prefix("sen_main_")
                .suffix(".py")
                .tempfile()
        })
        .await
        {
            Ok(Ok(f)) => f,
            Ok(Err(e)) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("failed to create temp file: {e}")),
                });
            }
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("temp file task failed: {e}")),
                });
            }
        };
        let tmp_path = named_tmp.path().to_path_buf();
        let tmp_str = tmp_path.to_string_lossy().to_string();

        if let Err(e) = tokio::fs::write(&tmp_path, code).await {

            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("failed to write temp file: {e}")),
            });
        }

        let result = run_mpremote(
            &["connect", &port, "cp", &tmp_str, ":main.py", "+", "reset"],
            MPREMOTE_TIMEOUT_SECS,
        )
        .await;

        if let Err(e) = named_tmp.close() {
            tracing::warn!(path = %tmp_str, err = %e, "failed to clean up temp file");
        }

        match result {
            Ok((_stdout, _stderr)) => {
                tracing::info!(alias = %alias, "main.py deployed and device reset");

                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let port_reappeared = wait_for_port(
                    &port,
                    std::time::Duration::from_secs(PORT_WAIT_SECS),
                    std::time::Duration::from_millis(PORT_POLL_MS),
                )
                .await;

                if port_reappeared {
                    Ok(ToolResult {
                        success: true,
                        output: format!(
                            "Code deployed to {alias} — main.py updated and device reset. \
                             {alias} is back online."
                        ),
                        error: None,
                    })
                } else {
                    Ok(ToolResult {
                        success: true,
                        output: format!(
                            "Code deployed to {alias} — main.py updated and device reset. \
                             Note: serial port did not reappear within {PORT_WAIT_SECS}s; \
                             the device may still be booting."
                        ),
                        error: None,
                    })
                }
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to deploy code to {alias}: {e}")),
            }),
        }
    }
}

pub struct DeviceExecTool {
    registry: Arc<RwLock<DeviceRegistry>>,
}

impl DeviceExecTool {
    pub fn new(registry: Arc<RwLock<DeviceRegistry>>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Tool for DeviceExecTool {
    fn name(&self) -> &str {
        "device_exec"
    }

    fn description(&self) -> &str {
        "Execute a code snippet on a connected device without modifying main.py. \
         Good for one-time actions, sensor reads, and testing before writing permanent code."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "device": {
                    "type": "string",
                    "description": "Device alias e.g. pico0, esp0. Auto-selected if only one device is connected."
                },
                "code": {
                    "type": "string",
                    "description": "Code to execute. Output is captured and returned."
                }
            },
            "required": ["code"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let code = match args.get("code").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("missing required parameter: code".to_string()),
                });
            }
        };

        if code.trim().is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    "code parameter is empty — provide a code snippet to execute".to_string(),
                ),
            });
        }

        let device_alias = args.get("device").and_then(|v| v.as_str());

        let (alias, port, runtime) = match resolve_device_port(&self.registry, device_alias).await {
            Ok(v) => v,
            Err(tool_result) => return Ok(tool_result),
        };

        match runtime {
            DeviceRuntime::MicroPython | DeviceRuntime::CircuitPython => {}
            other => return Ok(unsupported_runtime(&other, "device_exec")),
        }

        tracing::info!(alias = %alias, port = %port, runtime = %runtime, code_len = code.len(), "executing snippet on device");

        let named_tmp = match tokio::task::spawn_blocking(|| {
            tempfile::Builder::new()
                .prefix("sen_exec_")
                .suffix(".py")
                .tempfile()
        })
        .await
        {
            Ok(Ok(f)) => f,
            Ok(Err(e)) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("failed to create temp file: {e}")),
                });
            }
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("temp file task failed: {e}")),
                });
            }
        };
        let tmp_path = named_tmp.path().to_path_buf();
        let tmp_str = tmp_path.to_string_lossy().to_string();

        if let Err(e) = tokio::fs::write(&tmp_path, code).await {

            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("failed to write temp file: {e}")),
            });
        }

        let result =
            run_mpremote(&["connect", &port, "run", &tmp_str], MPREMOTE_TIMEOUT_SECS).await;

        if let Err(e) = named_tmp.close() {
            tracing::warn!(path = %tmp_str, err = %e, "failed to clean up temp file");
        }

        match result {
            Ok((stdout, stderr)) => {
                let output = if stdout.trim().is_empty() && !stderr.trim().is_empty() {

                    stderr.trim().to_string()
                } else {
                    stdout.trim().to_string()
                };

                Ok(ToolResult {
                    success: true,
                    output: if output.is_empty() {
                        format!("Code executed on {alias} — no output produced.")
                    } else {
                        format!("Output from {alias}:\n{output}")
                    },
                    error: None,
                })
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to execute code on {alias}: {e}")),
            }),
        }
    }
}

async fn wait_for_port(
    port_path: &str,
    timeout: std::time::Duration,
    interval: std::time::Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if std::path::Path::new(port_path).exists() {
            return true;
        }
        tokio::time::sleep(interval).await;
    }
    false
}

pub fn device_code_tools(registry: Arc<RwLock<DeviceRegistry>>) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(DeviceReadCodeTool::new(registry.clone())),
        Box::new(DeviceWriteCodeTool::new(registry.clone())),
        Box::new(DeviceExecTool::new(registry)),
    ]
}
