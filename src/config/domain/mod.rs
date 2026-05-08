// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Domain-specific config sub-schemas.
//!
//! This module exists as the target location for the future `schema.rs`
//! split (P6.1).  The top-level `Config` struct will eventually aggregate
//! structs defined here, one per domain:
//!
//! - `agent_runtime`  — agent loop, compression, pacing, memory
//! - `gateway_net`    — HTTP gateway, auth levels, rate limits
//! - `providers_ext`  — per-provider overrides (API keys, model routes)
//! - `channels_ext`   — messaging integrations (slack, telegram, etc.)
//! - `tooling`        — tool surface controls (allowlists, cost caps)
//! - `security_ops`   — policies, sandbox, trust, audit
//!
//! Each sub-schema owns its own `Default`, `validate()`, and optional
//! `migrate_from_legacy()` so the monolithic `schema.rs` can be reduced
//! to an aggregator of these types without breaking existing TOMLs.

pub mod agent;
pub mod agent_runtime;
pub mod backup;
pub mod channels;
pub mod channels_core;
pub mod cloud_ops;
pub mod conversational_ai;
pub mod delegate_agents;
pub mod evolution;
pub mod gateway_net;
pub mod hardware;
pub mod heartbeat;
pub mod memory;
pub mod memory_runtime;
pub mod multimodal;
pub mod observability;
pub mod observability_ext;
pub mod pacing;
pub mod pipeline;
pub mod providers;
pub mod proxy;
pub mod rpc;
pub mod runtime;
pub mod security;
pub mod security_ops;
pub mod tools;
pub mod tools_ext;

pub use agent_runtime::AgentRuntimeExtras;
pub use memory_runtime::MemoryRuntimeExtras;

pub use proxy::{ProxyConfig, ProxyScope};

pub mod core;

pub use crate::config::schema::{
    AgentConfig, AutoIndexConfig, AutonomyConfig, BrowserConfig, ChannelsConfig, ClaudeCodeConfig,
    CodeRagConfig, CodeRagEmbedderConfig, Config, CostConfig, CronConfig, DelegateToolConfig,
    EstopConfig, FeishuConfig, GatewayConfig, HttpRequestConfig, IdentityConfig, KnowledgeConfig,
    LarkConfig, MatrixConfig, McpConfig, ModelPricing, ModelProviderConfig, ModelRouteConfig,
    NodesConfig, OtpConfig, PeripheralsConfig, PluginsConfig, ReliabilityConfig, SandboxConfig,
    SchedulerConfig, SecretsConfig, ShellToolConfig, SkillsConfig, SwarmConfig, SwarmStrategy,
    ToolFilterGroup, ToolFilterGroupMode, TtsConfig, WebFetchConfig, WebSearchConfig,
    WorkspaceConfig,
};
