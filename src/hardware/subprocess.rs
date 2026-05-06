// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! SubprocessTool — wraps any external binary as a [`Tool`].
//!
//! Plugins do not need to be written in Rust. Any executable that follows the
//! SenWeaverCoding subprocess protocol is a valid tool:
//!
//! **Protocol (stdin/stdout, one line each):**
//! ```text
//! Host → binary stdin:  {"device":"pico0","pin":5}\n
//! Binary → stdout:      {"success":true,"output":"done","error":null}\n
//! ```
//!
//! Error protocol:
//! - **Timeout (10 s)** — process is killed; `ToolResult::error` contains timeout message.
//! - **Non-zero exit** — process is killed; `ToolResult::error` contains stderr.
//! - **Empty / unparseable stdout** — `ToolResult::error` describes the failure.
//!
//! The schema advertised to the LLM is auto-generated from [`ToolManifest::parameters`].

use super::manifest::ToolManifest;
use crate::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time::{Duration, timeout};

const SUBPROCESS_TIMEOUT_SECS: u64 = 10;

const PROCESS_EXIT_TIMEOUT_SECS: u64 = 5;

pub struct SubprocessTool {

    manifest: ToolManifest,

    binary_path: PathBuf,
}

impl SubprocessTool {

    pub fn new(manifest: ToolManifest, binary_path: PathBuf) -> Self {
        Self {
            manifest,
            binary_path,
        }
    }

    fn build_schema_properties(
        &self,
    ) -> (
        serde_json::Map<String, serde_json::Value>,
        Vec<serde_json::Value>,
    ) {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        for param in &self.manifest.parameters {
            let mut prop = json!({
                "type": param.r#type,
                "description": param.description,
            });

            if let Some(default) = &param.default {
                prop["default"] = default.clone();
            }

            properties.insert(param.name.clone(), prop);

            if param.required {
                required.push(serde_json::Value::String(param.name.clone()));
            }
        }

        (properties, required)
    }
}

#[async_trait]
impl Tool for SubprocessTool {
    fn name(&self) -> &str {
        &self.manifest.tool.name
    }

    fn description(&self) -> &str {
        &self.manifest.tool.description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        let (properties, required) = self.build_schema_properties();
        json!({
            "type": "object",
            "properties": properties,
            "required": required,
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let args_json = serde_json::to_string(&args)
            .map_err(|e| anyhow::anyhow!("failed to serialise args: {}", e))?;

        let mut child = Command::new(&self.binary_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                anyhow::anyhow!(
                    "failed to spawn plugin '{}' at {}: {}",
                    self.manifest.tool.name,
                    self.binary_path.display(),
                    e
                )
            })?;

        if let Some(mut stdin) = child.stdin.take() {
            let write_result = async {
                stdin.write_all(args_json.as_bytes()).await?;
                stdin.write_all(b"\n").await?;
                Ok::<(), std::io::Error>(())
            }
            .await;
            if let Err(e) = write_result {
                if e.kind() != std::io::ErrorKind::BrokenPipe {
                    let _ = child.kill().await;
                    return Err(anyhow::anyhow!(
                        "failed to write args to plugin '{}' stdin: {}",
                        self.manifest.tool.name,
                        e
                    ));
                }
            }

        }

        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();

        let read_result = match stdout_handle {
            None => {

                let _ = child.kill().await;
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "plugin '{}': could not attach stdout pipe",
                        self.manifest.tool.name
                    )),
                });
            }
            Some(stdout) => {
                let mut reader = BufReader::new(stdout);
                let mut line = String::new();
                timeout(
                    Duration::from_secs(SUBPROCESS_TIMEOUT_SECS),
                    reader.read_line(&mut line),
                )
                .await
                .map(|inner| inner.map(|_| line))
            }
        };

        match read_result {

            Err(_elapsed) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                let stderr_msg = collect_stderr(stderr_handle).await;
                Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "plugin '{}' timed out after {}s{}",
                        self.manifest.tool.name,
                        SUBPROCESS_TIMEOUT_SECS,
                        if stderr_msg.is_empty() {
                            String::new()
                        } else {
                            format!("; stderr: {}", stderr_msg)
                        }
                    )),
                })
            }

            Ok(Err(io_err)) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                let stderr_msg = collect_stderr(stderr_handle).await;
                Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "plugin '{}': I/O error reading stdout: {}{}",
                        self.manifest.tool.name,
                        io_err,
                        if stderr_msg.is_empty() {
                            String::new()
                        } else {
                            format!("; stderr: {}", stderr_msg)
                        }
                    )),
                })
            }

            Ok(Ok(line)) => {
                let child_status =
                    timeout(Duration::from_secs(PROCESS_EXIT_TIMEOUT_SECS), child.wait())
                        .await
                        .ok()
                        .and_then(|r| r.ok());
                let stderr_msg = collect_stderr(stderr_handle).await;
                let line = line.trim();

                if line.is_empty() {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "plugin '{}': empty stdout{}",
                            self.manifest.tool.name,
                            if stderr_msg.is_empty() {
                                String::new()
                            } else {
                                format!("; stderr: {}", stderr_msg)
                            }
                        )),
                    });
                }

                match serde_json::from_str::<ToolResult>(line) {
                    Ok(result) => {

                        if let Some(status) = child_status {
                            if !status.success() {
                                return Ok(ToolResult {
                                    success: false,
                                    output: String::new(),
                                    error: Some(format!(
                                        "plugin '{}' exited with {}{}",
                                        self.manifest.tool.name,
                                        status,
                                        if stderr_msg.is_empty() {
                                            String::new()
                                        } else {
                                            format!("; stderr: {}", stderr_msg)
                                        }
                                    )),
                                });
                            }
                        }
                        Ok(result)
                    }
                    Err(parse_err) => Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "plugin '{}': failed to parse output as ToolResult: {} (got: {:?})",
                            self.manifest.tool.name,
                            parse_err,

                            if line.chars().count() > 200 {
                                let truncated: String = line.chars().take(200).collect();
                                format!("{}...", truncated)
                            } else {
                                line.to_string()
                            }
                        )),
                    }),
                }
            }
        }
    }
}

async fn collect_stderr(handle: Option<tokio::process::ChildStderr>) -> String {
    use tokio::io::AsyncReadExt;
    let Some(mut stderr) = handle else {
        return String::new();
    };
    let mut buf = vec![0u8; 512];
    match stderr.read(&mut buf).await {
        Ok(n) if n > 0 => String::from_utf8_lossy(&buf[..n]).trim().to_string(),
        _ => String::new(),
    }
}
