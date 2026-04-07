// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Auto-mode configuration — manage auto-approve rules.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Auto-mode rule for automatic tool approval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoModeRule {
    pub tool: String,
    pub action: AutoModeAction,
    #[serde(default)]
    pub condition: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoModeAction {
    Allow,
    Deny,
    Ask,
}

/// Show current auto-mode defaults.
pub fn show_defaults() {
    println!("Auto-mode defaults:");
    println!("  file_read      -> allow");
    println!("  glob_search    -> allow");
    println!("  content_search -> allow");
    println!("  dir_list       -> allow");
    println!("  file_write     -> ask");
    println!("  file_edit      -> ask");
    println!("  shell          -> ask");
    println!("  notebook_edit  -> ask");
}

/// Show auto-mode configuration from workspace.
pub async fn show_config(workspace: &Path) -> Result<()> {
    let config_path = workspace.join(".senweavercoding").join("auto_mode.json");
    if !config_path.exists() {
        println!("No custom auto-mode configuration. Using defaults.");
        show_defaults();
        return Ok(());
    }

    let content = tokio::fs::read_to_string(&config_path).await?;
    let rules: Vec<AutoModeRule> = serde_json::from_str(&content)?;

    println!("Custom auto-mode rules:");
    for rule in &rules {
        println!(
            "  {:<20} -> {:?} {}",
            rule.tool,
            rule.action,
            rule.condition.as_deref().unwrap_or("")
        );
    }

    Ok(())
}

/// Critique the auto-mode configuration for potential issues.
pub fn critique_config(rules: &[AutoModeRule]) -> Vec<String> {
    let mut warnings = Vec::new();

    for rule in rules {
        if rule.action == AutoModeAction::Allow && rule.tool == "shell" && rule.condition.is_none()
        {
            warnings
                .push("Unconditional shell allow is dangerous — consider adding conditions".into());
        }
        if rule.action == AutoModeAction::Allow && rule.tool == "*" {
            warnings.push("Wildcard allow bypasses all permission checks".into());
        }
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn critique_warns_on_unconditional_shell() {
        let rules = vec![AutoModeRule {
            tool: "shell".into(),
            action: AutoModeAction::Allow,
            condition: None,
        }];
        let warnings = critique_config(&rules);
        assert!(!warnings.is_empty());
    }

    #[test]
    fn critique_warns_on_wildcard_allow() {
        let rules = vec![AutoModeRule {
            tool: "*".into(),
            action: AutoModeAction::Allow,
            condition: None,
        }];
        let warnings = critique_config(&rules);
        assert!(!warnings.is_empty());
    }

    #[test]
    fn critique_clean_config() {
        let rules = vec![AutoModeRule {
            tool: "file_read".into(),
            action: AutoModeAction::Allow,
            condition: None,
        }];
        let warnings = critique_config(&rules);
        assert!(warnings.is_empty());
    }
}
