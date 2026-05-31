// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use crate::config::GoogleWorkspaceAllowedOperation;
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

const MAX_OUTPUT_BYTES: usize = 1_048_576;

use crate::config::DEFAULT_GWS_SERVICES;

#[allow(dead_code)]
pub struct GoogleWorkspaceTool {
    security: Arc<SecurityPolicy>,
    allowed_services: Vec<String>,
    allowed_operations: Vec<GoogleWorkspaceAllowedOperation>,
    credentials_path: Option<String>,
    default_account: Option<String>,
    rate_limit_per_minute: u32,
    timeout_secs: u64,
    audit_log: bool,
}

impl GoogleWorkspaceTool {

    pub fn new(
        security: Arc<SecurityPolicy>,
        allowed_services: Vec<String>,
        allowed_operations: Vec<GoogleWorkspaceAllowedOperation>,
        credentials_path: Option<String>,
        default_account: Option<String>,
        rate_limit_per_minute: u32,
        timeout_secs: u64,
        audit_log: bool,
    ) -> Self {
        let services = if allowed_services.is_empty() {
            DEFAULT_GWS_SERVICES
                .iter()
                .map(|s| (*s).to_string())
                .collect()
        } else {
            allowed_services
                .into_iter()
                .map(|s| s.trim().to_string())
                .collect()
        };

        let operations = allowed_operations
            .into_iter()
            .map(|op| GoogleWorkspaceAllowedOperation {
                service: op.service.trim().to_string(),
                resource: op.resource.trim().to_string(),
                sub_resource: op.sub_resource.as_deref().map(|s| s.trim().to_string()),
                methods: op.methods.iter().map(|m| m.trim().to_string()).collect(),
            })
            .collect();
        Self {
            security,
            allowed_services: services,
            allowed_operations: operations,
            credentials_path,
            default_account,
            rate_limit_per_minute,
            timeout_secs,
            audit_log,
        }
    }

    fn positional_cmd_args(
        service: &str,
        resource: &str,
        sub_resource: Option<&str>,
        method: &str,
    ) -> Vec<String> {
        let mut args = vec![service.to_string(), resource.to_string()];
        if let Some(sub) = sub_resource {
            args.push(sub.to_string());
        }
        args.push(method.to_string());
        args
    }

    fn build_pagination_args(page_all: bool, page_limit: Option<u64>) -> Vec<String> {
        let mut args = Vec::new();
        if page_all {
            args.push("--page-all".into());
        }
        if page_all || page_limit.is_some() {
            args.push("--page-limit".into());
            args.push(page_limit.unwrap_or(10).to_string());
        }
        args
    }

    fn is_operation_allowed(
        &self,
        service: &str,
        resource: &str,
        sub_resource: Option<&str>,
        method: &str,
    ) -> bool {
        if self.allowed_operations.is_empty() {
            return true;
        }
        self.allowed_operations.iter().any(|operation| {
            operation.service == service
                && operation.resource == resource
                && operation.sub_resource.as_deref() == sub_resource
                && operation.methods.iter().any(|allowed| allowed == method)
        })
    }
}

#[async_trait]
impl Tool for GoogleWorkspaceTool {
    fn name(&self) -> &str {
        "google_workspace"
    }

