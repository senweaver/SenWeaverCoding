// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Tool Groups - named tool groups for organized tool management.
//!
//! Allows tools to be organized into named groups that can be
//! selectively activated per agent, session, or context.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A named group of tools.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolGroup {
    /// Group name (e.g., "research", "coding", "memory").
    pub name: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
    /// Tool names in this group.
    pub tools: Vec<String>,
    /// Whether this group is enabled by default.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Priority (higher = loaded first). Default: 0.
    #[serde(default)]
    pub priority: i32,
}

fn default_true() -> bool {
    true
}

impl ToolGroup {
    pub fn new(name: impl Into<String>, tools: Vec<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            tools,
            enabled: true,
            priority: 0,
        }
    }
}

/// Tool groups configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ToolGroupsConfig {
    /// Defined tool groups.
    #[serde(default)]
    pub groups: Vec<ToolGroup>,
    /// Active groups for the current session (empty = all enabled groups).
    #[serde(default)]
    pub active_groups: Vec<String>,
}

/// Registry managing tool groups.
pub struct ToolGroupRegistry {
    groups: HashMap<String, ToolGroup>,
    active: Vec<String>,
}

impl ToolGroupRegistry {
    pub fn new() -> Self {
        Self {
            groups: HashMap::new(),
            active: Vec::new(),
        }
    }

    pub fn from_config(config: &ToolGroupsConfig) -> Self {
        let mut registry = Self::new();
        for group in &config.groups {
            registry.register(group.clone());
        }
        if config.active_groups.is_empty() {
            registry.active = registry
                .groups
                .values()
                .filter(|g| g.enabled)
                .map(|g| g.name.clone())
                .collect();
        } else {
            registry.active = config.active_groups.clone();
        }
        registry
    }

    pub fn register(&mut self, group: ToolGroup) {
        self.groups.insert(group.name.clone(), group);
    }

    pub fn activate_group(&mut self, name: &str) {
        if self.groups.contains_key(name) && !self.active.contains(&name.to_string()) {
            self.active.push(name.to_string());
        }
    }

    pub fn deactivate_group(&mut self, name: &str) {
        self.active.retain(|g| g != name);
    }

    /// Get all tool names from active groups.
    pub fn active_tools(&self) -> Vec<String> {
        let mut tools = Vec::new();
        let mut sorted_groups: Vec<&ToolGroup> = self
            .active
            .iter()
            .filter_map(|name| self.groups.get(name))
            .collect();
        sorted_groups.sort_by(|a, b| b.priority.cmp(&a.priority));

        for group in sorted_groups {
            for tool in &group.tools {
                if !tools.contains(tool) {
                    tools.push(tool.clone());
                }
            }
        }
        tools
    }

    /// Check if a tool is in any active group.
    pub fn is_tool_active(&self, tool_name: &str) -> bool {
        self.active.iter().any(|group_name| {
            self.groups
                .get(group_name)
                .map_or(false, |g| g.tools.iter().any(|t| t == tool_name))
        })
    }

    pub fn list_groups(&self) -> Vec<&ToolGroup> {
        let mut groups: Vec<&ToolGroup> = self.groups.values().collect();
        groups.sort_by(|a, b| a.name.cmp(&b.name));
        groups
    }

    pub fn active_group_names(&self) -> &[String] {
        &self.active
    }

