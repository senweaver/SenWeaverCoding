// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::config::schema::WebhookAuditConfig;
use crate::hooks::traits::{HookHandler, HookResult};
use crate::tools::traits::ToolResult;

fn validate_webhook_url(url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("invalid webhook URL: {e}"))?;

    let scheme = parsed.scheme();
    let host_str = parsed.host_str().unwrap_or("");

    let is_localhost = host_str == "localhost" || host_str == "127.0.0.1" || host_str == "::1";

    if scheme != "https" {
        if scheme == "http" && is_localhost && cfg!(debug_assertions) {

        } else {
            return Err(format!(
                "webhook URL must use https:// scheme (got {scheme}://)"
            ));
        }
    }

    if let Some(host) = parsed.host_str() {

        let bare = host.trim_start_matches('[').trim_end_matches(']');
        if let Ok(ip) = bare.parse::<IpAddr>() {
            reject_private_ip(ip)?;
        } else if bare == "localhost" && !(cfg!(debug_assertions) && scheme == "http") {
            return Err("webhook URL must not target localhost".to_string());
        }
    }

    Ok(())
}

fn reject_private_ip(addr: IpAddr) -> Result<(), String> {
    match addr {
        IpAddr::V4(ip) => {
            if ip.is_loopback() {
                return Err(format!(
                    "webhook URL must not target loopback address ({ip})"
                ));
            }
            let octets = ip.octets();

            if octets[0] == 10 {
                return Err(format!(
                    "webhook URL must not target private address ({ip})"
                ));
            }

            if octets[0] == 172 && (octets[1] & 0xf0) == 16 {
                return Err(format!(
                    "webhook URL must not target private address ({ip})"
                ));
            }

            if octets[0] == 192 && octets[1] == 168 {
                return Err(format!(
                    "webhook URL must not target private address ({ip})"
                ));
            }

            if octets[0] == 169 && octets[1] == 254 {
                return Err(format!(
                    "webhook URL must not target link-local address ({ip})"
                ));
            }
        }
        IpAddr::V6(ip) => {
            if ip.is_loopback() {
                return Err(format!(
                    "webhook URL must not target loopback address ({ip})"
                ));
            }
            let segments = ip.segments();

            if (segments[0] & 0xffc0) == 0xfe80 {
                return Err(format!(
                    "webhook URL must not target link-local address ({ip})"
                ));
            }
        }
    }
    Ok(())
}

pub struct WebhookAuditHook {
    config: WebhookAuditConfig,
    client: reqwest::Client,
    pending_args: Arc<Mutex<HashMap<String, Vec<Value>>>>,
}

impl WebhookAuditHook {
    pub fn new(config: WebhookAuditConfig) -> anyhow::Result<Self> {
        if config.enabled && config.url.is_empty() {
            tracing::warn!(
                hook = "webhook-audit",
                "webhook-audit hook is enabled but no URL is configured  -  audit events will be dropped"
            );
        }

        if !config.url.is_empty() {
            if let Err(e) = validate_webhook_url(&config.url) {
                tracing::error!(hook = "webhook-audit", error = %e, "webhook URL validation failed");
                return Err(anyhow::anyhow!("webhook-audit: {e}"));
            }
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| anyhow::anyhow!("failed to build webhook HTTP client: {e}"))?;
        Ok(Self {
            config,
            client,
            pending_args: Arc::new(Mutex::new(HashMap::new())),
        })
    }
}

