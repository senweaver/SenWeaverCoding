// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::json;

use super::super::traits::{Tool, ToolResult};

pub struct BackgroundWaitTool;

impl BackgroundWaitTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BackgroundWaitTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for BackgroundWaitTool {
    fn name(&self) -> &str {
        "background_wait"
    }

    fn description(&self) -> &str {
        "Block until a background shell (bg-<id>) exits, or its output matches a regex, or a \
         timeout elapses — instead of polling background_status in a loop. Returns the final \
         status and a tail of the output. Use this after starting a long task with shell \
         background:true when your next step depends on its result."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The bg-<id> handle returned by shell background:true."
                },
                "pattern": {
                    "type": "string",
                    "description": "Optional regex; return as soon as the shell output matches it (even while still running)."
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Maximum seconds to wait (default 120, max 1800)."
                }
            },
            "required": ["id"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let id = args
            .get("id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Missing 'id' parameter"))?;
        let timeout = Duration::from_secs(
            args.get("timeout_secs")
                .and_then(|v| v.as_u64())
                .map(|n| n.clamp(1, 1800))
                .unwrap_or(120),
        );
        let regex = match args.get("pattern").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            Some(p) => match regex::Regex::new(p) {
                Ok(re) => Some(re),
                Err(e) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("invalid pattern regex: {e}")),
                    });
                }
            },
            None => None,
        };

        // Confirm the shell exists in this session up front.
        if super::registry::logs_for(id, 1).is_none() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "No background shell '{id}' in this session (it may have exited and been evicted)."
                )),
            });
        }

        let started = Instant::now();
        let poll = Duration::from_millis(250);
        loop {
            let Some((snap, logs)) = super::registry::logs_for(id, 200) else {
                return Ok(ToolResult {
                    success: true,
                    output: format!("Background shell '{id}' is no longer tracked (exited)."),
                    error: None,
                });
            };
            if let Some(re) = regex.as_ref() {
                if re.is_match(&logs) {
                    return Ok(ToolResult {
                        success: true,
                        output: format!(
                            "matched pattern after {}s (running={})\n---\n{}",
                            started.elapsed().as_secs(),
                            snap.running,
                            tail(&logs)
                        ),
                        error: None,
                    });
                }
            }
            if !snap.running {
                return Ok(ToolResult {
                    success: true,
                    output: format!(
                        "background shell '{id}' exited (code={:?}) after {}s\n---\n{}",
                        snap.exit_code,
                        snap.elapsed_secs,
                        tail(&logs)
                    ),
                    error: None,
                });
            }
            if started.elapsed() >= timeout {
                return Ok(ToolResult {
                    success: false,
                    output: format!(
                        "timed out after {}s waiting for '{id}' (still running)\n---\n{}",
                        timeout.as_secs(),
                        tail(&logs)
                    ),
                    error: Some("background_wait timed out".to_string()),
                });
            }
            tokio::time::sleep(poll).await;
        }
    }
}

fn tail(logs: &str) -> String {
    const MAX: usize = 4000;
    if logs.len() <= MAX {
        return logs.to_string();
    }
    let start = logs.len() - MAX;
    let mut idx = start;
    while idx < logs.len() && !logs.is_char_boundary(idx) {
        idx += 1;
    }
    format!("...(truncated)...\n{}", &logs[idx..])
}
