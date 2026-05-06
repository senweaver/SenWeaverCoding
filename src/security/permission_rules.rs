// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! OpenCode-style glob-based permission rules.
//!
//! This module provides fine-grained permission control using glob patterns,
//! inspired by OpenCode's permission system. Rules can match file paths,
//! tool names, or other patterns with allow/deny/ask actions.

use globset::{Glob, GlobMatcher, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionAction {

    Allow,

    Deny,

    Ask,
}

impl Default for PermissionAction {
    fn default() -> Self {
        Self::Ask
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {

    pub permission: String,

    #[serde(default = "default_pattern")]
    pub pattern: String,

    pub action: PermissionAction,
}

fn default_pattern() -> String {
    "*".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PermissionConfig {

    Simple(String),

    Pattern(HashMap<String, PatternRule>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PatternRule {

    Action(PermissionAction),

    Patterns(HashMap<String, PermissionAction>),
}

#[derive(Debug, Clone)]
pub struct PermissionRuleSet {

    glob_set: Arc<GlobSet>,

    rules: Vec<CompiledRule>,

    permission_index: HashMap<String, Vec<usize>>,
}

#[derive(Debug, Clone)]
struct CompiledRule {

    rule: PermissionRule,

    matcher: GlobMatcher,
}

#[derive(Debug, thiserror::Error)]
pub enum PermissionError {
    #[error("Invalid glob pattern: {0}")]
    InvalidPattern(String),
    #[error("Permission denied by rule: {0}")]
    Denied(String),
    #[error("Permission requires confirmation: {0}")]
    RequiresConfirmation(String),
}

impl PermissionRuleSet {

    pub fn new(rules: Vec<PermissionRule>) -> Result<Self, PermissionError> {
        let mut builder = GlobSetBuilder::new();
        let mut compiled_rules = Vec::new();
        let mut permission_index: HashMap<String, Vec<usize>> = HashMap::new();

        for (idx, rule) in rules.iter().enumerate() {
            let glob = Glob::new(&rule.pattern)
                .map_err(|e| PermissionError::InvalidPattern(e.to_string()))?;
            let matcher = glob.compile_matcher();
            builder.add(glob);
            compiled_rules.push(CompiledRule {
                rule: rule.clone(),
                matcher,
            });

            permission_index
                .entry(rule.permission.clone())
                .or_default()
                .push(idx);
        }

        let glob_set = Arc::new(
            builder
                .build()
                .map_err(|e| PermissionError::InvalidPattern(e.to_string()))?,
        );

        Ok(Self {
            glob_set,
            rules: compiled_rules,
            permission_index,
        })
    }

    pub fn from_config(
        config: &HashMap<String, PermissionConfig>,
    ) -> Result<Self, PermissionError> {
        let mut rules = Vec::new();

        for (permission, cfg) in config {
            match cfg {
                PermissionConfig::Simple(s) => {

                    if let Some((pattern, action)) = s.split_once(':') {
                        let action = match action.trim() {
                            "allow" => PermissionAction::Allow,
                            "deny" => PermissionAction::Deny,
                            "ask" => PermissionAction::Ask,
                            _ => continue,
                        };
                        rules.push(PermissionRule {
                            permission: permission.clone(),
                            pattern: pattern.trim().to_string(),
                            action,
                        });
                    } else {

                        let action = match s.trim() {
                            "allow" => PermissionAction::Allow,
                            "deny" => PermissionAction::Deny,
                            "ask" => PermissionAction::Ask,
                            _ => continue,
                        };
                        rules.push(PermissionRule {
                            permission: permission.clone(),
                            pattern: "*".to_string(),
                            action,
                        });
                    }
                }
                PermissionConfig::Pattern(patterns) => {
                    for (pattern, rule) in patterns {
                        let action = match rule {
                            PatternRule::Action(a) => *a,
                            PatternRule::Patterns(pats) => {

                                pats.values().next().copied().unwrap_or_default()
                            }
                        };
                        rules.push(PermissionRule {
                            permission: permission.clone(),
                            pattern: pattern.clone(),
                            action,
                        });
                    }
                }
            }
        }

        Self::new(rules)
    }

    pub fn evaluate(&self, permission: &str, target: &str) -> PermissionResult {

        if let Some(indices) = self.permission_index.get(permission) {
            for &idx in indices {
                let rule = &self.rules[idx];
                if rule.matcher.is_match(target) {
                    return PermissionResult {
                        action: rule.rule.action,
                        matched_rule: Some(rule.rule.clone()),
                    };
                }
            }
        }

        if let Some(indices) = self.permission_index.get("*") {
            for &idx in indices {
                let rule = &self.rules[idx];
                if rule.matcher.is_match(target) {
                    return PermissionResult {
                        action: rule.rule.action,
                        matched_rule: Some(rule.rule.clone()),
                    };
                }
            }
        }

        PermissionResult {
            action: PermissionAction::Ask,
            matched_rule: None,
        }
    }

    pub fn is_allowed(&self, permission: &str, target: &str) -> bool {
        matches!(
            self.evaluate(permission, target).action,
            PermissionAction::Allow
        )
    }

    pub fn is_denied(&self, permission: &str, target: &str) -> bool {
        matches!(
            self.evaluate(permission, target).action,
            PermissionAction::Deny
        )
    }

    pub fn requires_confirmation(&self, permission: &str, target: &str) -> bool {
        matches!(
            self.evaluate(permission, target).action,
            PermissionAction::Ask
        )
    }
}

#[derive(Debug, Clone)]
pub struct PermissionResult {

    pub action: PermissionAction,

    pub matched_rule: Option<PermissionRule>,
}

#[derive(Debug, Clone)]
pub struct PermissionRequest {

    pub permission: String,

    pub target: String,

    pub metadata: HashMap<String, serde_json::Value>,
}

impl PermissionRequest {

    pub fn new(permission: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            permission: permission.into(),
            target: target.into(),
            metadata: HashMap::new(),
        }
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    pub fn file_extension(&self) -> Option<String> {
        Path::new(&self.target)
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| format!("*.{}", s))
    }
}

#[allow(clippy::wildcard_imports)]
pub mod builder {
    use super::*;

    pub fn default_rules() -> Vec<PermissionRule> {
        vec![

            PermissionRule {
                permission: "edit".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "edit".to_string(),
                pattern: "*.env".to_string(),
                action: PermissionAction::Ask,
            },
            PermissionRule {
                permission: "edit".to_string(),
                pattern: "*.env.*".to_string(),
                action: PermissionAction::Ask,
            },
            PermissionRule {
                permission: "edit".to_string(),
                pattern: "*.env.example".to_string(),
                action: PermissionAction::Allow,
            },

            PermissionRule {
                permission: "read".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "read".to_string(),
                pattern: "*.env".to_string(),
                action: PermissionAction::Ask,
            },
            PermissionRule {
                permission: "read".to_string(),
                pattern: "*.env.*".to_string(),
                action: PermissionAction::Ask,
            },

            PermissionRule {
                permission: "bash".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Ask,
            },

            PermissionRule {
                permission: "external_directory".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Ask,
            },

            PermissionRule {
                permission: "doom_loop".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Ask,
            },
        ]
    }

    pub fn plan_mode_rules() -> Vec<PermissionRule> {
        vec![

            PermissionRule {
                permission: "read".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },

            PermissionRule {
                permission: "edit".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Deny,
            },

            PermissionRule {
                permission: "edit".to_string(),
                pattern: ".opencode/plans/*.md".to_string(),
                action: PermissionAction::Allow,
            },

            PermissionRule {
                permission: "bash".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Ask,
            },

            PermissionRule {
                permission: "question".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },
        ]
    }

    pub fn build_mode_rules() -> Vec<PermissionRule> {
        vec![

            PermissionRule {
                permission: "edit".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },

            PermissionRule {
                permission: "read".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },

            PermissionRule {
                permission: "bash".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },

            PermissionRule {
                permission: "external_directory".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Ask,
            },

            PermissionRule {
                permission: "question".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },

            PermissionRule {
                permission: "plan_enter".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },
        ]
    }

    pub fn harness_mode_rules() -> Vec<PermissionRule> {
        vec![

            PermissionRule {
                permission: "read".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },

            PermissionRule {
                permission: "edit".to_string(),
                pattern: "*.rs".to_string(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "edit".to_string(),
                pattern: "*.ts".to_string(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "edit".to_string(),
                pattern: "*.js".to_string(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "edit".to_string(),
                pattern: "*.py".to_string(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "edit".to_string(),
                pattern: "*.go".to_string(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "edit".to_string(),
                pattern: "*.md".to_string(),
                action: PermissionAction::Allow,
            },

            PermissionRule {
                permission: "edit".to_string(),
                pattern: ".opencode/**".to_string(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "edit".to_string(),
                pattern: ".trellis/**".to_string(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "edit".to_string(),
                pattern: "STATE.md".to_string(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "edit".to_string(),
                pattern: "ROADMAP.md".to_string(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "edit".to_string(),
                pattern: "TASKS.md".to_string(),
                action: PermissionAction::Allow,
            },

            PermissionRule {
                permission: "edit".to_string(),
                pattern: "*.env".to_string(),
                action: PermissionAction::Ask,
            },
            PermissionRule {
                permission: "edit".to_string(),
                pattern: "*.env.*".to_string(),
                action: PermissionAction::Ask,
            },
            PermissionRule {
                permission: "edit".to_string(),
                pattern: "secrets.toml".to_string(),
                action: PermissionAction::Ask,
            },

            PermissionRule {
                permission: "bash".to_string(),
                pattern: "*test*".to_string(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "bash".to_string(),
                pattern: "*build*".to_string(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "bash".to_string(),
                pattern: "*check*".to_string(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "bash".to_string(),
                pattern: "*lint*".to_string(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "bash".to_string(),
                pattern: "*clippy*".to_string(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "bash".to_string(),
                pattern: "*git*".to_string(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "bash".to_string(),
                pattern: "*worktree*".to_string(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "bash".to_string(),
                pattern: "*rm*".to_string(),
                action: PermissionAction::Ask,
            },
            PermissionRule {
                permission: "bash".to_string(),
                pattern: "*rmdir*".to_string(),
                action: PermissionAction::Ask,
            },

            PermissionRule {
                permission: "memory".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },

            PermissionRule {
                permission: "skills".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },

            PermissionRule {
                permission: "sessions".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },
        ]
    }
}
