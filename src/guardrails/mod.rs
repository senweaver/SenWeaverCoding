// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub mod types;
pub use types::*;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GuardrailsConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_policy")]
    pub default_policy: GuardrailPolicy,

    #[serde(default)]
    pub rules: Vec<GuardrailRule>,

    #[serde(default)]
    pub rate_limits: Vec<RateLimitRule>,

    #[serde(default)]
    pub max_calls_per_session: usize,

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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GuardrailRule {

    pub tool_pattern: String,

    pub policy: GuardrailPolicy,

    #[serde(default)]
    pub reason: Option<String>,

    #[serde(default)]
    pub contexts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RateLimitRule {

    pub tool_pattern: String,

    pub max_calls: usize,

    #[serde(default = "default_window_secs")]
    pub window_secs: u64,
}

fn default_window_secs() -> u64 {
    60
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GuardrailPolicy {

    Allow,

    Deny,

    RequireApproval,

    AuditOnly,
}

#[derive(Debug, Clone)]
pub struct GuardrailVerdict {

    pub allowed: bool,

    pub policy: GuardrailPolicy,

    pub reason: String,

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

pub struct GuardrailsEngine {
    config: GuardrailsConfig,
    call_counts: Arc<RwLock<HashMap<String, Vec<Instant>>>>,
    session_total: Arc<RwLock<usize>>,
    bypass_warned: Arc<RwLock<HashSet<String>>>,
}

impl GuardrailsEngine {
    pub fn new(config: GuardrailsConfig) -> Self {
        Self {
            config,
            call_counts: Arc::new(RwLock::new(HashMap::new())),
            session_total: Arc::new(RwLock::new(0)),
            bypass_warned: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    pub fn from_config(config: &GuardrailsConfig) -> Self {
        Self::new(config.clone())
    }

    pub fn check(&self, tool_name: &str, context: Option<&str>) -> GuardrailVerdict {
        let ctx = context.map(|s| GuardrailContext {
            coding_mode: Some(s),
            ..GuardrailContext::default()
        });
        self.check_with_context(tool_name, ctx.as_ref())
    }

    pub fn check_with_context(
        &self,
        tool_name: &str,
        context: Option<&GuardrailContext<'_>>,
    ) -> GuardrailVerdict {
        if !self.config.enabled {
            return GuardrailVerdict::allow();
        }

        let fallback = GuardrailContext {
            tool_name: Some(tool_name),
            ..GuardrailContext::default()
        };
        let active_ctx = context.unwrap_or(&fallback);

        if self.is_bypass_tool(tool_name) {
            self.warn_bypass_once(tool_name);
            if let Some(deny) = self.explicit_deny_verdict(tool_name, active_ctx) {
                return deny;
            }
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

        self.rule_verdict(tool_name, active_ctx)
    }

    pub fn authorize_with_context(
        &self,
        tool_name: &str,
        context: Option<&GuardrailContext<'_>>,
    ) -> GuardrailVerdict {
        if !self.config.enabled {
            return GuardrailVerdict::allow();
        }

        let fallback = GuardrailContext {
            tool_name: Some(tool_name),
            ..GuardrailContext::default()
        };
        let active_ctx = context.unwrap_or(&fallback);

        if self.is_bypass_tool(tool_name) {
            self.warn_bypass_once(tool_name);
            if let Some(deny) = self.explicit_deny_verdict(tool_name, active_ctx) {
                return deny;
            }
            self.record_call(tool_name);
            return GuardrailVerdict::allow();
        }

        let verdict = self.rule_verdict(tool_name, active_ctx);

        let now = Instant::now();
        let mut counts = self.call_counts.write();
        let mut total = self.session_total.write();

        if self.config.max_calls_per_session > 0 && *total >= self.config.max_calls_per_session {
            return GuardrailVerdict::deny(format!(
                "Session tool call limit ({}) reached",
                self.config.max_calls_per_session
            ));
        }

        for rule in &self.config.rate_limits {
            if Self::matches_pattern(&rule.tool_pattern, tool_name) {
                if let Some(calls) = counts.get(tool_name) {
                    let window = Duration::from_secs(rule.window_secs);
                    let recent = calls
                        .iter()
                        .filter(|t| now.duration_since(**t) < window)
                        .count();
                    if recent >= rule.max_calls {
                        return GuardrailVerdict::deny(format!(
                            "Rate limit exceeded: {} calls in {}s (max {})",
                            recent, rule.window_secs, rule.max_calls
                        ));
                    }
                }
            }
        }

        if verdict.allowed {
            let max_window = Duration::from_secs(3600);
            let timestamps = counts.entry(tool_name.to_string()).or_default();
            timestamps.push(now);
            timestamps.retain(|t| now.duration_since(*t) < max_window);
            *total += 1;
        }

        verdict
    }

    fn is_bypass_tool(&self, tool_name: &str) -> bool {
        self.config
            .bypass_tools
            .iter()
            .any(|t| t.eq_ignore_ascii_case(tool_name))
    }

    fn warn_bypass_once(&self, tool_name: &str) {
        {
            let warned = self.bypass_warned.read();
            if warned.contains(tool_name) {
                return;
            }
        }
        let mut warned = self.bypass_warned.write();
        if warned.insert(tool_name.to_string()) {
            tracing::warn!(
                tool = %tool_name,
                "guardrails bypass_tools matched: rate limits are skipped for this tool, but explicit deny rules still apply"
            );
        }
    }

    fn explicit_deny_verdict(
        &self,
        tool_name: &str,
        active_ctx: &GuardrailContext<'_>,
    ) -> Option<GuardrailVerdict> {
        for rule in &self.config.rules {
            if Self::matches_pattern(&rule.tool_pattern, tool_name) {
                if !active_ctx.matches(&rule.contexts) {
                    continue;
                }
                if rule.policy == GuardrailPolicy::Deny {
                    return Some(GuardrailVerdict::deny(
                        rule.reason
                            .as_deref()
                            .unwrap_or("Blocked by guardrail rule"),
                    ));
                }
                return None;
            }
        }
        None
    }

    fn rule_verdict(
        &self,
        tool_name: &str,
        active_ctx: &GuardrailContext<'_>,
    ) -> GuardrailVerdict {
        for rule in &self.config.rules {
            if Self::matches_pattern(&rule.tool_pattern, tool_name) {
                if !active_ctx.matches(&rule.contexts) {
                    continue;
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

    pub fn record_call(&self, tool_name: &str) {
        let now = Instant::now();
        let max_window = std::time::Duration::from_secs(3600);
        let mut counts = self.call_counts.write();
        let timestamps = counts.entry(tool_name.to_string()).or_default();
        timestamps.push(now);
        timestamps.retain(|t| t.elapsed() < max_window);
        *self.session_total.write() += 1;
    }

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
        if !pattern.contains(['*', '?', '[']) {
            return pattern == name;
        }
        {
            let cache = GLOB_MATCHER_CACHE.read();
            if let Some(cached) = cache.get(pattern) {
                return match cached {
                    Some(matcher) => matcher.is_match(name),
                    None => pattern == name,
                };
            }
        }
        let compiled = match globset::GlobBuilder::new(pattern)
            .literal_separator(false)
            .build()
        {
            Ok(glob) => Some(glob.compile_matcher()),
            Err(error) => {
                tracing::warn!(
                    target: "guardrails",
                    pattern,
                    %error,
                    "invalid guardrail tool pattern; treating as exact match"
                );
                None
            }
        };
        let result = match &compiled {
            Some(matcher) => matcher.is_match(name),
            None => pattern == name,
        };
        let mut cache = GLOB_MATCHER_CACHE.write();
        if cache.len() >= 256 {
            cache.clear();
        }
        cache.insert(pattern.to_string(), compiled);
        result
    }
}

static GLOB_MATCHER_CACHE: std::sync::LazyLock<
    RwLock<HashMap<String, Option<globset::GlobMatcher>>>,
> = std::sync::LazyLock::new(|| RwLock::new(HashMap::new()));

static GLOBAL_GUARDRAILS: std::sync::LazyLock<RwLock<Option<GuardrailsEngine>>> =
    std::sync::LazyLock::new(|| RwLock::new(None));

pub fn ensure_global_guardrails(config: GuardrailsConfig) {
    *GLOBAL_GUARDRAILS.write() = Some(GuardrailsEngine::new(config));
}

#[derive(Debug, Default, Clone)]
pub struct GuardrailContext<'a> {

    pub coding_mode: Option<&'a str>,

    pub permission_mode: Option<&'a str>,

    pub tool_name: Option<&'a str>,
}

impl GuardrailContext<'_> {

    fn matches(&self, rule_contexts: &[String]) -> bool {
        if rule_contexts.is_empty() {
            return true;
        }
        let candidates: Vec<String> = [self.coding_mode, self.permission_mode, self.tool_name]
            .into_iter()
            .flatten()
            .map(|s| s.to_ascii_lowercase())
            .collect();
        if candidates.is_empty() {
            return false;
        }
        rule_contexts
            .iter()
            .any(|c| candidates.iter().any(|cand| cand == &c.to_ascii_lowercase()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardrailDecision {
    Allow,
    Deny(String),
    RequireApproval(String),
}

pub fn evaluate_tool_guardrails(
    tool_name: &str,
    ctx: Option<&GuardrailContext<'_>>,
) -> GuardrailDecision {
    let guard = GLOBAL_GUARDRAILS.read();
    match guard.as_ref() {
        Some(engine) => {
            let verdict = engine.authorize_with_context(tool_name, ctx);
            if verdict.needs_approval {
                GuardrailDecision::RequireApproval(verdict.reason)
            } else if verdict.allowed {
                GuardrailDecision::Allow
            } else {
                GuardrailDecision::Deny(verdict.reason)
            }
        }
        None => GuardrailDecision::Allow,
    }
}

pub fn check_tool_guardrails(
    tool_name: &str,
    ctx: Option<&GuardrailContext<'_>>,
) -> Result<(), String> {
    match evaluate_tool_guardrails(tool_name, ctx) {
        GuardrailDecision::Allow => Ok(()),
        GuardrailDecision::Deny(reason) | GuardrailDecision::RequireApproval(reason) => Err(reason),
    }
}
