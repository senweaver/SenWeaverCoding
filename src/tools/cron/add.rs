// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::super::traits::{Tool, ToolResult};
use crate::config::Config;
use crate::cron::{
    self, DeliveryConfig, JobType, Schedule, SessionTarget, deserialize_maybe_stringified,
};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct CronAddTool {
    config: Arc<Config>,
    security: Arc<SecurityPolicy>,
}

impl CronAddTool {
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
impl Tool for CronAddTool {
    fn name(&self) -> &str {
        "cron_add"
    }

    fn description(&self) -> &str {
        "Create a scheduled cron job (shell or agent) with cron/at/every schedules. \
         Use job_type='agent' with a prompt to run the AI agent on schedule. \
         To deliver output to a channel (Discord, Telegram, Slack, Mattermost, Matrix, QQ), set \
         delivery={\"mode\":\"announce\",\"channel\":\"discord\",\"to\":\"<channel_id_or_chat_id>\"}. \
         This is the preferred tool for sending scheduled/delayed messages to users via channels."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Optional human-readable name for the job"
                },

                "schedule": {
                    "description": "When to run the job. Exactly one of three forms must be used.",
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
                "job_type": {
                    "type": "string",
                    "enum": ["shell", "agent"],
                    "description": "Type of job: 'shell' runs a command, 'agent' runs the AI agent with a prompt"
                },
                "command": {
                    "type": "string",
                    "description": "Shell command to run (required when job_type is 'shell')"
                },
                "prompt": {
                    "type": "string",
                    "description": "Agent prompt to run on schedule (required when job_type is 'agent')"
                },
                "session_target": {
                    "type": "string",
                    "enum": ["isolated", "main"],
                    "description": "Agent session context: 'isolated' starts a fresh session each run, 'main' reuses the primary session"
                },
                "model": {
                    "type": "string",
                    "description": "Optional model override for agent jobs, e.g. 'x-ai/grok-4-1-fast'"
                },
                "allowed_tools": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional allowlist of tool names for agent jobs. When omitted, all tools remain available."
                },
                "folder_path": {
                    "type": "string",
                    "description": "Optional working folder for agent jobs. Absolute, or relative to the workspace directory."
                },
                "use_worktree": {
                    "type": "boolean",
                    "description": "If true, agent jobs run in an isolated git worktree; results are merged back when the parent workspace is clean, otherwise kept on a sen-cron/* branch."
                },
                "delivery": {
                    "type": "object",
                    "description": "Optional delivery config to send job output to a channel after each run. When provided, all three of mode, channel, and to are expected.",
                    "properties": {
                        "mode": {
                            "type": "string",
                            "enum": ["none", "announce"],
                            "description": "'announce' sends output to the specified channel; 'none' disables delivery"
                        },
                        "channel": {
                            "type": "string",
                            "enum": ["telegram", "discord", "slack", "mattermost", "matrix", "qq"],
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
                },
                "delete_after_run": {
                    "type": "boolean",
                    "description": "If true, the job is automatically deleted after its first successful run. Defaults to true for 'at' schedules."
                }
            },
            "required": ["schedule"]
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

        let schedule = match args.get("schedule") {
            Some(v) => match deserialize_maybe_stringified::<Schedule>(v) {
                Ok(schedule) => schedule,
                Err(e) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Invalid schedule: {e}")),
                    });
                }
            },
            None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("Missing 'schedule' parameter".to_string()),
                });
            }
        };

        let name = args
            .get("name")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);

        let job_type = match args.get("job_type").and_then(serde_json::Value::as_str) {
            Some("agent") => JobType::Agent,
            Some("shell") => JobType::Shell,
            Some(other) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Invalid job_type: {other}")),
                });
            }
            None => {
                if args.get("prompt").is_some() {
                    JobType::Agent
                } else {
                    JobType::Shell
                }
            }
        };

        let default_delete_after_run = matches!(schedule, Schedule::At { .. });
        let delete_after_run = args
            .get("delete_after_run")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(default_delete_after_run);
        let approved = crate::agent::loop_::current_tool_runtime_approved();
        let delivery = match args.get("delivery") {
            Some(v) => match serde_json::from_value::<DeliveryConfig>(v.clone()) {
                Ok(cfg) => Some(cfg),
                Err(e) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Invalid delivery config: {e}")),
                    });
                }
            },
            None => None,
        };

        let result = match job_type {
            JobType::Shell => {
                let command = match args.get("command").and_then(serde_json::Value::as_str) {
                    Some(command) if !command.trim().is_empty() => command,
                    _ => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some("Missing 'command' for shell job".to_string()),
                        });
                    }
                };

                if let Err(reason) = self.security.validate_command_execution(command, approved) {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(reason),
                    });
                }

                if let Some(blocked) = self.enforce_mutation_allowed("cron_add") {
                    return Ok(blocked);
                }

                cron::add_shell_job_with_approval(
                    &self.config,
                    name,
                    schedule,
                    command,
                    delivery,
                    approved,
                )
            }
            JobType::Agent => {
                let prompt = match args.get("prompt").and_then(serde_json::Value::as_str) {
                    Some(prompt) if !prompt.trim().is_empty() => prompt,
                    _ => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some("Missing 'prompt' for agent job".to_string()),
                        });
                    }
                };

                let session_target = match args.get("session_target") {
                    Some(v) => match serde_json::from_value::<SessionTarget>(v.clone()) {
                        Ok(target) => target,
                        Err(e) => {
                            return Ok(ToolResult {
                                success: false,
                                output: String::new(),
                                error: Some(format!("Invalid session_target: {e}")),
                            });
                        }
                    },
                    None => SessionTarget::Isolated,
                };

                let model = args
                    .get("model")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string);
                let allowed_tools = match args.get("allowed_tools") {
                    Some(v) => match serde_json::from_value::<Vec<String>>(v.clone()) {
                        Ok(v) => {
                            if v.is_empty() {
                                None
                            } else {
                                Some(v)
                            }
                        }
                        Err(e) => {
                            return Ok(ToolResult {
                                success: false,
                                output: String::new(),
                                error: Some(format!("Invalid allowed_tools: {e}")),
                            });
                        }
                    },
                    None => None,
                };

                if let Some(blocked) = self.enforce_mutation_allowed("cron_add") {
                    return Ok(blocked);
                }

                let folder_path = args
                    .get("folder_path")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                let use_worktree = args
                    .get("use_worktree")
                    .and_then(serde_json::Value::as_bool);

                cron::add_agent_job(
                    &self.config,
                    name,
                    schedule,
                    prompt,
                    cron::AgentJobOptions {
                        session_target,
                        model,
                        delivery,
                        delete_after_run,
                        allowed_tools,
                        folder_path,
                        use_worktree,
                        ..Default::default()
                    },
                )
            }
            JobType::Computer => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(
                        "computer control jobs are managed from the desktop computer mode"
                            .to_string(),
                    ),
                });
            }
        };

        match result {
            Ok(job) => Ok(ToolResult {
                success: true,
                output: serde_json::to_string_pretty(&json!({
                    "id": job.id,
                    "name": job.name,
                    "job_type": job.job_type,
                    "schedule": job.schedule,
                    "next_run": job.next_run,
                    "enabled": job.enabled,
                    "allowed_tools": job.allowed_tools
                }))?,
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
