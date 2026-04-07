// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Services module — mirrors claude-code's `services/` directory.
//
// Provides the service layer that sits between the agent core and external
// integrations: analytics, API client wrappers, compaction, LSP, MCP
// management, OAuth, rate limiting, token estimation, voice STT,
// diagnostics, notifications, plugins, and tips.

pub mod analytics;
pub mod api;
pub mod compact;
pub mod diagnostics;
pub mod lsp;
pub mod mcp_manager;
pub mod notifier;
pub mod oauth;
pub mod rate_limit;
pub mod session_memory;
pub mod tips;
pub mod token_estimation;
pub mod voice_stt;

// -- Additional services ported from claude-code-typescript-srcaudit --
pub mod agent_summary;
pub mod auto_dream;
pub mod extract_memories;
pub mod magic_docs;
pub mod plugin_service;
pub mod policy_limits;
pub mod prevent_sleep;
pub mod prompt_suggestion;
pub mod settings_sync;
pub mod task_manager;
pub mod team_memory_sync;
pub mod tool_use_summary;

pub use agent_summary::AgentSummaryService;
pub use analytics::AnalyticsService;
pub use auto_dream::AutoDreamService;
pub use compact::CompactService;
pub use lsp::LspService;
pub use mcp_manager::McpManager;
pub use notifier::Notifier;
pub use oauth::OAuthService;
pub use plugin_service::PluginService;
pub use policy_limits::PolicyLimitsService;
pub use prompt_suggestion::PromptSuggestionService;
pub use rate_limit::RateLimiter;
pub use session_memory::SessionMemoryService;
pub use settings_sync::SettingsSyncService;
pub use team_memory_sync::TeamMemorySyncService;
pub use token_estimation::TokenEstimator;
pub use tool_use_summary::ToolUseSummaryService;

// -- Service container (wires all services for agent runtime) --
pub mod container;
pub use container::{ServiceContainer, ServiceContainerConfig, get_services, init_services};
