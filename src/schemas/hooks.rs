// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Hook schemas — mirrors claude-code-typescript-src`schemas/hooks.ts`.
// Defines the schema for hook events that plugins and SDK consumers can register.

use serde::{Deserialize, Serialize};

/// Schema describing a hook event that can be registered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookEventSchema {
    pub event: String,
    pub description: String,
    pub payload_schema: serde_json::Value,
    pub supports_filter: bool,
    pub supports_mutation: bool,
}

/// Collection of all hook event schemas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookSchema {
    pub events: Vec<HookEventSchema>,
}

impl HookSchema {
    /// Build the default hook schema with all supported events.
    pub fn default_schema() -> Self {
        Self {
            events: vec![
                HookEventSchema {
                    event: "pre_tool_use".to_string(),
                    description:
                        "Fired before a tool is executed. Can modify input or block execution."
                            .to_string(),
                    payload_schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "tool_name": { "type": "string" },
                            "tool_input": { "type": "object" },
                            "session_id": { "type": "string" }
                        }
                    }),
                    supports_filter: true,
                    supports_mutation: true,
                },
                HookEventSchema {
                    event: "post_tool_use".to_string(),
                    description: "Fired after a tool has executed. Can modify output.".to_string(),
                    payload_schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "tool_name": { "type": "string" },
                            "tool_input": { "type": "object" },
                            "tool_output": { "type": "string" },
                            "is_error": { "type": "boolean" },
                            "duration_ms": { "type": "number" },
                            "session_id": { "type": "string" }
                        }
                    }),
                    supports_filter: true,
                    supports_mutation: true,
                },
                HookEventSchema {
                    event: "notification".to_string(),
                    description: "Fired when the agent wants to send a notification.".to_string(),
                    payload_schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "title": { "type": "string" },
                            "body": { "type": "string" },
                            "priority": { "type": "string" }
                        }
                    }),
                    supports_filter: false,
                    supports_mutation: false,
                },
                HookEventSchema {
                    event: "stop".to_string(),
                    description: "Fired when the agent's main loop completes a turn.".to_string(),
                    payload_schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "reason": { "type": "string" },
                            "response_text": { "type": "string" },
                            "session_id": { "type": "string" }
                        }
                    }),
                    supports_filter: false,
                    supports_mutation: false,
                },
                HookEventSchema {
                    event: "subagent_stop".to_string(),
                    description: "Fired when a sub-agent completes.".to_string(),
                    payload_schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "agent_id": { "type": "string" },
                            "task_id": { "type": "string" },
                            "result": { "type": "string" },
                            "session_id": { "type": "string" }
                        }
                    }),
                    supports_filter: true,
                    supports_mutation: false,
                },
            ],
        }
    }

    /// Look up an event schema by name.
    pub fn get_event(&self, name: &str) -> Option<&HookEventSchema> {
        self.events.iter().find(|e| e.event == name)
    }
}

/// Validate a hook configuration against the schema.
pub fn validate_hook_config(
    schema: &HookSchema,
    event: &str,
    config: &serde_json::Value,
) -> Result<(), Vec<String>> {
    let event_schema = schema
        .get_event(event)
        .ok_or_else(|| vec![format!("Unknown hook event: {event}")])?;

    let mut errors = Vec::new();

    // Validate that the config is an object
    if !config.is_object() {
        errors.push("Hook config must be an object".to_string());
    }

    // Validate filter usage
    if config.get("filter").is_some() && !event_schema.supports_filter {
        errors.push(format!("Event '{event}' does not support filtering"));
    }

    // Validate mutation usage
    if config.get("mutate").is_some() && !event_schema.supports_mutation {
        errors.push(format!("Event '{event}' does not support mutation"));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
