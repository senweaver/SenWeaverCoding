// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Guardrails - pre-tool-execution authorization and policy enforcement.
//!
//! Provides a pluggable interception layer that evaluates tool calls before execution.
//! Supports allowlist/denylist rules, rate limiting, and custom provider-based authorization.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub mod types;
#[allow(unused_imports)]
pub use types::*;

/// Configuration for the guardrails system.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GuardrailsConfig {
    /// Enable guardrails enforcement. Default: false.
    #[serde(default)]
    pub enabled: bool,
    /// Default policy when no rule matches. Default: "allow".
    #[serde(default = "default_policy")]
    pub default_policy: GuardrailPolicy,
    /// Tool-specific rules.
    #[serde(default)]
    pub rules: Vec<GuardrailRule>,
    /// Rate limiting configuration.
    #[serde(default)]
    pub rate_limits: Vec<RateLimitRule>,
    /// Maximum total tool calls per session. 0 = unlimited.
    #[serde(default)]
    pub max_calls_per_session: usize,
    /// Tools that always bypass guardrails (safety-critical).
    #[serde(default)]
    pub bypass_tools: Vec<String>,
}

fn default_policy() -> GuardrailPolicy {
    GuardrailPolicy::Allow
}

impl Default for GuardrailsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_policy: GuardrailPolicy::Allow,
            rules: Vec::new(),
            rate_limits: Vec::new(),
            max_calls_per_session: 0,
            bypass_tools: Vec::new(),
        }
    }
}

/// A single guardrail rule for tool authorization.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GuardrailRule {
    /// Tool name pattern (exact match or glob with '*').
    pub tool_pattern: String,
    /// Policy to apply when matched.
    pub policy: GuardrailPolicy,
    /// Optional reason shown when blocked.
    #[serde(default)]
    pub reason: Option<String>,
    /// Only apply during these contexts (empty = always).
    #[serde(default)]
    pub contexts: Vec<String>,
}

/// Rate limiting rule for tool calls.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RateLimitRule {
    /// Tool name pattern.
    pub tool_pattern: String,
    /// Maximum calls allowed in the window.
    pub max_calls: usize,
    /// Window duration in seconds.
    #[serde(default = "default_window_secs")]
    pub window_secs: u64,
}

fn default_window_secs() -> u64 {
    60
}

/// Guardrail policy actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GuardrailPolicy {
    /// Allow the tool call.
    Allow,
    /// Block the tool call.
    Deny,
    /// Require explicit user approval.
    RequireApproval,
    /// Allow but log for audit.
    AuditOnly,
}

/// Result of a guardrail check.
#[derive(Debug, Clone)]
pub struct GuardrailVerdict {
    /// Whether the tool call is allowed.
    pub allowed: bool,
    /// Policy that was applied.
    pub policy: GuardrailPolicy,
    /// Human-readable reason for the decision.
    pub reason: String,
    /// Whether this requires user approval before proceeding.
    pub needs_approval: bool,
}

impl GuardrailVerdict {
    pub fn allow() -> Self {
        Self {
            allowed: true,
            policy: GuardrailPolicy::Allow,
            reason: "Allowed by default policy".to_string(),
            needs_approval: false,
        }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            allowed: false,
            policy: GuardrailPolicy::Deny,
            reason: reason.into(),
            needs_approval: false,
        }
    }

    pub fn require_approval(reason: impl Into<String>) -> Self {
        Self {
            allowed: false,
            policy: GuardrailPolicy::RequireApproval,
            reason: reason.into(),
            needs_approval: true,
        }
    }

    pub fn audit(reason: impl Into<String>) -> Self {
        Self {
            allowed: true,
            policy: GuardrailPolicy::AuditOnly,
            reason: reason.into(),
            needs_approval: false,
        }
    }
}

/// The guardrails engine that evaluates tool calls.
pub struct GuardrailsEngine {
    config: GuardrailsConfig,
    call_counts: Arc<RwLock<HashMap<String, Vec<Instant>>>>,
    session_total: Arc<RwLock<usize>>,
}

impl GuardrailsEngine {
    pub fn new(config: GuardrailsConfig) -> Self {
        Self {
            config,
            call_counts: Arc::new(RwLock::new(HashMap::new())),
            session_total: Arc::new(RwLock::new(0)),
        }
    }

