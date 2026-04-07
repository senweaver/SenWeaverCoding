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

/// Permission action: allow, deny, or ask for confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionAction {
    /// Allow the operation without prompting
    Allow,
    /// Deny the operation
    Deny,
    /// Prompt the user for confirmation
    Ask,
}

impl Default for PermissionAction {
    fn default() -> Self {
        Self::Ask
    }
}

/// A single permission rule with glob pattern matching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    /// The permission/target to match (e.g., "edit", "bash", "read")
    pub permission: String,
    /// Glob pattern to match against (e.g., "*.env", "src/**/*.rs")
    #[serde(default = "default_pattern")]
    pub pattern: String,
    /// Action to take when the rule matches
    pub action: PermissionAction,
}

fn default_pattern() -> String {
    "*".to_string()
}

/// Configuration for permissions, supporting both simple and pattern-based rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PermissionConfig {
    /// Simple string-based permission (allow/deny/ask or pattern:action)
    Simple(String),
    /// Map of permission to action or pattern rules
    Pattern(HashMap<String, PatternRule>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PatternRule {
    /// Simple action string (allow/deny/ask)
    Action(PermissionAction),
    /// Map of glob patterns to actions
    Patterns(HashMap<String, PermissionAction>),
}

/// A compiled set of permission rules for efficient matching.
#[derive(Debug, Clone)]
pub struct PermissionRuleSet {
    /// Glob set for the permission patterns
    glob_set: Arc<GlobSet>,
    /// The rules in order
    rules: Vec<CompiledRule>,
    /// Map from permission name to rule indices
    permission_index: HashMap<String, Vec<usize>>,
}

#[derive(Debug, Clone)]
struct CompiledRule {
    /// The original rule
    rule: PermissionRule,
    /// Compiled glob matcher
    matcher: GlobMatcher,
}

/// Error type for permission rule operations.
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
    /// Create a new permission rule set from a list of rules.
    pub fn new(rules: Vec<PermissionRule>) -> Result<Self, PermissionError> {
        let mut builder = GlobSetBuilder::new();
        let mut compiled_rules = Vec::new();
        let mut permission_index: HashMap<String, Vec<usize>> = HashMap::new();

        for (idx, rule) in rules.iter().enumerate() {
            let glob = Glob::new(&rule.pattern).map_err(|e| PermissionError::InvalidPattern(e.to_string()))?;
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

        let glob_set = Arc::new(builder.build().map_err(|e| PermissionError::InvalidPattern(e.to_string()))?);

        Ok(Self {
            glob_set,
            rules: compiled_rules,
            permission_index,
        })
    }

    /// Create a permission rule set from configuration.
    pub fn from_config(config: &HashMap<String, PermissionConfig>) -> Result<Self, PermissionError> {
        let mut rules = Vec::new();

        for (permission, cfg) in config {
            match cfg {
                PermissionConfig::Simple(s) => {
                    // Handle "allow", "deny", "ask" or "pattern:action"
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
                        // Simple action for all patterns
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
                                // Use the first pattern's action or default
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

    /// Evaluate a permission request against the rules.
    pub fn evaluate(&self, permission: &str, target: &str) -> PermissionResult {
        // First check permission-specific rules
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

        // Then check wildcard rules
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

        // Default: ask for confirmation
        PermissionResult {
            action: PermissionAction::Ask,
            matched_rule: None,
        }
    }

    /// Check if a permission is allowed without prompting.
    pub fn is_allowed(&self, permission: &str, target: &str) -> bool {
        matches!(self.evaluate(permission, target).action, PermissionAction::Allow)
    }

    /// Check if a permission is denied.
    pub fn is_denied(&self, permission: &str, target: &str) -> bool {
        matches!(self.evaluate(permission, target).action, PermissionAction::Deny)
    }

    /// Check if a permission requires user confirmation.
    pub fn requires_confirmation(&self, permission: &str, target: &str) -> bool {
        matches!(self.evaluate(permission, target).action, PermissionAction::Ask)
    }
}

/// Result of evaluating a permission request.
#[derive(Debug, Clone)]
pub struct PermissionResult {
    /// The action to take
    pub action: PermissionAction,
    /// The rule that matched (if any)
    pub matched_rule: Option<PermissionRule>,
}

/// Permission request context.
#[derive(Debug, Clone)]
pub struct PermissionRequest {
    /// The tool/permission being requested
    pub permission: String,
    /// The target to access (e.g., file path)
    pub target: String,
    /// Additional metadata about the request
    pub metadata: HashMap<String, serde_json::Value>,
}

impl PermissionRequest {
    /// Create a new permission request.
    pub fn new(permission: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            permission: permission.into(),
            target: target.into(),
            metadata: HashMap::new(),
        }
    }

    /// Add metadata to the request.
    pub fn with_metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    /// Get the file extension from the target, if any.
    pub fn file_extension(&self) -> Option<String> {
        Path::new(&self.target)
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| format!("*.{}", s))
    }
}

/// Build a permission rule set from common configurations.
pub mod builder {
    use super::*;

    /// Create default rules matching OpenCode's behavior.
    pub fn default_rules() -> Vec<PermissionRule> {
        vec![
            // Edit permissions
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
            // Read permissions (generally allowed)
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
            // Bash permissions
            PermissionRule {
                permission: "bash".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Ask,
            },
            // External directory access
            PermissionRule {
                permission: "external_directory".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Ask,
            },
            // Loop detection
            PermissionRule {
                permission: "doom_loop".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Ask,
            },
        ]
    }

    /// Create a restrictive rule set for plan mode.
    pub fn plan_mode_rules() -> Vec<PermissionRule> {
        vec![
            // Read is allowed
            PermissionRule {
                permission: "read".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },
            // Edit is denied by default
            PermissionRule {
                permission: "edit".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Deny,
            },
            // But allow creating plan files
            PermissionRule {
                permission: "edit".to_string(),
                pattern: ".opencode/plans/*.md".to_string(),
                action: PermissionAction::Allow,
            },
            // Bash requires confirmation
            PermissionRule {
                permission: "bash".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Ask,
            },
            // Question is allowed
            PermissionRule {
                permission: "question".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },
        ]
    }

    /// Create a permissive rule set for build mode.
    pub fn build_mode_rules() -> Vec<PermissionRule> {
        vec![
            // All edits allowed
            PermissionRule {
                permission: "edit".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },
            // Read allowed
            PermissionRule {
                permission: "read".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },
            // Bash allowed
            PermissionRule {
                permission: "bash".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },
            // External directory requires confirmation
            PermissionRule {
                permission: "external_directory".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Ask,
            },
            // Question allowed
            PermissionRule {
                permission: "question".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },
            // Plan mode transitions allowed
            PermissionRule {
                permission: "plan_enter".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },
        ]
    }

    /// Create engineering-grade rules for Harness mode — spec files, skills, state, test files allowed.
    pub fn harness_mode_rules() -> Vec<PermissionRule> {
        vec![
            // Read: everything allowed
            PermissionRule {
                permission: "read".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },
            // Edit: source files allowed, sensitive files ask
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
            // Harness spec and state files
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
            // Sensitive files: ask
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
            // Bash: allow build/test, ask for destructive
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
            // Memory persistence allowed
            PermissionRule {
                permission: "memory".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },
            // Skills allowed
            PermissionRule {
                permission: "skills".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },
            // Session management allowed
            PermissionRule {
                permission: "sessions".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_allow_deny() {
        let rules = vec![
            PermissionRule {
                permission: "edit".to_string(),
                pattern: "*.rs".to_string(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "edit".to_string(),
                pattern: "*.md".to_string(),
                action: PermissionAction::Deny,
            },
        ];

        let rule_set = PermissionRuleSet::new(rules).unwrap();

        assert!(rule_set.is_allowed("edit", "src/main.rs"));
        assert!(rule_set.is_denied("edit", "README.md"));
        assert!(rule_set.requires_confirmation("edit", "src/lib.py"));
    }

    #[test]
    fn test_wildcard_matching() {
        let rules = vec![
            PermissionRule {
                permission: "read".to_string(),
                pattern: "*.env".to_string(),
                action: PermissionAction::Ask,
            },
            PermissionRule {
                permission: "read".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },
        ];

        let rule_set = PermissionRuleSet::new(rules).unwrap();

        assert!(rule_set.requires_confirmation("read", ".env"));
        assert!(rule_set.is_allowed("read", "src/main.rs"));
    }

    #[test]
    fn test_permission_specific_rules() {
        let rules = vec![
            PermissionRule {
                permission: "edit".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "bash".to_string(),
                pattern: "*".to_string(),
                action: PermissionAction::Ask,
            },
        ];

        let rule_set = PermissionRuleSet::new(rules).unwrap();

        assert!(rule_set.is_allowed("edit", "src/main.rs"));
        assert!(rule_set.requires_confirmation("bash", "ls"));
    }

    #[test]
    fn test_default_rules() {
        let rules = builder::default_rules();
        let rule_set = PermissionRuleSet::new(rules).unwrap();

        // .env files should require confirmation for edit
        assert!(rule_set.requires_confirmation("edit", ".env"));
        assert!(rule_set.requires_confirmation("edit", ".env.local"));

        // Regular files should be allowed
        assert!(rule_set.is_allowed("edit", "src/main.rs"));
        assert!(rule_set.is_allowed("read", "src/main.rs"));
    }

    #[test]
    fn test_plan_mode_rules() {
        let rules = builder::plan_mode_rules();
        let rule_set = PermissionRuleSet::new(rules).unwrap();

        // Read is allowed
        assert!(rule_set.is_allowed("read", "src/main.rs"));

        // Edit is denied
        assert!(rule_set.is_denied("edit", "src/main.rs"));

        // But plan files can be created
        assert!(rule_set.is_allowed("edit", ".opencode/plans/my-plan.md"));
    }

    #[test]
    fn test_build_mode_rules() {
        let rules = builder::build_mode_rules();
        let rule_set = PermissionRuleSet::new(rules).unwrap();

        // Everything is more permissive
        assert!(rule_set.is_allowed("edit", "src/main.rs"));
        assert!(rule_set.is_allowed("read", ".env"));
        assert!(rule_set.is_allowed("bash", "cargo build"));
        assert!(rule_set.requires_confirmation("external_directory", "/tmp"));
    }

    #[test]
    fn test_harness_mode_rules() {
        let rules = builder::harness_mode_rules();
        let rule_set = PermissionRuleSet::new(rules).unwrap();

        // Source files allowed
        assert!(rule_set.is_allowed("edit", "src/main.rs"));
        assert!(rule_set.is_allowed("edit", "src/lib.ts"));
        assert!(rule_set.is_allowed("edit", "lib/main.py"));
        // MD/state files allowed
        assert!(rule_set.is_allowed("edit", "STATE.md"));
        assert!(rule_set.is_allowed("edit", "ROADMAP.md"));
        assert!(rule_set.is_allowed("edit", ".opencode/plans/plan.md"));
        assert!(rule_set.is_allowed("edit", ".trellis/tasks/task.md"));
        // Sensitive files ask
        assert!(rule_set.requires_confirmation("edit", ".env"));
        assert!(rule_set.requires_confirmation("edit", ".env.local"));
        // Bash: build/test allowed, destructive asks
        assert!(rule_set.is_allowed("bash", "cargo test"));
        assert!(rule_set.is_allowed("bash", "cargo build"));
        assert!(rule_set.is_allowed("bash", "cargo clippy"));
        assert!(rule_set.is_allowed("bash", "git status"));
        assert!(rule_set.is_allowed("bash", "git worktree add"));
        assert!(rule_set.requires_confirmation("bash", "rm -rf"));
        // Memory/sessions/skills allowed
        assert!(rule_set.is_allowed("memory", "store key value"));
        assert!(rule_set.is_allowed("skills", "read_skill brainstorming"));
        assert!(rule_set.is_allowed("sessions", "sessions_list"));
    }
}
