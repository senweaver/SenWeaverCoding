// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
pub mod service;

pub mod assist;
pub mod governance;
pub mod memory;

pub mod api;
pub mod compact;
pub mod container;
pub mod lsp;
pub mod mcp_manager;

pub mod mcp_server;

pub mod proxy;
pub mod oauth;

pub mod token_estimation;
pub mod voice_stt;

pub mod agent_summary;
pub mod auto_dream;
pub mod magic_docs;
pub mod plugin_service;
pub mod prevent_sleep;
pub mod prompt_suggestion;
pub mod settings_sync;
pub mod team_runtime;
pub mod team_store;
pub mod template_library;
pub mod tool_telemetry;

pub use container::{
    RuntimeFlags, ServiceContainer, ServiceContainerConfig, ToolSearchMetricsSnapshot,
    get_services, init_services, require_services, try_get_services,
};

pub use tool_telemetry::activation_store::{
    ToolActivationRecord, ToolActivationStore, ToolActivationStoreHandle,
};

pub use template_library::{
    content_hash as template_library_content_hash, TemplateKind, TemplateLibraryStore,
};

pub use compact::CompactService;

pub use governance::credential_vault::{
    init_credential_vault, redact_args_optional, redact_for_audit_optional,
    try_get_credential_vault, CredentialKind, CredentialMeta, CredentialVault,
};

pub use governance::pii_sanitizer::{
    global_sanitizer, sanitize_json as pii_sanitize_json, sanitize_text as pii_sanitize_text,
    sanitize_text_in_place as pii_sanitize_text_in_place, update_global_config as update_pii_config,
    PiiKind, PiiSanitizer, PiiSanitizerConfig, SanitizationReport,
};

pub use lsp::{ServerInfo, ServerKey};
