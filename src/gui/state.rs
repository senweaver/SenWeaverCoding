// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Shared application state for the SenWeaverCoding desktop GUI.

/// Which top-level page is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Marketplace,
    Settings,
}

/// Agent interaction mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMode {
    Plan,
    Debug,
    Ask,
    Image,
}

/// Settings category in the left nav.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsCategory {
    General,
    Appearance,
    Models,
    Plugins,
    RulesSkills,
    ToolsMcps,
    Hooks,
    Network,
}

impl SettingsCategory {
    pub fn all() -> &'static [Self] {
        &[
            Self::General,
            Self::Appearance,
            Self::Models,
            Self::Plugins,
            Self::RulesSkills,
            Self::ToolsMcps,
            Self::Hooks,
            Self::Network,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Models => "Models",
            Self::Plugins => "Plugins",
            Self::RulesSkills => "Rules, Skills, Subagents",
            Self::ToolsMcps => "Tools & MCPs",
            Self::Hooks => "Hooks",
            Self::Network => "Network",
        }
    }
}

/// Marketplace category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketCategory {
    Featured,
    Infrastructure,
    DataAnalytics,
    Productivity,
    Payments,
    AgentOrchestration,
    Documentation,
    AllPlugins,
}

impl MarketCategory {
    pub fn all() -> &'static [Self] {
        &[
            Self::Featured,
            Self::Infrastructure,
            Self::DataAnalytics,
            Self::Productivity,
            Self::Payments,
            Self::AgentOrchestration,
            Self::Documentation,
            Self::AllPlugins,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Featured => "Featured",
            Self::Infrastructure => "Infrastructure",
            Self::DataAnalytics => "Data & Analytics",
            Self::Productivity => "Productivity",
            Self::Payments => "Payments",
            Self::AgentOrchestration => "Agent Orchestration",
            Self::Documentation => "Documentation",
            Self::AllPlugins => "All Plugins",
        }
    }
}

/// A single chat message in the conversation.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    ToolUse,
    ToolResult,
    System,
}

/// A plugin/skill card for the marketplace.
#[derive(Debug, Clone)]
pub struct PluginCard {
    pub name: String,
    pub description: String,
    pub category: String,
    pub icon_char: char,
}

/// Model entry for the model picker.
#[derive(Debug, Clone)]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    pub tier: String,
}

/// A conversation in the history sidebar.
#[derive(Debug, Clone)]
pub struct ConversationEntry {
    pub id: String,
    pub title: String,
}

/// Central application state — owned by `SenApp`.
pub struct AppState {
    pub current_page: Page,

    // -- Chat --
    pub chat_input: String,
    pub messages: Vec<ChatMessage>,
    pub conversations: Vec<ConversationEntry>,
    pub active_conversation_id: Option<String>,

    // -- Mode --
    pub active_modes: Vec<AgentMode>,
    pub show_mode_popup: bool,

    // -- Model --
    pub selected_model: String,
    pub model_search: String,
    pub max_mode: bool,
    pub show_model_picker: bool,
    pub available_models: Vec<ModelEntry>,

    // -- Marketplace --
    pub market_category: MarketCategory,
    pub market_search: String,
    pub plugins: Vec<PluginCard>,

    // -- Settings --
    pub settings_category: SettingsCategory,
    pub setting_notifications: bool,
    pub setting_warning_notifications: bool,
    pub setting_system_tray: bool,
    pub setting_completion_sound: bool,

    // -- Sidebar --
    pub sidebar_search: String,

    // -- Agent status --
    pub is_agent_busy: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            current_page: Page::Home,
            chat_input: String::new(),
            messages: Vec::new(),
            conversations: vec![
                ConversationEntry {
                    id: "1".into(),
                    title: "Project integration...".into(),
                },
            ],
            active_conversation_id: None,
            active_modes: vec![AgentMode::Plan],
            show_mode_popup: false,
            selected_model: "Opus 4.6".into(),
            model_search: String::new(),
            max_mode: false,
            show_model_picker: false,
            available_models: vec![
                ModelEntry { id: "composer-2".into(), name: "Composer 2".into(), tier: "Auto".into() },
                ModelEntry { id: "composer-2-fast".into(), name: "Composer 2 Fast".into(), tier: "Auto".into() },
                ModelEntry { id: "composer-1.5".into(), name: "Composer 1.5".into(), tier: "Auto".into() },
                ModelEntry { id: "gpt-5.3-codex".into(), name: "GPT-5.3 Codex".into(), tier: "Premium".into() },
                ModelEntry { id: "gpt-5.4".into(), name: "GPT-5.4".into(), tier: "Premium".into() },
                ModelEntry { id: "sonnet-4.6".into(), name: "Sonnet 4.6".into(), tier: "Premium".into() },
                ModelEntry { id: "opus-4.6".into(), name: "Opus 4.6".into(), tier: "Premium".into() },
            ],
            market_category: MarketCategory::Featured,
            market_search: String::new(),
            plugins: default_plugins(),
            settings_category: SettingsCategory::General,
            setting_notifications: true,
            setting_warning_notifications: false,
            setting_system_tray: true,
            setting_completion_sound: false,
            sidebar_search: String::new(),
            is_agent_busy: false,
        }
    }
}

fn default_plugins() -> Vec<PluginCard> {
    vec![
        PluginCard { name: "Datadog".into(), description: "Use Datadog directly in Cursor through a...".into(), category: "Featured".into(), icon_char: 'D' },
        PluginCard { name: "Slack".into(), description: "Slack MCP server. Search channels, send ...".into(), category: "Featured".into(), icon_char: 'S' },
        PluginCard { name: "Figma".into(), description: "Plugin that includes the Figma MCP serve...".into(), category: "Featured".into(), icon_char: 'F' },
        PluginCard { name: "Linear".into(), description: "Plugin for Linear - enables AI assis...".into(), category: "Featured".into(), icon_char: 'L' },
        PluginCard { name: "MongoDB".into(), description: "Official plugin for MongoDB (MCP...".into(), category: "Infrastructure".into(), icon_char: 'M' },
        PluginCard { name: "Firebase".into(), description: "The official Firebase plugin. Protot...".into(), category: "Infrastructure".into(), icon_char: 'F' },
        PluginCard { name: "Encore".into(), description: "Build backends in TypeScript and Go with...".into(), category: "Infrastructure".into(), icon_char: 'E' },
        PluginCard { name: "Render".into(), description: "Deploy, debug, and monitor applications...".into(), category: "Infrastructure".into(), icon_char: 'R' },
    ]
}
