// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use crate::config::Config;
use crate::cron::{self, CronJobPatch, deserialize_maybe_stringified};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct CronUpdateTool {
    config: Arc<Config>,
    security: Arc<SecurityPolicy>,
}

impl CronUpdateTool {
    pub fn new(config: Arc<Config>, security: Arc<SecurityPolicy>) -> Self {
        Self { config, security }
    }

    fn enforce_mutation_allowed(&self, action: &str) -> Option<ToolResult> {
        if !self.security.can_act() {
            return Some(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Security policy: read-only mode, cannot perform '{action}'"
                )),
            });
        }

        if self.security.is_rate_limited() {
            return Some(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: too many actions in the last hour".to_string()),
            });
        }

        if !self.security.record_action() {
            return Some(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: action budget exhausted".to_string()),
            });
        }

        None
    }
}

#[async_trait]
impl Tool for CronUpdateTool {
    fn name(&self) -> &str {
        "cron_update"
    }

    fn description(&self) -> &str {
        "Patch an existing cron job (schedule, command, prompt, enabled, delivery, model, etc.)"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "job_id": {
                    "type": "string",
                    "description": "ID of the cron job to update, as returned by cron_add or cron_list"
                },
                "patch": {
                    "type": "object",
                    "description": "Fields to update. Only include fields you want to change; omitted fields are left as-is.",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "New human-readable name for the job"
                        },
                        "enabled": {
                            "type": "boolean",
                            "description": "Enable or disable the job without deleting it"
                        },
                        "command": {
                            "type": "string",
                            "description": "New shell command (for shell jobs)"
                        },
                        "prompt": {
                            "type": "string",
                            "description": "New agent prompt (for agent jobs)"
                        },
                        "model": {
                            "type": "string",
                            "description": "Model override for agent jobs, e.g. 'x-ai/grok-4-1-fast'"
                        },
                        "allowed_tools": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Optional replacement allowlist of tool names for agent jobs"
                        },
                        "session_target": {
                            "type": "string",
                            "enum": ["isolated", "main"],
                            "description": "Agent session context: 'isolated' starts fresh each run, 'main' reuses the primary session"
                        },
                        "delete_after_run": {
                            "type": "boolean",
                            "description": "If true, delete the job automatically after its first successful run"
                        },

                        "schedule": {
                            "description": "New schedule for the job. Exactly one of three forms must be used.",
                            "oneOf": [
                                {
                                    "type": "object",
                                    "description": "Cron expression schedule (repeating). Example: {\"kind\":\"cron\",\"expr\":\"0 9 * * 1-5\",\"tz\":\"America/New_York\"}",
                                    "properties": {
                                        "kind": { "type": "string", "enum": ["cron"] },
                                        "expr": { "type": "string", "description": "Standard 5-field cron expression, e.g. '*/5 * * * *'" },
                                        "tz": { "type": "string", "description": "Optional IANA timezone name, e.g. 'America/New_York'. Defaults to UTC." }
                                    },
                                    "required": ["kind", "expr"]
                                },
                                {
                                    "type": "object",
                                    "description": "One-shot schedule at a specific UTC datetime. Example: {\"kind\":\"at\",\"at\":\"2025-12-31T23:59:00Z\"}",
                                    "properties": {
                                        "kind": { "type": "string", "enum": ["at"] },
                                        "at": { "type": "string", "description": "ISO 8601 UTC datetime string, e.g. '2025-12-31T23:59:00Z'" }
                                    },
                                    "required": ["kind", "at"]
                                },
                                {
                                    "type": "object",
                                    "description": "Repeating interval schedule in milliseconds. Example: {\"kind\":\"every\",\"every_ms\":3600000} runs every hour.",
                                    "properties": {
                                        "kind": { "type": "string", "enum": ["every"] },
                                        "every_ms": { "type": "integer", "description": "Interval in milliseconds, e.g. 3600000 for every hour" }
                                    },
                                    "required": ["kind", "every_ms"]
                                }
                            ]
                        },
                        "delivery": {
                            "type": "object",
                            "description": "Delivery config to send job output to a channel after each run. When provided, mode, channel, and to are all expected.",
                            "properties": {
                                "mode": {
                                    "type": "string",
                                    "enum": ["none", "announce"],
                                    "description": "'announce' sends output to the specified channel; 'none' disables delivery"
                                },
                                "channel": {
                                    "type": "string",
                                    "enum": ["telegram", "discord", "slack", "mattermost", "matrix"],
                                    "description": "Channel type to deliver output to"
                                },
                                "to": {
                                    "type": "string",
                                    "description": "Destination ID: Discord channel ID, Telegram chat ID, Slack channel name, etc."
                                },
                                "best_effort": {
                                    "type": "boolean",
                                    "description": "If true, a delivery failure does not fail the job itself. Defaults to true."
                                }
                            }
                        }
                    }
                },
                "approved": {
                    "type": "boolean",
                    "description": "Set true to explicitly approve medium/high-risk shell commands in supervised mode",
                    "default": false
                }
            },
            "required": ["job_id", "patch"]
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

        let patch_val = match args.get("patch") {
            Some(v) => v.clone(),
            None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("Missing 'patch' parameter".to_string()),
                });
            }
        };

        let patch = match deserialize_maybe_stringified::<CronJobPatch>(&patch_val) {
            Ok(patch) => patch,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Invalid patch payload: {e}")),
                });
            }
        };
        let approved = args
            .get("approved")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        if let Some(blocked) = self.enforce_mutation_allowed("cron_update") {
            return Ok(blocked);
        }

        match cron::update_shell_job_with_approval(&self.config, job_id, patch, approved) {
            Ok(job) => Ok(ToolResult {
                success: true,
                output: serde_json::to_string_pretty(&job)?,
                error: None,
            }),
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(e.to_string()),
            }),
        }
    }
}
