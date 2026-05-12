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

pub mod service;

#[allow(unused_imports)]
pub mod analytics;
#[allow(unused_imports)]
pub mod api;
#[allow(unused_imports)]
pub mod compact;
#[allow(unused_imports)]
pub mod container;
#[allow(unused_imports)]
pub mod credential_vault;
#[allow(unused_imports)]
pub mod diagnostics;
#[allow(unused_imports)]
pub mod lsp;

pub mod lsp_incremental;
pub mod lsp_pool;

pub mod lsp_rename;
#[allow(unused_imports)]
pub mod mcp_manager;

pub mod mcp_server;
#[allow(unused_imports)]
pub mod notifier;
#[allow(unused_imports)]
pub mod oauth;
#[allow(unused_imports)]
pub mod rate_limit;
#[allow(unused_imports)]
pub mod session_memory;
#[allow(unused_imports)]
pub mod tips;
#[allow(unused_imports)]
pub mod token_estimation;
#[allow(unused_imports)]
pub mod voice_stt;

#[allow(unused_imports)]
pub mod agent_summary;
#[allow(unused_imports)]
pub mod auto_dream;
#[allow(unused_imports)]
pub mod extract_memories;
#[allow(unused_imports)]
pub mod magic_docs;
#[allow(unused_imports)]
pub mod plugin_service;
#[allow(unused_imports)]
pub mod policy_limits;
#[allow(unused_imports)]
pub mod prevent_sleep;
#[allow(unused_imports)]
pub mod prompt_suggestion;
#[allow(unused_imports)]
pub mod settings_sync;
#[allow(unused_imports)]
pub mod task_manager;
#[allow(unused_imports)]
pub mod team_memory_sync;
#[allow(unused_imports)]
pub mod tool_activation_store;
#[allow(unused_imports)]
pub mod tool_use_summary;

#[allow(unused_imports)]
pub use container::{
    RuntimeFlags, ServiceContainer, ServiceContainerConfig, ToolSearchMetricsSnapshot,
    get_services, init_services, try_get_services,
};

#[allow(unused_imports)]
pub use tool_activation_store::{ToolActivationRecord, ToolActivationStore, ToolActivationStoreHandle};

#[allow(unused_imports)]
pub use compact::CompactService;

#[allow(unused_imports)]
pub use credential_vault::{
    init_credential_vault, redact_args_optional, redact_for_audit_optional,
    try_get_credential_vault, CredentialKind, CredentialMeta, CredentialVault,
};

#[allow(unused_imports)]
pub use lsp::{ServerInfo, ServerKey};
