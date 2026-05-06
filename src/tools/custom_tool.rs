// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Script-wrapped user-defined tool.
//!
//! Each entry in `[[custom_tools.tools]]` becomes a `CustomTool` registered
//! into the global tool registry under `custom_<name>`. The tool spawns the
//! configured `command` with templated arguments, the JSON payload on stdin,
//! and the merged environment, then returns a structured `ToolResult`.
//!
//! Argument templating supports two placeholder forms inside `args`:
//!
//! * `{json}` — replaced by the full JSON arguments (compact form).
//! * `{<key>}` — replaced by `args.<key>` rendered as a string. Numeric
//!   and boolean values are stringified naturally; objects/arrays are
//!   rendered as compact JSON; missing keys resolve to an empty string.
//!
//! The raw JSON args are also written to the spawned process's stdin and
//! exposed as `SEN_TOOL_ARGS` so the script can choose the form it likes.

use super::traits::{Tool, ToolResult};
use crate::config::CustomToolDef;
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const MAX_OUTPUT_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone)]
pub struct CustomTool {
    name: String,
    description: String,
    command: String,
    args_template: Vec<String>,
    cwd: Option<PathBuf>,
    env: HashMap<String, String>,
    timeout_secs: u64,
    schema: Value,
    workspace_root: Arc<RwLock<PathBuf>>,
}

impl CustomTool {
    pub fn from_def(def: &CustomToolDef, workspace_root: Arc<RwLock<PathBuf>>) -> Self {
        let registered_name = format!("custom_{}", def.name.trim());
        let schema = if def.schema.is_object() {
            def.schema.clone()
        } else {
            serde_json::json!({ "type": "object" })
        };
        Self {
            name: registered_name,
            description: def.description.clone(),
            command: def.command.clone(),
            args_template: def.args.clone(),
            cwd: def
                .cwd
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
            env: def.env.clone(),
            timeout_secs: def.timeout_secs.max(1),
            schema,
            workspace_root,
        }
    }

    fn workspace_snapshot(&self) -> PathBuf {
        self.workspace_root.read().clone()
    }

    fn resolve_cwd(&self) -> PathBuf {
        let base = self.workspace_snapshot();
        match &self.cwd {
            Some(p) if p.is_absolute() => p.clone(),
            Some(p) => base.join(p),
            None => base,
        }
    }

    fn substitute_arg(template: &str, args: &Value, json_payload: &str) -> String {
        if !template.contains('{') {
            return template.to_string();
        }
        let mut out = String::with_capacity(template.len());
        let mut buf = template.chars().peekable();
        while let Some(ch) = buf.next() {
            if ch == '{' {
                let mut placeholder = String::new();
                let mut closed = false;
                while let Some(&inner) = buf.peek() {
                    if inner == '}' {
                        buf.next();
                        closed = true;
                        break;
                    }
                    placeholder.push(inner);
                    buf.next();
                }
                if !closed {
                    out.push('{');
                    out.push_str(&placeholder);
                    continue;
                }
                let key = placeholder.trim();
                if key == "json" {
                    out.push_str(json_payload);
                } else if let Some(value) = args.get(key) {
                    out.push_str(&render_scalar(value));
                }
            } else {
                out.push(ch);
            }
        }
        out
    }
}

fn render_scalar(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

#[async_trait]
impl Tool for CustomTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> Value {
        self.schema.clone()
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let json_payload = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());

        let resolved_args: Vec<String> = self
            .args_template
            .iter()
            .map(|tpl| Self::substitute_arg(tpl, &args, &json_payload))
            .collect();

        let mut command = tokio::process::Command::new(&self.command);
        command
            .args(&resolved_args)
            .current_dir(self.resolve_cwd())
            .env("SEN_TOOL_NAME", &self.name)
            .env("SEN_TOOL_ARGS", &json_payload)
            .env(
                "SEN_WORKSPACE_DIR",
                self.workspace_snapshot().display().to_string(),
            )
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        for (key, value) in &self.env {
            if !key.trim().is_empty() {
                command.env(key, value);
            }
        }

        #[cfg(target_os = "windows")]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = match command.spawn() {
            Ok(c) => c,
            Err(err) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "failed to spawn '{}': {err}",
                        self.command
                    )),
                });
            }
        };

        if let Some(mut stdin) = child.stdin.take() {
            let payload = json_payload.clone();
            tokio::spawn(async move {
                let _ = stdin.write_all(payload.as_bytes()).await;
                let _ = stdin.shutdown().await;
            });
        }

        let timeout = Duration::from_secs(self.timeout_secs);
        let result = tokio::time::timeout(timeout, child.wait_with_output()).await;
        let output = match result {
            Ok(Ok(out)) => out,
            Ok(Err(err)) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("tool execution error: {err}")),
                });
            }
            Err(_) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "tool '{}' timed out after {}s",
                        self.name, self.timeout_secs
                    )),
                });
            }
        };

        let stdout = truncate_output(&output.stdout);
        let stderr = truncate_output(&output.stderr);
        let success = output.status.success();

        let mut combined = stdout.clone();
        if !stderr.is_empty() {
            if !combined.is_empty() {
                combined.push_str("\n--- stderr ---\n");
            }
            combined.push_str(&stderr);
        }

        let error = if success {
            None
        } else {
            Some(format!(
                "tool exited with status {}",
                output
                    .status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "<signal>".to_string())
            ))
        };

        Ok(ToolResult {
            success,
            output: combined,
            error,
        })
    }
}

fn truncate_output(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let cap = MAX_OUTPUT_BYTES.min(bytes.len());
    let slice = &bytes[..cap];
    let mut text = String::from_utf8_lossy(slice).into_owned();
    if bytes.len() > cap {
        text.push_str(&format!(
            "\n[...truncated {} bytes]",
            bytes.len() - cap
        ));
    }
    text
}
