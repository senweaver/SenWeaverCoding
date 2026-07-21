// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolGroup {

    pub name: String,

    #[serde(default)]
    pub description: String,

    pub tools: Vec<String>,

    #[serde(default = "default_true")]
    pub enabled: bool,

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

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ToolGroupsConfig {

    #[serde(default)]
    pub groups: Vec<ToolGroup>,

    #[serde(default)]
    pub active_groups: Vec<String>,
}

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
                    "web_search_tool".into(),
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