    pub fn from_config(config: &GuardrailsConfig) -> Self {
        Self::new(config.clone())
    }

    /// Check whether a tool call should be allowed.
    pub fn check(&self, tool_name: &str, context: Option<&str>) -> GuardrailVerdict {
        if !self.config.enabled {
            return GuardrailVerdict::allow();
        }

        if self
            .config
            .bypass_tools
            .iter()
            .any(|t| t.eq_ignore_ascii_case(tool_name))
        {
            return GuardrailVerdict::allow();
        }

        if self.config.max_calls_per_session > 0 {
            let total = *self.session_total.read();
            if total >= self.config.max_calls_per_session {
                return GuardrailVerdict::deny(format!(
                    "Session tool call limit ({}) reached",
                    self.config.max_calls_per_session
                ));
            }
        }

        if let Some(verdict) = self.check_rate_limits(tool_name) {
            return verdict;
        }

        for rule in &self.config.rules {
            if Self::matches_pattern(&rule.tool_pattern, tool_name) {
                if !rule.contexts.is_empty() {
                    if let Some(ctx) = context {
                        if !rule.contexts.iter().any(|c| c == ctx) {
                            continue;
                        }
                    } else {
                        continue;
                    }
                }

                return match rule.policy {
                    GuardrailPolicy::Allow => GuardrailVerdict::allow(),
                    GuardrailPolicy::Deny => GuardrailVerdict::deny(
                        rule.reason
                            .as_deref()
                            .unwrap_or("Blocked by guardrail rule"),
                    ),
                    GuardrailPolicy::RequireApproval => GuardrailVerdict::require_approval(
                        rule.reason
                            .as_deref()
                            .unwrap_or("Requires approval per guardrail rule"),
                    ),
                    GuardrailPolicy::AuditOnly => GuardrailVerdict::audit(
                        rule.reason.as_deref().unwrap_or("Audit: tool call logged"),
                    ),
                };
            }
        }

        match self.config.default_policy {
            GuardrailPolicy::Allow => GuardrailVerdict::allow(),
            GuardrailPolicy::Deny => GuardrailVerdict::deny("Denied by default policy"),
            GuardrailPolicy::RequireApproval => {
                GuardrailVerdict::require_approval("Requires approval by default policy")
            }
            GuardrailPolicy::AuditOnly => GuardrailVerdict::audit("Audit: default policy"),
        }
    }

    /// Record that a tool call was executed (for rate limiting).
    pub fn record_call(&self, tool_name: &str) {
        let now = Instant::now();
        let max_window = std::time::Duration::from_secs(3600);
        let mut counts = self.call_counts.write();
        let timestamps = counts.entry(tool_name.to_string()).or_default();
        timestamps.push(now);
        timestamps.retain(|t| t.elapsed() < max_window);
        *self.session_total.write() += 1;
    }

    /// Reset session counters.
    pub fn reset_session(&self) {
        self.call_counts.write().clear();
        *self.session_total.write() = 0;
    }

    fn check_rate_limits(&self, tool_name: &str) -> Option<GuardrailVerdict> {
        let counts = self.call_counts.read();
        for rule in &self.config.rate_limits {
            if Self::matches_pattern(&rule.tool_pattern, tool_name) {
                if let Some(calls) = counts.get(tool_name) {
                    let window = Duration::from_secs(rule.window_secs);
                    let now = Instant::now();
                    let recent = calls
                        .iter()
                        .filter(|t| now.duration_since(**t) < window)
                        .count();
                    if recent >= rule.max_calls {
                        return Some(GuardrailVerdict::deny(format!(
                            "Rate limit exceeded: {} calls in {}s (max {})",
                            recent, rule.window_secs, rule.max_calls
                        )));
                    }
                }
            }
        }
        None
    }

    fn matches_pattern(pattern: &str, name: &str) -> bool {
        if pattern == "*" {
            return true;
        }
        if let Some(prefix) = pattern.strip_suffix('*') {
            return name.starts_with(prefix);
        }
        if let Some(suffix) = pattern.strip_prefix('*') {
            return name.ends_with(suffix);
        }
        pattern == name
    }
}

static GLOBAL_GUARDRAILS: std::sync::LazyLock<RwLock<Option<GuardrailsEngine>>> =
    std::sync::LazyLock::new(|| RwLock::new(None));