    /// Create default built-in groups.
    pub fn with_defaults(mut self) -> Self {
        let defaults = vec![
            ToolGroup {
                name: "core".to_string(),
                description: "Essential tools (shell, file operations, search)".to_string(),
                tools: vec![
                    "shell".into(),
                    "file_read".into(),
                    "file_write".into(),
                    "file_edit".into(),
                    "notebook_edit".into(),
                    "dir_list".into(),
                    "glob_search".into(),
                    "content_search".into(),
                    "present_files".into(),
                    "view_image".into(),
                ],
                enabled: true,
                priority: 100,
            },
            ToolGroup {
                name: "memory".to_string(),
                description: "Memory management tools".to_string(),
                tools: vec![
                    "memory_store".into(),
                    "memory_recall".into(),
                    "memory_forget".into(),
                    "memory_export".into(),
                ],
                enabled: true,
                priority: 80,
            },
            ToolGroup {
                name: "web".to_string(),
                description: "Web research and browsing tools".to_string(),
                tools: vec![
                    "web_search".into(),
                    "multi_search".into(),
                    "web_fetch".into(),
                    "image_search".into(),
                    "youtube_search".into(),
                    "github_search".into(),
                    "reddit_search".into(),
                    "text_browser".into(),
                    "browser_open".into(),
                ],
                enabled: true,
                priority: 70,
            },
            ToolGroup {
                name: "scheduling".to_string(),
                description: "Cron and scheduling tools".to_string(),
                tools: vec![
                    "cron_add".into(),
                    "cron_list".into(),
                    "cron_remove".into(),
                    "schedule".into(),
                ],
                enabled: true,
                priority: 50,
            },
            ToolGroup {
                name: "delegation".to_string(),
                description: "Sub-agent delegation and swarm tools".to_string(),
                tools: vec![
                    "delegate".into(),
                    "swarm".into(),
                    "llm_task".into(),
                    "setup_agent".into(),
                ],
                enabled: true,
                priority: 60,
            },
            ToolGroup {
                name: "devtools".to_string(),
                description: "Development-related tools".to_string(),
                tools: vec![
                    "git_operations".into(),
                    "claude_code".into(),
                    "codex_cli".into(),
                    "project_intel".into(),
                ],
                enabled: false,
                priority: 40,
            },
        ];

        for group in defaults {
            if !self.groups.contains_key(&group.name) {
                self.register(group);
            }
        }
        self
    }
}

impl Default for ToolGroupRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_basics() {
        let mut registry = ToolGroupRegistry::new();
        registry.register(ToolGroup::new(
            "test",
            vec!["tool_a".into(), "tool_b".into()],
        ));
        registry.activate_group("test");

        assert!(registry.is_tool_active("tool_a"));
        assert!(!registry.is_tool_active("tool_c"));
        assert_eq!(registry.active_tools().len(), 2);
    }

    #[test]
    fn test_deactivate() {
        let mut registry = ToolGroupRegistry::new();
        registry.register(ToolGroup::new("test", vec!["tool_a".into()]));
        registry.activate_group("test");
        assert!(registry.is_tool_active("tool_a"));

        registry.deactivate_group("test");
        assert!(!registry.is_tool_active("tool_a"));
    }

    #[test]
    fn test_priority_ordering() {
        let mut registry = ToolGroupRegistry::new();
        registry.register(ToolGroup {
            name: "low".to_string(),
            tools: vec!["tool_low".into()],
            priority: 1,
            ..ToolGroup::new("low", vec![])
        });
        registry.register(ToolGroup {
            name: "high".to_string(),
            tools: vec!["tool_high".into()],
            priority: 100,
            ..ToolGroup::new("high", vec![])
        });
        registry.activate_group("low");
        registry.activate_group("high");

        let tools = registry.active_tools();
        assert_eq!(tools[0], "tool_high");
    }

    #[test]
    fn test_defaults() {
        let registry = ToolGroupRegistry::new().with_defaults();
        assert!(registry.groups.contains_key("core"));
        assert!(registry.groups.contains_key("memory"));
        assert!(registry.groups.contains_key("web"));
    }

    #[test]
    fn test_from_config() {
        let config = ToolGroupsConfig {
            groups: vec![ToolGroup::new("custom", vec!["my_tool".into()])],
            active_groups: vec!["custom".into()],
        };
        let registry = ToolGroupRegistry::from_config(&config);
        assert!(registry.is_tool_active("my_tool"));
    }
}
