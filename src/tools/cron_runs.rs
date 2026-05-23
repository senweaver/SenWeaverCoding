// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use crate::config::Config;
use crate::cron;
use async_trait::async_trait;
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;

const MAX_RUN_OUTPUT_CHARS: usize = 500;

pub struct CronRunsTool {
    config: Arc<Config>,
}

impl CronRunsTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[derive(Serialize)]
struct RunView {
    id: i64,
    job_id: String,
    started_at: chrono::DateTime<chrono::Utc>,
    finished_at: chrono::DateTime<chrono::Utc>,
    status: String,
    output: Option<String>,
    duration_ms: Option<i64>,
}

#[async_trait]
impl Tool for CronRunsTool {
    fn name(&self) -> &str {
        "cron_runs"
    }

    fn description(&self) -> &str {
        "List recent run history for a cron job"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "job_id": { "type": "string" },
                "limit": { "type": "integer" }
            },
            "required": ["job_id"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        if !self.config.cron.enabled {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("cron is disabled by config (cron.enabled=false)".to_string()),
            });
        }

        let job_id = match args.get("job_id").and_then(serde_json::Value::as_str) {
            Some(v) if !v.trim().is_empty() => v,
            _ => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("Missing 'job_id' parameter".to_string()),
                });
            }
        };

        let limit = args
            .get("limit")
            .and_then(serde_json::Value::as_u64)
            .map_or(10, |v| usize::try_from(v).unwrap_or(10));

        match cron::list_runs(&self.config, job_id, limit) {
            Ok(runs) => {
                let runs: Vec<RunView> = runs
                    .into_iter()
                    .map(|run| RunView {
                        id: run.id,
                        job_id: run.job_id,
                        started_at: run.started_at,
                        finished_at: run.finished_at,
                        status: run.status,
                        output: run.output.map(|out| truncate(&out, MAX_RUN_OUTPUT_CHARS)),
                        duration_ms: run.duration_ms,
                    })
                    .collect();

                Ok(ToolResult {
                    success: true,
                    output: serde_json::to_string_pretty(&runs)?,
                    error: None,
                })
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(e.to_string()),
            }),
        }
    }
}

fn truncate(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let mut out: String = input.chars().take(max_chars).collect();
    out.push_str("...");
    out
}