/// Set (or update) the global guardrails engine.
/// The engine is always constructed; when `config.enabled` is false, [`GuardrailsEngine::check`]
/// is a no-op that allows all tools.
pub fn ensure_global_guardrails(config: GuardrailsConfig) {
    *GLOBAL_GUARDRAILS.write() = Some(GuardrailsEngine::new(config));
}

/// Check a tool call against the global guardrails.
/// Returns `Ok(())` if allowed, `Err(reason)` if denied.
pub fn check_tool_guardrails(tool_name: &str) -> Result<(), String> {
    let guard = GLOBAL_GUARDRAILS.read();
    match guard.as_ref() {
        Some(engine) => {
            let verdict = engine.check(tool_name, None);
            if verdict.allowed {
                engine.record_call(tool_name);
                Ok(())
            } else {
                Err(verdict.reason)
            }
        }
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_allow() {
        let engine = GuardrailsEngine::new(GuardrailsConfig {
            enabled: true,
            ..Default::default()
        });
        let v = engine.check("shell", None);
        assert!(v.allowed);
    }

    #[test]
    fn test_deny_rule() {
        let engine = GuardrailsEngine::new(GuardrailsConfig {
            enabled: true,
            rules: vec![GuardrailRule {
                tool_pattern: "shell".to_string(),
                policy: GuardrailPolicy::Deny,
                reason: Some("Shell blocked".to_string()),
                contexts: vec![],
            }],
            ..Default::default()
        });
        let v = engine.check("shell", None);
        assert!(!v.allowed);
        assert_eq!(v.reason, "Shell blocked");
    }

    #[test]
    fn test_wildcard_pattern() {
        let engine = GuardrailsEngine::new(GuardrailsConfig {
            enabled: true,
            rules: vec![GuardrailRule {
                tool_pattern: "memory_*".to_string(),
                policy: GuardrailPolicy::AuditOnly,
                reason: None,
                contexts: vec![],
            }],
            ..Default::default()
        });
        let v = engine.check("memory_store", None);
        assert!(v.allowed);
        assert_eq!(v.policy, GuardrailPolicy::AuditOnly);
    }

    #[test]
    fn test_rate_limit() {
        let engine = GuardrailsEngine::new(GuardrailsConfig {
            enabled: true,
            rate_limits: vec![RateLimitRule {
                tool_pattern: "web_search".to_string(),
                max_calls: 3,
                window_secs: 60,
            }],
            ..Default::default()
        });

        for _ in 0..3 {
            assert!(engine.check("web_search", None).allowed);
            engine.record_call("web_search");
        }

        let v = engine.check("web_search", None);
        assert!(!v.allowed);
        assert!(v.reason.contains("Rate limit"));
    }

    #[test]
    fn test_session_limit() {
        let engine = GuardrailsEngine::new(GuardrailsConfig {
            enabled: true,
            max_calls_per_session: 2,
            ..Default::default()
        });

        engine.record_call("a");
        engine.record_call("b");
        let v = engine.check("c", None);
        assert!(!v.allowed);
    }

    #[test]
    fn test_bypass_tools() {
        let engine = GuardrailsEngine::new(GuardrailsConfig {
            enabled: true,
            default_policy: GuardrailPolicy::Deny,
            bypass_tools: vec!["emergency_stop".to_string()],
            ..Default::default()
        });

        assert!(!engine.check("shell", None).allowed);
        assert!(engine.check("emergency_stop", None).allowed);
    }

    #[test]
    fn test_context_filter() {
        let engine = GuardrailsEngine::new(GuardrailsConfig {
            enabled: true,
            rules: vec![GuardrailRule {
                tool_pattern: "file_write".to_string(),
                policy: GuardrailPolicy::Deny,
                reason: Some("Blocked in subagent".to_string()),
                contexts: vec!["subagent".to_string()],
            }],
            ..Default::default()
        });

        assert!(engine.check("file_write", None).allowed);
        assert!(engine.check("file_write", Some("main")).allowed);
        assert!(!engine.check("file_write", Some("subagent")).allowed);
    }

    #[test]
    fn test_disabled_guardrails() {
        let engine = GuardrailsEngine::new(GuardrailsConfig::default());
        assert!(engine.check("anything", None).allowed);
    }
}
