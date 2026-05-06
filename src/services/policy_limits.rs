// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Policy limits service — mirrors claude-code-typescript-src`services/policyLimits/`.
// Enforces organization-level policies on tool usage, model selection,
// spending limits, and allowed operations.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRule {
    pub id: String,
    pub description: String,
    pub kind: PolicyKind,
    pub enforcement: PolicyEnforcement,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PolicyKind {

    BlockTools { tool_names: Vec<String> },

    AllowModels { model_ids: Vec<String> },

    SpendingCap { max_cents: u64 },

    RestrictPaths { allowed_globs: Vec<String> },

    MaxToolCallsPerTurn { limit: u32 },

    AllowDomains { domains: Vec<String> },

    Custom { hook_event: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyEnforcement {
    Block,
    Warn,
    Log,
}

pub struct PolicyLimitsService {
    rules: Vec<PolicyRule>,
}

impl PolicyLimitsService {
    pub fn new(rules: Vec<PolicyRule>) -> Self {
        Self { rules }
    }

    pub fn check_tool(&self, tool_name: &str) -> PolicyCheckResult {
        for rule in &self.rules {
            if let PolicyKind::BlockTools { tool_names } = &rule.kind {
                if tool_names.iter().any(|t| t == tool_name) {
                    return PolicyCheckResult {
                        allowed: rule.enforcement != PolicyEnforcement::Block,
                        violations: vec![PolicyViolation {
                            rule_id: rule.id.clone(),
                            message: format!(
                                "Tool '{tool_name}' is blocked by policy: {}",
                                rule.description
                            ),
                            enforcement: rule.enforcement,
                        }],
                    };
                }
            }
        }
        PolicyCheckResult::ok()
    }

    pub fn check_model(&self, model_id: &str) -> PolicyCheckResult {
        for rule in &self.rules {
            if let PolicyKind::AllowModels { model_ids } = &rule.kind {
                if !model_ids.iter().any(|m| m == model_id) {
                    return PolicyCheckResult {
                        allowed: rule.enforcement != PolicyEnforcement::Block,
                        violations: vec![PolicyViolation {
                            rule_id: rule.id.clone(),
                            message: format!("Model '{model_id}' is not in the allowed list"),
                            enforcement: rule.enforcement,
                        }],
                    };
                }
            }
        }
        PolicyCheckResult::ok()
    }

    pub fn check_spending(&self, current_cents: u64) -> PolicyCheckResult {
        for rule in &self.rules {
            if let PolicyKind::SpendingCap { max_cents } = &rule.kind {
                if current_cents > *max_cents {
                    return PolicyCheckResult {
                        allowed: rule.enforcement != PolicyEnforcement::Block,
                        violations: vec![PolicyViolation {
                            rule_id: rule.id.clone(),
                            message: format!(
                                "Spending limit exceeded: {current_cents} cents > {max_cents} cents cap"
                            ),
                            enforcement: rule.enforcement,
                        }],
                    };
                }
            }
        }
        PolicyCheckResult::ok()
    }

    pub fn check_write_path(&self, path: &str) -> PolicyCheckResult {
        for rule in &self.rules {
            if let PolicyKind::RestrictPaths { allowed_globs } = &rule.kind {
                let allowed = allowed_globs.iter().any(|g| glob_matches(g, path));
                if !allowed {
                    return PolicyCheckResult {
                        allowed: rule.enforcement != PolicyEnforcement::Block,
                        violations: vec![PolicyViolation {
                            rule_id: rule.id.clone(),
                            message: format!("Path '{path}' is not in allowed write paths"),
                            enforcement: rule.enforcement,
                        }],
                    };
                }
            }
        }
        PolicyCheckResult::ok()
    }
}

#[derive(Debug, Clone)]
pub struct PolicyCheckResult {
    pub allowed: bool,
    pub violations: Vec<PolicyViolation>,
}

impl PolicyCheckResult {
    pub fn ok() -> Self {
        Self {
            allowed: true,
            violations: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyViolation {
    pub rule_id: String,
    pub message: String,
    pub enforcement: PolicyEnforcement,
}

fn glob_matches(pattern: &str, path: &str) -> bool {
    if pattern == "**" {
        return true;
    }
    if pattern.contains("**") {
        let prefix = pattern.split("**").next().unwrap_or("");
        return path.starts_with(prefix);
    }
    if pattern.contains('*') {
        let parts: Vec<&str> = pattern.split('*').collect();
        if parts.len() == 2 {
            return path.starts_with(parts[0]) && path.ends_with(parts[1]);
        }
    }
    path == pattern
}