    fn description(&self) -> &str {
        "Interact with Google Workspace services (Drive, Gmail, Calendar, Sheets, Docs, etc.) \
         via the gws CLI. Requires gws to be installed and authenticated."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "service": {
                    "type": "string",
                    "description": "Google Workspace service (e.g. drive, gmail, calendar, sheets, docs, slides, tasks, people, chat, classroom, forms, keep, meet, events)"
                },
                "resource": {
                    "type": "string",
                    "description": "Service resource (e.g. files, messages, events, spreadsheets)"
                },
                "method": {
                    "type": "string",
                    "description": "Method to call on the resource (e.g. list, get, create, update, delete)"
                },
                "sub_resource": {
                    "type": "string",
                    "description": "Optional sub-resource for nested operations"
                },
                "params": {
                    "type": "object",
                    "description": "URL/query parameters as key-value pairs (passed as --params JSON)"
                },
                "body": {
                    "type": "object",
                    "description": "Request body for POST/PATCH/PUT operations (passed as --json JSON)"
                },
                "format": {
                    "type": "string",
                    "enum": ["json", "table", "yaml", "csv"],
                    "description": "Output format (default: json)"
                },
                "page_all": {
                    "type": "boolean",
                    "description": "Auto-paginate through all results"
                },
                "page_limit": {
                    "type": "integer",
                    "description": "Max pages to fetch when using page_all (default: 10)"
                }
            },
            "required": ["service", "resource", "method"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let service = args
            .get("service")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'service' parameter"))?;
        let resource = args
            .get("resource")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'resource' parameter"))?;
        let method = args
            .get("method")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'method' parameter"))?;

        let sub_resource: Option<&str> = if let Some(sub_resource_value) = args.get("sub_resource")
        {
            let s = match sub_resource_value.as_str() {
                Some(s) => s,
                None => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some("'sub_resource' must be a string".into()),
                    });
                }
            };
            if !s
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
            {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(
                        "Invalid characters in 'sub_resource': only lowercase alphanumeric, underscore, and hyphen are allowed"
                            .into(),
                    ),
                });
            }
            Some(s)
        } else {
            None
        };

        if self.security.is_rate_limited() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: too many actions in the last hour".into()),
            });
        }

        if !self.allowed_services.iter().any(|s| s == service) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Service '{service}' is not in the allowed services list. \
                     Allowed: {}",
                    self.allowed_services.join(", ")
                )),
            });
        }

        if !self.is_operation_allowed(service, resource, sub_resource, method) {
            let op_path = match sub_resource {
                Some(sub) => format!("{service}/{resource}/{sub}/{method}"),
                None => format!("{service}/{resource}/{method}"),
            };
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Operation '{op_path}' is not in the allowed operations list"
                )),
            });
        }

        for (label, value) in [
            ("service", service),
            ("resource", resource),
            ("method", method),
        ] {
            if !value
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
            {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Invalid characters in '{label}': only lowercase alphanumeric, underscore, and hyphen are allowed"
                    )),
                });
            }
        }

        let mut cmd_args = Self::positional_cmd_args(service, resource, sub_resource, method);

        if let Some(params) = args.get("params") {
            if !params.is_object() {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("'params' must be an object".into()),
                });
            }
            cmd_args.push("--params".into());
            cmd_args.push(params.to_string());
        }

        if let Some(body) = args.get("body") {
            if !body.is_object() {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("'body' must be an object".into()),
                });
            }
            cmd_args.push("--json".into());
            cmd_args.push(body.to_string());
        }

        if let Some(format_value) = args.get("format") {
            let format = match format_value.as_str() {
                Some(s) => s,
                None => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some("'format' must be a string".into()),
                    });
                }
            };
            match format {
                "json" | "table" | "yaml" | "csv" => {
                    cmd_args.push("--format".into());
                    cmd_args.push(format.to_string());
                }
                _ => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Invalid format '{format}': must be json, table, yaml, or csv"
                        )),
                    });
                }
            }
        }

        let page_all = match args.get("page_all") {
            Some(v) => match v.as_bool() {
                Some(b) => b,
                None => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some("'page_all' must be a boolean".into()),
                    });
                }
            },
            None => false,
        };
        let page_limit = match args.get("page_limit") {
            Some(v) => match v.as_u64() {
                Some(n) => Some(n),
                None => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some("'page_limit' must be a non-negative integer".into()),
                    });
                }
            },
            None => None,
        };
        cmd_args.extend(Self::build_pagination_args(page_all, page_limit));

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: action budget exhausted".into()),
            });
        }

        let mut cmd = crate::util::hidden_async_command("gws");
        cmd.args(&cmd_args);
        cmd.env_clear();

        for key in &["PATH", "HOME", "APPDATA", "USERPROFILE", "LANG", "TERM"] {
            if let Ok(val) = std::env::var(key) {
                cmd.env(key, val);
            }
        }

        if let Some(ref creds) = self.credentials_path {
            cmd.env("GOOGLE_APPLICATION_CREDENTIALS", creds);
        }

        if let Some(ref account) = self.default_account {
            cmd.args(["--account", account]);
        }

        if self.audit_log {
            tracing::info!(
                tool = "google_workspace",
                service = service,
                resource = resource,
                sub_resource = sub_resource.unwrap_or(""),
                method = method,
                "gws audit: executing API call"
            );
        }

        let result =
            tokio::time::timeout(Duration::from_secs(self.timeout_secs), cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                let mut stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();

                if stdout.len() > MAX_OUTPUT_BYTES {

                    let mut boundary = MAX_OUTPUT_BYTES;
                    while boundary > 0 && !stdout.is_char_boundary(boundary) {
                        boundary -= 1;
                    }
                    stdout.truncate(boundary);
                    stdout.push_str("\n... [output truncated at 1MB]");
                }
                if stderr.len() > MAX_OUTPUT_BYTES {
                    let mut boundary = MAX_OUTPUT_BYTES;
                    while boundary > 0 && !stderr.is_char_boundary(boundary) {
                        boundary -= 1;
                    }
                    stderr.truncate(boundary);
                    stderr.push_str("\n... [stderr truncated at 1MB]");
                }

                Ok(ToolResult {
                    success: output.status.success(),
                    output: stdout,
                    error: if stderr.is_empty() {
                        None
                    } else {
                        Some(stderr)
                    },
                })
            }
            Ok(Err(e)) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Failed to execute gws: {e}. Is gws installed? Run: npm install -g @googleworkspace/cli"
                )),
            }),
            Err(_) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "gws command timed out after {}s and was killed",
                    self.timeout_secs
                )),
            }),
        }
    }
}
