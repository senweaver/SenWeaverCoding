// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
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
pub mod pii_sanitizer;
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

pub mod proxy_registry;

pub mod proxy_runtime;
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
pub use pii_sanitizer::{
    global_sanitizer, sanitize_json as pii_sanitize_json, sanitize_text as pii_sanitize_text,
    sanitize_text_in_place as pii_sanitize_text_in_place, update_global_config as update_pii_config,
    PiiKind, PiiSanitizer, PiiSanitizerConfig, SanitizationReport,
};

#[allow(unused_imports)]
pub use lsp::{ServerInfo, ServerKey};