fn glob_matches(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return pattern == text;
    }

    let parts: Vec<&str> = pattern.split('*').collect();

    let mut pos = 0usize;

    if !pattern.starts_with('*') {
        let first = parts[0];
        if !text.starts_with(first) {
            return false;
        }
        pos = first.len();
    }

    if !pattern.ends_with('*') {
        let last = parts[parts.len() - 1];
        if !text.ends_with(last) {
            return false;
        }

        if text.len() < pos + last.len() {

            return false;
        }
    }

    let end_boundary = if pattern.ends_with('*') {
        text.len()
    } else {
        text.len() - parts[parts.len() - 1].len()
    };

    let start_idx = usize::from(!pattern.starts_with('*'));
    let end_idx = if pattern.ends_with('*') {
        parts.len()
    } else {
        parts.len() - 1
    };

    for part in &parts[start_idx..end_idx] {
        if part.is_empty() {
            continue;
        }
        if let Some(found) = text[pos..end_boundary].find(part) {
            pos += found + part.len();
        } else {
            return false;
        }
    }

    true
}

fn matches_any_pattern(patterns: &[String], tool: &str) -> bool {
    patterns.iter().any(|p| glob_matches(p, tool))
}

#[allow(clippy::cast_possible_truncation)]
fn truncate_args(args: Value, max_bytes: u64) -> Value {
    if max_bytes == 0 {
        return args;
    }
    let serialised = match serde_json::to_string(&args) {
        Ok(s) => s,
        Err(_) => return args,
    };
    if serialised.len() <= max_bytes as usize {
        args
    } else {
        let mut end = max_bytes as usize;
        while end > 0 && !serialised.is_char_boundary(end) {
            end -= 1;
        }
        Value::String(format!("{}...[truncated]", &serialised[..end]))
    }
}

#[async_trait]
impl HookHandler for WebhookAuditHook {
    fn name(&self) -> &str {
        "webhook-audit"
    }

    fn priority(&self) -> i32 {
        -100
    }

    async fn before_tool_call(&self, name: String, args: Value) -> HookResult<(String, Value)> {
        if self.config.include_args && matches_any_pattern(&self.config.tool_patterns, &name) {
            tracing::debug!(hook = "webhook-audit", tool = %name, "capturing args for audit");
            self.pending_args
                .lock()
                .entry(name.clone())
                .or_default()
                .push(args.clone());
        }
        HookResult::Continue((name, args))
    }

    async fn on_after_tool_call(&self, tool: &str, result: &ToolResult, duration: Duration) {

        if self.config.url.is_empty() {
            return;
        }

        if !matches_any_pattern(&self.config.tool_patterns, tool) {
            return;
        }

        let args_value: Value = if self.config.include_args {
            let raw = {
                let mut map = self.pending_args.lock();
                let entry = map.get_mut(tool).and_then(|v| {
                    if v.is_empty() {
                        None
                    } else {
                        Some(v.remove(0))
                    }
                });

                if map.get(tool).is_some_and(|v| v.is_empty()) {
                    map.remove(tool);
                }
                entry
            };
            match raw {
                Some(a) => truncate_args(
                    crate::services::governance::credential_vault::redact_args_optional(&a),
                    self.config.max_args_bytes,
                ),
                None => Value::Null,
            }
        } else {
            Value::Null
        };

        #[allow(clippy::cast_possible_truncation)]
        let duration_ms = duration.as_millis() as u64;

        let redacted_error = result.error.as_deref().map(
            crate::services::governance::credential_vault::redact_for_audit_optional,
        );

        let payload = serde_json::json!({
            "event": "tool_call",
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "tool": tool,
            "success": result.success,
            "duration_ms": duration_ms,
            "error": redacted_error,
            "args": args_value,
        });

        let client = self.client.clone();
        let url = self.config.url.clone();

        let _ = crate::runtime::spawn_supervised("hooks.webhook_audit.post", async move {
            match client.post(&url).json(&payload).send().await {
                Ok(resp) => {
                    if !resp.status().is_success() {
                        tracing::error!(
                            hook = "webhook-audit",
                            url = %url,
                            status = %resp.status(),
                            "webhook endpoint returned non-success status"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        hook = "webhook-audit",
                        url = %url,
                        error = %e,
                        "failed to POST audit payload"
                    );
                }
            }
        });
    }
}
