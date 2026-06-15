// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod api;
pub mod auth_rate_limit;
pub mod canvas;
pub mod nodes;
pub mod routes;
pub mod sse;

pub mod tls;
pub mod ws;
pub mod credential_routes;
pub mod desktop_bridge;
pub mod desktop_routes;
pub mod evolution_routes;
pub mod git_routes;
pub mod mcp_live;
pub mod python_env_routes;
pub mod workspace_files;

pub mod a2a;
pub mod client_ip;
pub mod cors;
pub mod hardware_context;
pub mod lifecycle;
pub mod channel_supervisor;
pub mod rate_limit;

pub use crate::gateway::lifecycle::{
    StartupWarning, is_fully_stopped, is_running, is_shutdown_requested, push_startup_warning,
    request_embedded_shutdown, request_shutdown, snapshot_startup_warnings, wait_embedded_stopped,
};
use crate::gateway::lifecycle::GatewayRunningGuard;

use crate::channels::{
    Channel, GmailPushChannel, LinqChannel, NextcloudTalkChannel, SendMessage, WatiChannel,
    WhatsAppChannel, session::backend::SessionBackend, session::sqlite::SqliteSessionBackend,
};
use crate::config::Config;
use crate::cost::CostTracker;
use crate::memory::{self, Memory, MemoryCategory};
use crate::providers::{self, ChatMessage, Provider};
use crate::runtime;
use crate::security::SecurityPolicy;
use crate::security::pairing::{PairingGuard, constant_time_eq, is_public_bind};
use crate::tools;
use crate::tools::canvas::CanvasStore;
use crate::tools::traits::ToolSpec;
use crate::util::truncate_with_ellipsis;
use anyhow::{Context, Result};
use axum::{
    Router,
    body::Bytes,
    extract::{ConnectInfo, Query, State},
    http::{HeaderMap, StatusCode, header},
    middleware,
    response::{IntoResponse, Json},
    routing::{delete, get, post, put},
};
use parking_lot::Mutex;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;
use uuid::Uuid;

pub const MAX_BODY_SIZE: usize = 65_536;

static GATEWAY_EVENT_TX: std::sync::OnceLock<
    tokio::sync::broadcast::Sender<serde_json::Value>,
> = std::sync::OnceLock::new();

pub fn install_gateway_event_tx(tx: tokio::sync::broadcast::Sender<serde_json::Value>) {
    let _ = GATEWAY_EVENT_TX.set(tx);
}

pub fn emit_gateway_event(payload: serde_json::Value) {
    if let Some(tx) = GATEWAY_EVENT_TX.get() {
        let _ = tx.send(payload);
    }
}

pub fn emit_session_task_notification(
    session_id: &str,
    data: serde_json::Value,
) {
    emit_gateway_event(serde_json::json!({
        "type": "system_notification",
        "subtype": "task_notification",
        "sessionId": session_id,
        "data": data,
    }));
}

pub fn emit_session_task_update(
    session_id: &str,
    task_id: &str,
    status: &str,
    progress: Option<&str>,
) {
    let mut payload = serde_json::json!({
        "type": "task_update",
        "sessionId": session_id,
        "taskId": task_id,
        "status": status,
    });
    if let Some(progress) = progress.filter(|p| !p.trim().is_empty()) {
        payload["progress"] = serde_json::Value::String(progress.to_string());
    }
    emit_gateway_event(payload);
}

pub const REQUEST_TIMEOUT_SECS: u64 = 30;

pub fn gateway_request_timeout_secs() -> u64 {
    std::env::var("SEN_GATEWAY_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(REQUEST_TIMEOUT_SECS)
}

use crate::gateway::cors::desktop_cors_layer;

pub const RATE_LIMIT_WINDOW_SECS: u64 = 60;

pub const RATE_LIMIT_MAX_KEYS_DEFAULT: usize = 10_000;

pub const IDEMPOTENCY_MAX_KEYS_DEFAULT: usize = 10_000;

fn webhook_memory_key() -> String {
    format!("webhook_msg_{}", Uuid::new_v4())
}

fn effective_model_names_for_profile(profile: &crate::config::ModelProviderConfig) -> Vec<String> {
    if !profile.model_names.is_empty() {
        return profile
            .model_names
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut out: Vec<String> = Vec::new();
    for slot in ["main", "haiku", "sonnet", "opus"] {
        if let Some(value) = profile.models.get(slot) {
            let trimmed = value.trim();
            if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
                out.push(trimmed.to_string());
            }
        }
    }
    for value in profile.models.values() {
        let trimmed = value.trim();
        if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
            out.push(trimmed.to_string());
        }
    }
    out
}

fn provider_runtime_options_for(
    profile: &crate::config::ModelProviderConfig,
    config: &crate::config::schema::Config,
) -> providers::ProviderRuntimeOptions {
    let mut merged_headers = providers::merged_extra_headers_for_config(config);
    let profile_headers =
        crate::config::build_custom_headers_map(&profile.custom_headers);
    for (k, v) in profile_headers {
        merged_headers.insert(k, v);
    }
    providers::ProviderRuntimeOptions {
        auth_profile_override: None,
        provider_api_url: profile.base_url.clone().or_else(|| config.api_url.clone()),
        sen_dir: config.config_path.parent().map(std::path::PathBuf::from),
        secrets_encrypt: config.secrets.encrypt,
        reasoning_enabled: config.runtime.reasoning_enabled,
        reasoning_effort: config.runtime.reasoning_effort.clone(),
        provider_timeout_secs: Some(config.provider_timeout_secs),
        extra_headers: merged_headers,
        api_path: profile.api_path.clone().or_else(|| config.api_path.clone()),
        provider_max_tokens: profile.max_tokens.or(config.provider_max_tokens),
        model_context_windows: profile.model_context_windows.clone(),
    }
}

fn collect_registered_models_for_engine(
    config: &crate::config::schema::Config,
) -> Vec<crate::evolution::RegisteredModel> {
    let mut out: Vec<crate::evolution::RegisteredModel> = Vec::new();
    for (pid, profile) in config.model_providers.iter() {
        for name in effective_model_names_for_profile(profile) {
            out.push(crate::evolution::RegisteredModel {
                provider_id: pid.clone(),
                model: name,
            });
        }
    }
    out
}

fn register_per_provider_reflection_factories(
    engine: &std::sync::Arc<crate::evolution::EvolutionEngine>,
    config: &crate::config::schema::Config,
) {
    for (pid, profile) in config.model_providers.iter() {
        let names = effective_model_names_for_profile(profile);
        if names.is_empty() {
            continue;
        }
        let credential = profile.api_key.as_deref();
        let runtime_options = provider_runtime_options_for(profile, config);
        let provider_url = profile.base_url.as_deref();
        match providers::create_provider_with_url_and_options(
            pid,
            credential,
            provider_url,
            &runtime_options,
        ) {
            Ok(boxed) => {
                let default_model = names.first().cloned().unwrap_or_default();
                let arc_provider: std::sync::Arc<dyn providers::Provider> =
                    std::sync::Arc::from(boxed);
                engine.register_reflection_provider(
                    pid,
                    crate::evolution::JudgeProviderRef {
                        provider: arc_provider,
                        model: default_model,
                    },
                );
            }
            Err(error) => {
                tracing::debug!(
                    provider_id = pid.as_str(),
                    error = %error,
                    "skip reflection provider construction"
                );
            }
        }
    }
}

fn whatsapp_memory_key(msg: &crate::channels::traits::ChannelMessage) -> String {
    format!("whatsapp_{}_{}", msg.sender, msg.id)
}

fn linq_memory_key(msg: &crate::channels::traits::ChannelMessage) -> String {
    format!("linq_{}_{}", msg.sender, msg.id)
}

fn wati_memory_key(msg: &crate::channels::traits::ChannelMessage) -> String {
    format!("wati_{}_{}", msg.sender, msg.id)
}

fn nextcloud_talk_memory_key(msg: &crate::channels::traits::ChannelMessage) -> String {
    format!("nextcloud_talk_{}_{}", msg.sender, msg.id)
}

fn sender_session_id(channel: &str, msg: &crate::channels::traits::ChannelMessage) -> String {
    match &msg.thread_ts {
        Some(thread_id) => format!("{channel}_{thread_id}_{}", msg.sender),
        None => format!("{channel}_{}", msg.sender),
    }
}

fn webhook_session_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get("X-Session-Id")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn hash_webhook_secret(value: &str) -> String {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(value.as_bytes());
    hex::encode(digest)
}

pub use crate::gateway::rate_limit::{GatewayRateLimiter, IdempotencyStore};

use crate::gateway::client_ip::{client_key_from_request, normalize_max_keys};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Mutex<Config>>,

    pub live_config: crate::config::live::LiveConfig,

    pub provider: Arc<parking_lot::RwLock<Arc<dyn Provider>>>,

    pub model: Arc<parking_lot::RwLock<String>>,
    pub temperature: f64,
    pub mem: Arc<dyn Memory>,
    pub auto_save: bool,

    pub webhook_secret_hash: Option<Arc<str>>,
    pub pairing: Arc<PairingGuard>,
    pub trust_forwarded_headers: bool,
    pub rate_limiter: Arc<GatewayRateLimiter>,
    pub auth_limiter: Arc<auth_rate_limit::AuthRateLimiter>,
    pub idempotency_store: Arc<IdempotencyStore>,
    pub whatsapp: Option<Arc<WhatsAppChannel>>,

    pub whatsapp_app_secret: Option<Arc<str>>,
    pub linq: Option<Arc<LinqChannel>>,

    pub linq_signing_secret: Option<Arc<str>>,
    pub nextcloud_talk: Option<Arc<NextcloudTalkChannel>>,

    pub nextcloud_talk_webhook_secret: Option<Arc<str>>,
    pub wati: Option<Arc<WatiChannel>>,

    pub gmail_push: Option<Arc<GmailPushChannel>>,

    pub observer: Arc<dyn crate::observability::Observer>,

    pub tools_registry: Arc<Vec<ToolSpec>>,

    pub cost_tracker: Option<Arc<CostTracker>>,

    pub event_tx: tokio::sync::broadcast::Sender<serde_json::Value>,

    pub shutdown_tx: tokio::sync::watch::Sender<bool>,

    pub node_registry: Arc<nodes::NodeRegistry>,

    pub path_prefix: String,

    pub session_backend: Option<Arc<dyn SessionBackend>>,

    pub device_registry: Option<Arc<api::pairing::DeviceRegistry>>,

    pub rbac: Option<Arc<crate::security::rbac::RbacEngine>>,

    pub canvas_store: CanvasStore,

    #[cfg(feature = "webauthn")]
    pub webauthn: Option<Arc<api::webauthn::WebAuthnState>>,

    pub hooks: Arc<crate::hooks::HotHookRunner>,

    pub lsp: Arc<crate::lsp::LspManager>,

    pub lsp_events: crate::lsp::LspBroadcast,

    pub session_run_state: Arc<crate::session::SessionRunStateRegistry>,

    pub workspace_resources: Arc<crate::session::WorkspaceResourceManager>,

    pub git_status_cache: git_routes::GitStatusCache,

    pub config_subscriptions: Arc<Vec<crate::runtime::TaskHandle>>,
}

impl AppState {

    pub fn push_live_config(&self, snapshot: Config) {
        self.live_config.store(snapshot);
    }

    pub fn current_provider(&self) -> Arc<dyn Provider> {
        Arc::clone(&self.provider.read())
    }

    pub fn current_model(&self) -> String {
        self.model.read().clone()
    }

    pub async fn rebuild_runtime_from_config_async(&self) {
        let cfg = self.config.lock().clone();
        let cfg_for_build = cfg.clone();
        let built = tokio::task::spawn_blocking(move || {
            build_runtime_provider_from_cfg(&cfg_for_build)
        })
        .await
        .ok()
        .flatten();
        let Some((provider_arc, model_string)) = built else {
            return;
        };
        {
            let mut guard = self.provider.write();
            *guard = provider_arc;
        }
        {
            let mut guard = self.model.write();
            *guard = model_string;
        }
        self.push_live_config(cfg);
    }
}

fn build_runtime_provider_from_cfg(cfg: &Config) -> Option<(Arc<dyn Provider>, String)> {
    let resolved_default_provider = providers::resolve_runtime_provider_name(
        cfg.default_provider.as_deref().unwrap_or("openrouter"),
        cfg,
    );
    let provider_runtime_options = providers::ProviderRuntimeOptions {
        auth_profile_override: None,
        provider_api_url: cfg.api_url.clone(),
        sen_dir: cfg.config_path.parent().map(std::path::PathBuf::from),
        secrets_encrypt: cfg.secrets.encrypt,
        reasoning_enabled: cfg.runtime.reasoning_enabled,
        reasoning_effort: cfg.runtime.reasoning_effort.clone(),
        provider_timeout_secs: Some(cfg.provider_timeout_secs),
        extra_headers: providers::merged_extra_headers_for_config(cfg),
        api_path: cfg.api_path.clone(),
        provider_max_tokens: cfg.provider_max_tokens,
        model_context_windows: cfg.model_context_windows.clone(),
    };

    let provider_arc: Arc<dyn Provider> = match providers::create_resilient_provider_with_options(
        &resolved_default_provider,
        cfg.api_key.as_deref(),
        cfg.api_url.as_deref(),
        &cfg.reliability,
        &provider_runtime_options,
    ) {
        Ok(p) => Arc::from(p),
        Err(err) => {
            tracing::warn!(
                resolved_default_provider = %resolved_default_provider,
                error = %err,
                "gateway runtime hot-reload: failed to build provider for new config; \
                 falling back to placeholder openrouter so the desktop shell stays alive. \
                 The user can re-check their Provider settings."
            );
            match providers::create_resilient_provider_with_options(
                "openrouter",
                None,
                None,
                &Default::default(),
                &provider_runtime_options,
            ) {
                Ok(p) => Arc::from(p),
                Err(inner) => {
                    tracing::error!(
                        error = %inner,
                        "gateway runtime hot-reload: placeholder provider build failed; \
                         keeping previous provider Arc to avoid breaking in-flight requests"
                    );
                    return None;
                }
            }
        }
    };

    let model_string = providers::resolve_default_model(cfg).unwrap_or_default();
    Some((provider_arc, model_string))
}

#[allow(clippy::too_many_lines)]
pub async fn run_gateway(
    host: &str,
    port: u16,
    config: Config,
    prebound: Option<tokio::net::TcpListener>,
) -> Result<()> {
    run_gateway_inner(host, port, config, prebound, false).await
}

pub async fn run_gateway_with_supervisors(
    host: &str,
    port: u16,
    config: Config,
    prebound: Option<tokio::net::TcpListener>,
) -> Result<()> {
    run_gateway_inner(host, port, config, prebound, true).await
}

async fn run_gateway_inner(
    host: &str,
    port: u16,
    mut config: Config,
    prebound: Option<tokio::net::TcpListener>,
    with_scheduler: bool,
) -> Result<()> {
    desktop_bridge::install_remote_controllers();
    if desktop_bridge::bridge_mode() {
        config.gateway.require_pairing = false;
        config.gateway.allow_public_bind = false;
        config.gateway.trust_forwarded_headers = false;
        config.gateway.paired_tokens.clear();
        tracing::info!(
            target: "gateway.desktop_bridge",
            "desktop bridge mode: applied loopback-only gateway overrides (pairing disabled)"
        );
    }
    if with_scheduler && config.cron.enabled {
        let scheduler_cfg = config.clone();
        crate::runtime::task_manager::spawn_supervised_restartable(
            "gateway.cron_scheduler",
            3,
            move || {
                let scheduler_cfg = scheduler_cfg.clone();
                async move {
                    loop {
                        if crate::gateway::lifecycle::is_shutdown_requested() {
                            tracing::info!(
                                "Embedded cron scheduler stopping: gateway shutdown requested"
                            );
                            break;
                        }
                        match crate::cron::scheduler::run(scheduler_cfg.clone()).await {
                            Ok(()) => {
                                tracing::warn!(
                                    "cron scheduler exited unexpectedly; restarting in 30s"
                                );
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "cron scheduler exited with error: {e}; restarting in 30s"
                                );
                            }
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    }
                }
            },
        );
        tracing::info!("Embedded cron scheduler started alongside gateway");
    } else if with_scheduler {
        tracing::info!("Cron disabled; embedded scheduler not started");
    }

    if with_scheduler && config.hands.enabled {
        let hands_cfg = config.clone();
        crate::runtime::task_manager::spawn_supervised_restartable(
            "gateway.hands",
            3,
            move || {
                let hands_cfg = hands_cfg.clone();
                async move {
                    loop {
                        if crate::gateway::lifecycle::is_shutdown_requested() {
                            tracing::info!(
                                "Embedded hands worker stopping: gateway shutdown requested"
                            );
                            break;
                        }
                        match crate::hands::runner::run(hands_cfg.clone()).await {
                            Ok(()) => {
                                tracing::warn!(
                                    "hands worker exited unexpectedly; restarting in 30s"
                                );
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "hands worker exited with error: {e}; restarting in 30s"
                                );
                            }
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    }
                }
            },
        );
        tracing::info!("Embedded hands worker started alongside gateway");
    }

    if is_public_bind(host) && config.tunnel.provider == "none" {
        if !config.gateway.allow_public_bind {
            tracing::warn!(
                "Binding to {host}: gateway will be exposed to all network interfaces.\n\
                 Suggestion: use --host 127.0.0.1 (default), configure a tunnel, or set\n\
                 [gateway] allow_public_bind = true in config.toml to silence this warning.\n\
                 Docker/VM: if you are running inside a container or VM, this is expected."
            );
        }

        if !config.gateway.require_pairing {
            tracing::error!(
                "SECURITY: gateway is binding to public address {host} with [gateway] require_pairing = false. \
                 All endpoints will be reachable without authentication. \
                 Strongly recommended to either: (1) bind to 127.0.0.1, (2) front with a tunnel/reverse proxy, \
                 or (3) set [gateway] require_pairing = true and pair devices explicitly."
            );
        }
    }

    if crate::gateway::desktop_routes::sanitize_active_profile_in_place(&mut config) {
        tracing::info!(
            "gateway startup: sanitized stale default_provider/default_model in persisted config"
        );
        let snapshot = config.clone();

        if let Err(e) = snapshot.save().await {
            tracing::warn!(
                error = %e,
                "gateway startup: failed to persist sanitized config; will retry on next mutation"
            );
        }
    }
    let config_state = Arc::new(Mutex::new(config.clone()));
    let live_config_state = crate::config::live::LiveConfig::new(config.clone());

    if let Some(parent) = config.config_path.parent() {
        crate::gateway::ws::desktop::desktop_runtime_state()
            .set_settings_path(parent.join("desktop_user.json"));
    }

    let _event_bus = crate::event_bus::integration::init_global_bus(
        config
            .config_path
            .parent()
            .map(|p| p.join("event_audit.jsonl")),
    );
    let _multi_agent_rt = crate::agent::multi_agent_runtime::init_global_runtime();
    crate::agent::multi_agent_runtime::register_configured_agents(&_multi_agent_rt, &config);

    {
        let workspace_root = if config.workspace_dir.as_os_str().is_empty() {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        } else {
            config.workspace_dir.clone()
        };
        crate::workers::init_global_supervisor(workspace_root.clone());
        crate::workers::scan_and_recover_at(&workspace_root);
    }
    let svc_data_dir = config
        .config_path
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| config.workspace_dir.join(".senweavercoding"));
    let _ = crate::services::init_services(crate::services::ServiceContainerConfig {
        data_dir: svc_data_dir.clone(),
        shared_config: Some(Arc::clone(live_config_state.shared())),
        team_sync_enabled: config.teams.sync_enabled,
        ..Default::default()
    });
    if let Some(svc) = crate::services::try_get_services() {
        svc.update_config(config.clone());
        svc.auto_dream
            .bind_persistence(svc_data_dir.join("auto_dream.json"))
            .await;
    }
    if with_scheduler {
        crate::runtime::task_manager::spawn_supervised_restartable(
            "gateway.auto_dream_scheduler",
            3,
            || async {
                crate::agent::auto_dream_scheduler::run().await;
            },
        );
        tracing::info!("AutoDream scheduler started alongside gateway");
    }
    crate::event_bus::integration::publish_system(
        "gateway",
        crate::event_bus::types::SystemCategory::Startup,
        "Gateway starting",
    )
    .await;

    {
        let workspace_for_loc = config.workspace_dir.clone();
        let loc_cache_dir = svc_data_dir.clone();
        let project_loc = tokio::task::spawn_blocking(move || {
            crate::agent::token::budget::count_source_loc_cached(
                &workspace_for_loc,
                &loc_cache_dir,
            )
        })
        .await
        .unwrap_or(0);
        crate::observability::session_write_mode_metrics::set_token_budget_project_loc(
            project_loc,
        );
        crate::agent::token::optimizer::ensure_global_optimizer_with_loc(
            config.tool_output_compressor.clone(),
            config.token_budget.clone(),
            project_loc,
        );
    }

    crate::token_saver::set_enabled(config.token_saver.enabled);
    crate::token_saver::set_global(config.token_saver.to_runtime_ctx());
    crate::guardrails::ensure_global_guardrails(config.guardrails.clone());

    let hooks_workspace_anchor = if config.workspace_dir.as_os_str().is_empty() {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    } else {
        config.workspace_dir.clone()
    };
    let hooks_runner: std::sync::Arc<crate::hooks::HotHookRunner> =
        crate::hooks::HotHookRunner::empty();
    hooks_runner.rebuild(&config, &hooks_workspace_anchor);
    let hooks: std::sync::Arc<crate::hooks::HotHookRunner> = std::sync::Arc::clone(&hooks_runner);

    if let Err(err) =
        crate::services::governance::credential_vault::init_credential_vault(&hooks_workspace_anchor)
    {
        tracing::warn!(error = %err, "Failed to initialise credential vault");
    }

    let lsp_broadcast = crate::lsp::LspBroadcast::default();
    let lsp_service = crate::services::try_get_services()
        .map(|svc| svc.lsp.clone())
        .unwrap_or_else(crate::services::lsp::LspService::new);
    let lsp_manager = std::sync::Arc::new(
        crate::lsp::LspManager::new(
            lsp_service,
            hooks_workspace_anchor.clone(),
            lsp_broadcast.clone(),
        )
        .await,
    );
    {
        let lsp_manager_bg = std::sync::Arc::clone(&lsp_manager);
        let config_bg = config.clone();
        crate::runtime::task_manager::spawn_supervised(
            "gateway.lsp_reconcile",
            async move {
                let started = std::time::Instant::now();
                lsp_manager_bg.reconcile(&config_bg).await;
                tracing::info!(
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "Gateway LSP: background reconcile finished"
                );
            },
        );
    }

    let (listener, actual_port) = match prebound {
        Some(l) => {
            let p = l.local_addr()?.port();
            (l, p)
        }
        None => {
            let addr: SocketAddr = format!("{host}:{port}").parse()?;
            let l = tokio::net::TcpListener::bind(addr).await?;
            let p = l.local_addr()?.port();
            (l, p)
        }
    };
    let display_addr = format!("{host}:{actual_port}");

    let resolved_default_provider = providers::resolve_runtime_provider_name(
        config.default_provider.as_deref().unwrap_or("openrouter"),
        &config,
    );
    let provider_runtime_options = providers::ProviderRuntimeOptions {
        auth_profile_override: None,
        provider_api_url: config.api_url.clone(),
        sen_dir: config.config_path.parent().map(std::path::PathBuf::from),
        secrets_encrypt: config.secrets.encrypt,
        reasoning_enabled: config.runtime.reasoning_enabled,
        reasoning_effort: config.runtime.reasoning_effort.clone(),
        provider_timeout_secs: Some(config.provider_timeout_secs),
        extra_headers: providers::merged_extra_headers_for_config(&config),
        api_path: config.api_path.clone(),
        provider_max_tokens: config.provider_max_tokens,
        model_context_windows: config.model_context_windows.clone(),
    };
    let provider_inner: Arc<dyn Provider> =
        match providers::create_resilient_provider_with_options_async(
            resolved_default_provider.clone(),
            config.api_key.clone(),
            config.api_url.clone(),
            config.reliability.clone(),
            provider_runtime_options.clone(),
        )
        .await
        {
            Ok(p) => Arc::from(p),
            Err(err) => {
                tracing::warn!(
                    resolved_default_provider = %resolved_default_provider,
                    error = %err,
                    "gateway startup: failed to instantiate default provider; \
                     starting in degraded mode with a placeholder provider so the desktop \
                     shell can render Provider settings page"
                );
                Arc::from(
                    providers::create_resilient_provider_with_options_async(
                        "openrouter".to_string(),
                        None,
                        None,
                        Default::default(),
                        provider_runtime_options.clone(),
                    )
                    .await?,
                )
            }
        };
    let provider: Arc<parking_lot::RwLock<Arc<dyn Provider>>> =
        Arc::new(parking_lot::RwLock::new(provider_inner));
    let model_string = providers::resolve_default_model(&config).unwrap_or_else(|err| {
        tracing::warn!(
            "gateway startup: no default model configured ({err}); \
             starting gateway in degraded mode  -  /health will respond OK \
             so the user can enter Provider settings to add a model"
        );
        String::new()
    });
    let model: Arc<parking_lot::RwLock<String>> =
        Arc::new(parking_lot::RwLock::new(model_string));
    let temperature = config.default_temperature;
    let mem: Arc<dyn Memory> = match memory::create_memory_with_storage_and_routes_async(
        config.memory.clone(),
        config.embedding_routes.clone(),
        Some(config.storage.provider.config.clone()),
        config.workspace_dir.clone(),
        config.api_key.clone(),
    )
    .await
    {
        Ok(m) => Arc::from(m),
        Err(err) => {
            tracing::warn!(
                error = %err,
                "gateway startup: memory backend init failed; falling back to in-process \
                 NoneMemory so the desktop shell can render. The user can fix storage \
                 settings later (Settings > Memory)"
            );
            push_startup_warning(
                "memory_backend_fallback",
                format!(
                    "memory backend '{backend}' initialization failed: {err}. Falling back to \
                     in-memory (non-persistent) storage. Edit [memory] in Settings to fix.",
                    backend = config.memory.backend
                ),
            );
            Arc::new(memory::NoneMemory::new()) as Arc<dyn Memory>
        }
    };
    let runtime: Arc<dyn runtime::RuntimeAdapter> = match runtime::create_runtime(&config.runtime) {
        Ok(rt) => Arc::from(rt),
        Err(err) => {
            tracing::warn!(
                error = %err,
                "gateway startup: runtime adapter init failed; falling back to NativeRuntime \
                 so the gateway can come up. Inspect [runtime] in config.toml later"
            );
            Arc::new(runtime::NativeRuntime::new()) as Arc<dyn runtime::RuntimeAdapter>
        }
    };
    let security = Arc::new(SecurityPolicy::from_config(
        &config.autonomy,
        &config.workspace_dir,
    ));
    let mut config_subscriptions: Vec<crate::runtime::TaskHandle> = Vec::new();
    if let Some(svc) = crate::services::try_get_services() {
        let security_for_sub = Arc::clone(&security);
        let hooks_for_sub = std::sync::Arc::clone(&hooks);
        let lsp_for_sub = std::sync::Arc::clone(&lsp_manager);
        let handle = svc.config_subscribe_filtered(
            vec![String::new()],
            move |cfg| {
                security_for_sub
                    .set_command_policy_enabled(cfg.autonomy.enable_command_policy);
                crate::token_saver::set_enabled(cfg.token_saver.enabled);
                crate::token_saver::set_global(cfg.token_saver.to_runtime_ctx());
                crate::guardrails::ensure_global_guardrails(cfg.guardrails.clone());
                crate::services::proxy::runtime::ProxyRuntime::global().replace(cfg.proxy.clone());
                crate::agent::token::optimizer::ensure_global_optimizer_from_config(&cfg);
                let workspace_anchor = if cfg.workspace_dir.as_os_str().is_empty() {
                    std::env::current_dir()
                        .unwrap_or_else(|_| std::path::PathBuf::from("."))
                } else {
                    cfg.workspace_dir.clone()
                };
                hooks_for_sub.rebuild(&cfg, &workspace_anchor);
                lsp_for_sub.set_workspace_root(workspace_anchor.clone());
                let lsp_clone = std::sync::Arc::clone(&lsp_for_sub);
                let cfg_clone = std::sync::Arc::clone(&cfg);
                crate::runtime::task_manager::spawn_supervised(
                    "gateway.hot_reload.lsp_reconcile",
                    async move {
                        lsp_clone.reconcile(&cfg_clone).await;
                    },
                );
            },
        );
        config_subscriptions.push(handle);
    }

    let (composio_key, composio_entity_id) = if config.composio.enabled {
        (
            config.composio.api_key.as_deref(),
            Some(config.composio.entity_id.as_str()),
        )
    } else {
        (None, None)
    };

    let rbac_engine = if config.rbac.enabled {
        Some(Arc::new(crate::security::rbac::RbacEngine::new(
            config.rbac.clone(),
            &config.workspace_dir,
        )))
    } else {
        None
    };

    let canvas_store = tools::CanvasStore::new();

    let (
        mut tools_registry_raw,
        delegate_handle_gw,
        _reaction_handle_gw,
        _channel_map_handle,
        _ask_user_handle_gw,
        _escalate_handle_gw,
        _plan_mode_flag_gw,
    ) = tools::all_tools_with_runtime(
        Arc::new(config.clone()),
        &security,
        runtime,
        Arc::clone(&mem),
        composio_key,
        composio_entity_id,
        &config.browser,
        &config.http_request,
        &config.web_fetch,
        &config.workspace_dir,
        &config.agents,
        config.api_key.as_deref(),
        &config,
        Some(canvas_store.clone()),
    );

    let (gateway_builtin_deferred_enabled, gateway_mcp_deferred_enabled) =
        crate::tools::deferred_loading_effective(&config);
    let mut gateway_activated_handle: Option<
        std::sync::Arc<parking_lot::Mutex<tools::ActivatedToolSet>>,
    > = None;
    if config.mcp.enabled && !config.mcp.servers.is_empty() {
        tracing::info!(
            "Gateway: initializing MCP client  -  {} server(s) configured",
            config.mcp.servers.len()
        );
        let mcp_started = std::time::Instant::now();
        let mcp_overall_deadline = std::time::Duration::from_secs(5);
        let mcp_result = tokio::time::timeout(
            mcp_overall_deadline,
            tools::McpRegistry::connect_all(&config.mcp.servers),
        )
        .await;
        let mcp_outcome = match mcp_result {
            Ok(inner) => inner,
            Err(_) => {
                tracing::warn!(
                    elapsed_ms = mcp_started.elapsed().as_millis() as u64,
                    deadline_secs = mcp_overall_deadline.as_secs(),
                    "Gateway MCP: connect_all exceeded startup deadline; continuing with empty registry. \
                     Slow MCP servers will not block /health; tools become available after restart or when stubs are populated by background retry"
                );
                Ok(tools::McpRegistry::empty())
            }
        };
        match mcp_outcome {
            Ok(registry) => {
                let registry = std::sync::Arc::new(registry);
                if gateway_mcp_deferred_enabled {
                    let deferred_set =
                        tools::DeferredMcpToolSet::from_registry(std::sync::Arc::clone(&registry))
                            .await;
                    tracing::info!(
                        "Gateway MCP deferred: {} tool stub(s) from {} server(s)",
                        deferred_set.len(),
                        registry.server_count()
                    );
                    let activated = std::sync::Arc::new(parking_lot::Mutex::new(
                        tools::ActivatedToolSet::new(),
                    ));
                    gateway_activated_handle = Some(std::sync::Arc::clone(&activated));
                    tools_registry_raw.push(Box::new(tools::ToolSearchTool::new(
                        deferred_set,
                        activated,
                    )));
                } else {
                    let names = registry.tool_names();
                    let mut registered = 0usize;
                    for name in names {
                        if let Some(def) = registry.get_tool_def(&name).await {
                            let wrapper: std::sync::Arc<dyn tools::Tool> =
                                std::sync::Arc::new(tools::McpToolWrapper::new(
                                    name,
                                    def,
                                    std::sync::Arc::clone(&registry),
                                ));
                            if let Some(ref handle) = delegate_handle_gw {
                                handle.write().push(std::sync::Arc::clone(&wrapper));
                            }
                            tools_registry_raw.push(Box::new(tools::ArcToolRef(wrapper)));
                            registered += 1;
                        }
                    }
                    tracing::info!(
                        "Gateway MCP: {} tool(s) registered from {} server(s)",
                        registered,
                        registry.server_count()
                    );
                }
            }
            Err(e) => {
                tracing::error!("Gateway MCP registry failed to initialize: {e:#}");
            }
        }
    }

    if gateway_builtin_deferred_enabled {
        let mut gateway_deferred_section = String::new();
        let workspace_key = crate::session::workspace_key_from_path(
            &config.workspace_dir,
            "gateway",
        );
        let allowlist = config.permissions.tool_allowlist.clone();
        let sink = crate::gateway::ws::gateway_approval_sink_handle();
        let bus = crate::gateway::ws::gateway_approval_bus().clone();
        let gate: Option<crate::security::permissions::ToolActivationGateHandle> = Some(
            std::sync::Arc::new(crate::security::permissions::SessionActivationGate::new(
                sink,
                bus,
                300_000,
            )) as crate::security::permissions::ToolActivationGateHandle,
        );
        let options = crate::tools::BuiltinDeferredRegistrationOptions {
            workspace_key,
            allowlist,
            gate,
            config: Some(&config),
        };
        let _gateway_builtin_set =
            crate::tools::apply_builtin_deferred_registration_with_options(
                &mut tools_registry_raw,
                &mut gateway_deferred_section,
                crate::tools::ToolSurfaceBaseline::Desktop,
                &mut gateway_activated_handle,
                options,
            );
    }

    let tools_registry: Arc<Vec<ToolSpec>> =
        Arc::new(tools_registry_raw.iter().map(|t| t.spec()).collect());

    let cost_tracker = CostTracker::get_or_init_global(config.cost.clone(), &config.workspace_dir);

    let (event_tx, _event_rx) = tokio::sync::broadcast::channel::<serde_json::Value>(256);
    install_gateway_event_tx(event_tx.clone());

    {
        let usage_event_tx = event_tx.clone();
        crate::cost::tracker::set_usage_notify_callback(move |record| {
            let payload = serde_json::json!({
                "type": "usage_updated",
                "sessionId": record.chat_session_id,
                "codingMode": record.coding_mode,
                "provider": record.provider,
                "model": record.usage.model,
                "inputTokens": record.usage.input_tokens,
                "outputTokens": record.usage.output_tokens,
                "totalTokens": record.usage.total_tokens,
                "costUsd": record.usage.cost_usd,
                "timestamp": record.usage.timestamp.to_rfc3339(),
            });
            let _ = usage_event_tx.send(payload);
        });
    }

    {
        let live_mcp = crate::gateway::mcp_live::LiveMcpReconciler::new();
        live_mcp.seed_from_config(&config);
        if let Some(svc) = crate::services::try_get_services() {
            let live_mcp_for_sub = std::sync::Arc::clone(&live_mcp);
            let event_tx_for_sub = event_tx.clone();
            let handle = svc.config_subscribe_filtered(
                vec![String::new()],
                move |cfg| {
                    live_mcp_for_sub
                        .schedule_reconcile(cfg, event_tx_for_sub.clone());
                },
            );
            config_subscriptions.push(handle);
        }
    }

    let webhook_secret_hash: Option<Arc<str>> =
        config.channels_config.webhook.as_ref().and_then(|webhook| {
            webhook.secret.as_ref().and_then(|raw_secret| {
                let trimmed_secret = raw_secret.trim();
                (!trimmed_secret.is_empty())
                    .then(|| Arc::<str>::from(hash_webhook_secret(trimmed_secret)))
            })
        });

    let whatsapp_channel: Option<Arc<WhatsAppChannel>> = config
        .channels_config
        .whatsapp
        .as_ref()
        .filter(|wa| wa.is_cloud_config())
        .map(|wa| {
            Arc::new(WhatsAppChannel::new(
                wa.access_token.clone().unwrap_or_default(),
                wa.phone_number_id.clone().unwrap_or_default(),
                wa.verify_token.clone().unwrap_or_default(),
                wa.allowed_numbers.clone(),
            ))
        });

    let whatsapp_app_secret: Option<Arc<str>> = std::env::var("SEN_WHATSAPP_APP_SECRET")
        .ok()
        .and_then(|secret| {
            let secret = secret.trim();
            (!secret.is_empty()).then(|| secret.to_owned())
        })
        .or_else(|| {
            config.channels_config.whatsapp.as_ref().and_then(|wa| {
                wa.app_secret
                    .as_deref()
                    .map(str::trim)
                    .filter(|secret| !secret.is_empty())
                    .map(ToOwned::to_owned)
            })
        })
        .map(Arc::from);

    let linq_channel: Option<Arc<LinqChannel>> = config.channels_config.linq.as_ref().map(|lq| {
        Arc::new(LinqChannel::new(
            lq.api_token.clone(),
            lq.from_phone.clone(),
            lq.allowed_senders.clone(),
        ))
    });

    let linq_signing_secret: Option<Arc<str>> = std::env::var("SEN_LINQ_SIGNING_SECRET")
        .ok()
        .and_then(|secret| {
            let secret = secret.trim();
            (!secret.is_empty()).then(|| secret.to_owned())
        })
        .or_else(|| {
            config.channels_config.linq.as_ref().and_then(|lq| {
                lq.signing_secret
                    .as_deref()
                    .map(str::trim)
                    .filter(|secret| !secret.is_empty())
                    .map(ToOwned::to_owned)
            })
        })
        .map(Arc::from);

    let wati_channel: Option<Arc<WatiChannel>> =
        config.channels_config.wati.as_ref().map(|wati_cfg| {
            Arc::new(
                WatiChannel::new(
                    wati_cfg.api_token.clone(),
                    wati_cfg.api_url.clone(),
                    wati_cfg.tenant_id.clone(),
                    wati_cfg.allowed_numbers.clone(),
                )
                .with_transcription(config.transcription.clone()),
            )
        });

    let nextcloud_talk_channel: Option<Arc<NextcloudTalkChannel>> =
        config.channels_config.nextcloud_talk.as_ref().map(|nc| {
            Arc::new(NextcloudTalkChannel::new(
                nc.base_url.clone(),
                nc.app_token.clone(),
                nc.bot_name.clone().unwrap_or_default(),
                nc.allowed_users.clone(),
            ))
        });

    let nextcloud_talk_webhook_secret: Option<Arc<str>> =
        std::env::var("SEN_NEXTCLOUD_TALK_WEBHOOK_SECRET")
            .ok()
            .and_then(|secret| {
                let secret = secret.trim();
                (!secret.is_empty()).then(|| secret.to_owned())
            })
            .or_else(|| {
                config
                    .channels_config
                    .nextcloud_talk
                    .as_ref()
                    .and_then(|nc| {
                        nc.webhook_secret
                            .as_deref()
                            .map(str::trim)
                            .filter(|secret| !secret.is_empty())
                            .map(ToOwned::to_owned)
                    })
            })
            .map(Arc::from);

    let gmail_push_channel: Option<Arc<GmailPushChannel>> = config
        .channels_config
        .gmail_push
        .as_ref()
        .filter(|gp| gp.enabled)
        .map(|gp| Arc::new(GmailPushChannel::new(gp.clone())));

    let session_backend: Option<Arc<dyn SessionBackend>> = if config.gateway.session_persistence {
        let backend_workspace_dir = config.workspace_dir.clone();
        let backend_init =
            tokio::task::spawn_blocking(move || SqliteSessionBackend::new(&backend_workspace_dir))
                .await
                .unwrap_or_else(|join_err| {
                    Err(anyhow::anyhow!(
                        "session backend init task panicked: {join_err}"
                    ))
                });
        match backend_init {
            Ok(b) => {
                tracing::info!("Gateway session persistence enabled (SQLite)");
                let backend = Arc::new(b);
                crate::channels::session::set_global_session_backend(
                    Arc::clone(&backend) as Arc<dyn SessionBackend>,
                );
                if config.gateway.session_ttl_hours > 0 {
                    let cleanup_backend = Arc::clone(&backend);
                    let ttl_hours = config.gateway.session_ttl_hours;
                    tokio::task::spawn_blocking(move || {
                        match cleanup_backend.cleanup_stale(ttl_hours) {
                            Ok(cleaned) if cleaned > 0 => {
                                tracing::info!("Cleaned up {cleaned} stale gateway sessions");
                            }
                            Ok(_) => {}
                            Err(e) => {
                                tracing::warn!("stale gateway session cleanup failed: {e}");
                            }
                        }
                    });
                }
                Some(backend as Arc<dyn SessionBackend>)
            }
            Err(e) => {
                tracing::warn!("Session persistence disabled: {e}");
                None
            }
        }
    } else {
        None
    };

    let pairing = Arc::new(PairingGuard::new(
        config.gateway.require_pairing,
        &config.gateway.paired_tokens,
    ));
    let rate_limit_max_keys = normalize_max_keys(
        config.gateway.rate_limit_max_keys,
        RATE_LIMIT_MAX_KEYS_DEFAULT,
    );
    let rate_limiter = Arc::new(GatewayRateLimiter::new(
        config.gateway.pair_rate_limit_per_minute,
        config.gateway.webhook_rate_limit_per_minute,
        rate_limit_max_keys,
    ));
    let idempotency_max_keys = normalize_max_keys(
        config.gateway.idempotency_max_keys,
        IDEMPOTENCY_MAX_KEYS_DEFAULT,
    );
    let idempotency_store = Arc::new(IdempotencyStore::new(
        Duration::from_secs(config.gateway.idempotency_ttl_secs.max(1)),
        idempotency_max_keys,
    ));

    let path_prefix: Option<&str> = config
        .gateway
        .path_prefix
        .as_deref()
        .filter(|p| !p.is_empty());

    let tunnel = match crate::tunnel::create_tunnel(&config.tunnel) {
        Ok(t) => t,
        Err(err) => {
            tracing::warn!(
                provider = %config.tunnel.provider,
                error = %err,
                "gateway startup: tunnel construction failed; continuing in local-only mode \
                 (the user can fix [tunnel] settings later without a restart-blocking error)"
            );
            None
        }
    };
    let mut tunnel_url: Option<String> = None;

    if let Some(ref tun) = tunnel {
        println!(" - ? Starting {} tunnel...", tun.name());
        match tun.start(host, actual_port).await {
            Ok(url) => {
                println!(" - ? Tunnel active: {url}");
                tunnel_url = Some(url);
            }
            Err(e) => {
                println!("\u{274C}  Tunnel failed to start: {e}");
                println!("   Falling back to local-only mode.");
            }
        }
    }

    crate::event_bus::integration::publish_lifecycle(
        "gateway",
        crate::event_bus::types::LifecyclePhase::Started,
        None,
    )
    .await;

    let pfx = path_prefix.unwrap_or("");
    println!("\u{1F680} SenWeaverCoding Gateway listening on http://{display_addr}{pfx}");
    if let Some(ref url) = tunnel_url {
        println!("   - ? Public URL: {url}");
    }
    println!("   - ? Web Dashboard: http://{display_addr}{pfx}/");
    if let Some(code) = pairing.pairing_code() {
        println!();
        println!("   - ? PAIRING REQUIRED  -  use this one-time code:");
        println!("      \u{2500}  \u{2500}  \u{2500}  \u{2500}  \u{2500}  \u{2500}  \u{2500}  \u{2500} \u{2022}");
        println!("      -   {code}   - ");
        println!("      -  -  -  -  -  -  -  - ");
        println!("     Send: POST {pfx}/pair with header X-Pairing-Code: {code}");
    } else if pairing.require_pairing() {
        println!("   - ? Pairing: ACTIVE (bearer token required)");
        println!("     To pair a new device: sen gateway get-paircode --new");
        println!();
    } else {
        println!("  \u{26A0}\u{FE0F}  Pairing: DISABLED (all requests accepted)");
        println!();
    }
    println!("  POST {pfx}/pair       -  pair a new client (X-Pairing-Code header)");
    println!("  POST {pfx}/webhook    -  {{\"message\": \"your prompt\"}}");
    if whatsapp_channel.is_some() {
        println!("  GET  {pfx}/whatsapp   -  Meta webhook verification");
        println!("  POST {pfx}/whatsapp   -  WhatsApp message webhook");
    }
    if linq_channel.is_some() {
        println!("  POST {pfx}/linq       -  Linq message webhook (iMessage/RCS/SMS)");
    }
    if wati_channel.is_some() {
        println!("  GET  {pfx}/wati       -  WATI webhook verification");
        println!("  POST {pfx}/wati       -  WATI message webhook");
    }
    if nextcloud_talk_channel.is_some() {
        println!("  POST {pfx}/nextcloud-talk  -  Nextcloud Talk bot webhook");
    }
    println!("  GET  {pfx}/api/*      -  REST API (bearer token required)");
    println!("  GET  {pfx}/ws/chat    -  WebSocket agent chat");
    if config.nodes.enabled {
        println!("  GET  {pfx}/ws/nodes   -  WebSocket node discovery");
    }
    println!("  GET  {pfx}/health     -  health check");
    println!("  GET  {pfx}/metrics    -  Prometheus metrics");
    println!("  Press Ctrl+C to stop.\n");

    hooks.fire_gateway_start(host, actual_port).await;

    let broadcast_observer: Arc<dyn crate::observability::Observer> =
        Arc::new(sse::BroadcastObserver::new(
            crate::observability::create_observer(&config.observability),
            event_tx.clone(),
        ));

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    crate::gateway::lifecycle::install_shutdown_sender(shutdown_tx.clone());
    let _running_guard = GatewayRunningGuard::install();

    let node_registry = Arc::new(nodes::NodeRegistry::new(config.nodes.max_nodes));

    let device_registry = if config.gateway.require_pairing {
        match api::pairing::DeviceRegistry::new(&config.workspace_dir) {
            Ok(registry) => Some(Arc::new(registry)),
            Err(e) => {
                tracing::error!("Failed to initialise device registry: {e}");
                None
            }
        }
    } else {
        None
    };
    if config.evolution.enabled {
        match crate::evolution::init_global(
            config.workspace_dir.clone(),
            config.evolution.clone(),
        ) {
            Ok(engine) => {
                engine.set_judge_provider(crate::evolution::JudgeProviderRef {
                    provider: Arc::clone(&provider.read()),
                    model: model.read().clone(),
                });
                let registered_models = collect_registered_models_for_engine(&config);
                engine.clear_reflection_providers();
                {
                    let engine_for_factories = Arc::clone(&engine);
                    let config_for_factories = config.clone();
                    tokio::task::spawn_blocking(move || {
                        register_per_provider_reflection_factories(
                            &engine_for_factories,
                            &config_for_factories,
                        );
                    })
                    .await
                    .ok();
                }
                engine.set_registered_models(registered_models);
                engine.ensure_judge_worker();
                engine.ensure_reflection_scheduler();
                tracing::info!(
                    workspace_dir = %config.workspace_dir.display(),
                    persist_training_data = config.evolution.persist_training_data,
                    judge_enabled = config.evolution.next_state_judge_enabled,
                    recycling_enabled = config.evolution.recycling.enabled,
                    reflection_enabled = config.evolution.reflection.enabled,
                    registered_model_count = engine.registered_models().len(),
                    "Evolution engine initialised"
                );
            }
            Err(error) => {
                tracing::warn!(error = %error, "Failed to initialise evolution engine");
            }
        }
    }

    let state = AppState {
        config: config_state,
        live_config: live_config_state,
        provider,
        model,
        temperature,
        mem,
        auto_save: config.memory.auto_save,
        webhook_secret_hash,
        pairing,
        trust_forwarded_headers: config.gateway.trust_forwarded_headers,
        rate_limiter,
        auth_limiter: Arc::new(auth_rate_limit::AuthRateLimiter::new()),
        idempotency_store,
        whatsapp: whatsapp_channel,
        whatsapp_app_secret,
        linq: linq_channel,
        linq_signing_secret,
        nextcloud_talk: nextcloud_talk_channel,
        nextcloud_talk_webhook_secret,
        wati: wati_channel,
        gmail_push: gmail_push_channel,
        observer: broadcast_observer,
        tools_registry,
        cost_tracker,
        event_tx,
        shutdown_tx,
        node_registry,
        session_backend,
        device_registry,
        path_prefix: path_prefix.unwrap_or("").to_string(),
        rbac: rbac_engine,
        canvas_store,
        hooks: hooks.clone(),
        lsp: lsp_manager.clone(),
        lsp_events: lsp_broadcast.clone(),
        session_run_state: crate::session::SessionRunStateRegistry::new(),
        workspace_resources: {
            let mgr = crate::session::WorkspaceResourceManager::new();
            crate::session::install_global_workspace_resources(mgr.clone());
            mgr
        },
        git_status_cache: git_routes::new_git_status_cache(),
        config_subscriptions: Arc::new(config_subscriptions),
        #[cfg(feature = "webauthn")]
        webauthn: if config.security.webauthn.enabled {
            let secret_store = Arc::new(crate::security::SecretStore::new(
                &config.workspace_dir,
                true,
            ));
            let wa_config = crate::security::webauthn::WebAuthnConfig {
                enabled: true,
                rp_id: config.security.webauthn.rp_id.clone(),
                rp_origin: config.security.webauthn.rp_origin.clone(),
                rp_name: config.security.webauthn.rp_name.clone(),
            };
            Some(Arc::new(api::webauthn::WebAuthnState {
                manager: crate::security::webauthn::WebAuthnManager::new(
                    wa_config,
                    secret_store,
                    &config.workspace_dir,
                ),
                pending_registrations: parking_lot::Mutex::new(std::collections::HashMap::new()),
                pending_authentications: parking_lot::Mutex::new(std::collections::HashMap::new()),
            }))
        } else {
            None
        },
    };

    if with_scheduler {
        let cfg_snapshot = state.config.lock().clone();
        channel_supervisor::start_embedded_channels(&cfg_snapshot, &state.live_config);
    }

    let config_put_router = Router::new()
        .route("/api/config", put(api::handle_api_config_put))
        .layer(RequestBodyLimitLayer::new(1_048_576));

    let workspace_files_writes_router = Router::new()
        .route(
            "/api/workspace/file",
            put(workspace_files::handle_workspace_file_put)
                .post(workspace_files::handle_workspace_file_post),
        )
        .route(
            "/api/workspace/upload",
            post(workspace_files::handle_workspace_upload),
        )
        .layer(RequestBodyLimitLayer::new(16 * 1024 * 1024));

    let a2a_config = state.config.clone();
    let a2a_state = a2a::build_a2a_state("sen", format!("http://{}:{}", host, actual_port))
        .with_executor(Arc::new(move |description: String| {
            let cfg_handle = a2a_config.clone();
            Box::pin(async move {
                let cfg = cfg_handle.lock().clone();
                let temperature = cfg.default_temperature;
                crate::agent::run(
                    cfg,
                    Some(description),
                    None,
                    None,
                    temperature,
                    Vec::new(),
                    false,
                    None,
                    None,
                    None,
                )
                .await
                .map_err(|e| format!("{e:#}"))
            })
        }));
    let a2a_router = a2a::create_a2a_router(a2a_state);

    let agent_turn_router = routes::agent::agent_router(state.clone());

    let api_routes = Router::new()
        .route("/api/status", get(api::handle_api_status))
        .route("/api/config", get(api::handle_api_config_get))
        .route("/api/provider", get(api::handle_api_provider_get))
        .route("/api/provider", put(api::handle_api_provider_put))
        .route("/api/tools", get(api::handle_api_tools))
        .route("/api/cron", get(api::handle_api_cron_list))
        .route("/api/cron", post(api::handle_api_cron_add))
        .route(
            "/api/cron/settings",
            get(api::handle_api_cron_settings_get).patch(api::handle_api_cron_settings_patch),
        )
        .route(
            "/api/cron/{id}",
            delete(api::handle_api_cron_delete).patch(api::handle_api_cron_patch),
        )
        .route("/api/cron/{id}/runs", get(api::handle_api_cron_runs))
        .route("/api/integrations", get(api::handle_api_integrations))
        .route(
            "/api/integrations/settings",
            get(api::handle_api_integrations_settings),
        )
        .route(
            "/api/doctor",
            get(api::handle_api_doctor).post(api::handle_api_doctor),
        )
        .route("/api/memory", get(api::handle_api_memory_list))
        .route("/api/memory", post(api::handle_api_memory_store))
        .route("/api/memory/{key}", delete(api::handle_api_memory_delete))
        .route("/api/cost", get(api::handle_api_cost))
        .route("/api/cli-tools", get(api::handle_api_cli_tools))
        .route(
            "/api/channels",
            get(api::handle_api_channels_get).put(api::handle_api_channels_put),
        )
        .route(
            "/api/workflows/validate",
            post(api::handle_api_workflows_validate),
        )
        .route(
            "/api/workflows/execute",
            post(api::handle_api_workflows_execute),
        )
        .route("/api/health", get(api::handle_api_health))
        .route("/api/tips/next", get(api::handle_api_tips_next))
        .route("/api/tips/dismiss", post(api::handle_api_tips_dismiss))
        .route(
            "/api/remote/sessions",
            get(api::handle_api_remote_sessions_list).post(api::handle_api_remote_sessions_register),
        )
        .route("/api/metrics", get(api::handle_api_metrics))
        .route(
            "/api/sessions",
            get(api::handle_api_sessions_list).post(api::handle_api_session_create),
        )
        .route(
            "/api/sessions/events",
            get(api::handle_api_sessions_events),
        )
        .route(
            "/api/gateway/events",
            get(crate::gateway::sse::handle_sse_events),
        )
        .route(
            "/api/sessions/recent-projects",
            get(api::handle_api_sessions_recent_projects),
        )
        .route(
            "/api/sessions/delete-batch",
            post(api::handle_api_sessions_delete_batch),
        )
        .route(
            "/api/sessions/{id}/messages",
            get(api::handle_api_session_messages),
        )
        .route(
            "/api/sessions/{id}/git-info",
            get(api::handle_api_session_git_info),
        )
        .route(
            "/api/sessions/{id}/slash-commands",
            get(api::handle_api_session_slash_commands),
        )
        .route(
            "/api/sessions/{id}/rewind",
            post(api::handle_api_session_rewind),
        )
        .route(
            "/api/sessions/{id}/rewind/restore",
            post(api::handle_api_session_rewind_restore),
        )
        .route(
            "/api/sessions/{id}/rewind/commit",
            post(api::handle_api_session_rewind_commit),
        )
        .route(
            "/api/sessions/{id}/revert-batches",
            post(api::handle_api_session_revert_batches),
        )
        .route(
            "/api/sessions/{id}/design-artifacts",
            get(desktop_routes::handle_session_design_artifacts),
        )
        .route(
            "/api/sessions/{id}/design-artifacts/delete",
            post(desktop_routes::handle_session_design_artifact_delete),
        )
        .route(
            "/api/sessions/{id}/design-handoff",
            post(desktop_routes::handle_session_design_handoff),
        )
        .route(
            "/api/sessions/{id}/design-lint",
            post(desktop_routes::handle_session_design_lint),
        )
        .route(
            "/api/sessions/{id}/design-units",
            post(desktop_routes::handle_session_design_unit_add),
        )
        .route(
            "/api/sessions/{id}",
            delete(api::handle_api_session_delete)
                .put(api::handle_api_session_rename)
                .patch(api::handle_api_session_rename),
        )
        .route("/api/pairing/initiate", post(api::pairing::initiate_pairing))
        .route("/api/pair", post(api::pairing::submit_pairing_enhanced))
        .route("/api/devices", get(api::pairing::list_devices))
        .route("/api/devices/{id}", delete(api::pairing::revoke_device))
        .route(
            "/api/devices/{id}/token/rotate",
            post(api::pairing::rotate_token),
        )
        .route("/api/canvas", get(canvas::handle_canvas_list))
        .route(
            "/api/canvas/{id}",
            get(canvas::handle_canvas_get)
                .post(canvas::handle_canvas_post)
                .delete(canvas::handle_canvas_clear),
        )
        .route(
            "/api/canvas/{id}/history",
            get(canvas::handle_canvas_history),
        )
        .route("/hooks/claude-code", post(api::handle_claude_code_hook))

        .route("/api/models", get(desktop_routes::handle_models_list))
        .route(
            "/api/models/available",
            get(desktop_routes::handle_models_available),
        )
        .route(
            "/api/models/current",
            get(desktop_routes::handle_models_current).put(desktop_routes::handle_models_set_current),
        )
        .route(
            "/api/effort",
            get(desktop_routes::handle_effort_get).put(desktop_routes::handle_effort_set),
        )
        .route(
            "/api/providers",
            get(desktop_routes::handle_providers_list).post(desktop_routes::handle_providers_create),
        )
        .route(
            "/api/providers/presets",
            get(desktop_routes::handle_providers_presets),
        )
        .route(
            "/api/providers/auth-status",
            get(desktop_routes::handle_providers_auth_status),
        )
        .route(
            "/api/providers/settings",
            get(desktop_routes::handle_providers_settings_get)
                .put(desktop_routes::handle_providers_settings_put),
        )
        .route(
            "/api/providers/official",
            post(desktop_routes::handle_providers_official),
        )
        .route(
            "/api/providers/test",
            post(desktop_routes::handle_providers_test_config),
        )
        .route(
            "/api/providers/{id}",
            put(desktop_routes::handle_providers_update).delete(desktop_routes::handle_providers_delete),
        )
        .route(
            "/api/providers/{id}/activate",
            post(desktop_routes::handle_providers_activate),
        )
        .route(
            "/api/providers/{id}/test",
            post(desktop_routes::handle_providers_test),
        )
        .route(
            "/api/credentials",
            get(credential_routes::handle_list).put(credential_routes::handle_put),
        )
        .route(
            "/api/credentials/{name}",
            delete(credential_routes::handle_delete),
        )
        .route(
            "/api/skills",
            get(desktop_routes::handle_skills_list).put(api::handle_api_skills_put),
        )
        .route("/api/skills/detail", get(desktop_routes::handle_skills_detail))
        .route(
            "/api/skills/file",
            get(desktop_routes::handle_user_skill_get)
                .post(desktop_routes::handle_user_skill_upsert)
                .put(desktop_routes::handle_user_skill_upsert)
                .delete(desktop_routes::handle_user_skill_delete),
        )
        .route(
            "/api/skills/install",
            post(desktop_routes::handle_user_skill_install),
        )
        .route("/api/rules", get(desktop_routes::handle_user_rules_list))
        .route(
            "/api/rules/file",
            get(desktop_routes::handle_user_rule_get)
                .post(desktop_routes::handle_user_rule_upsert)
                .put(desktop_routes::handle_user_rule_upsert)
                .delete(desktop_routes::handle_user_rule_delete),
        )
        .route(
            "/api/hooks",
            get(desktop_routes::handle_hooks_get).put(desktop_routes::handle_hooks_put),
        )
        .route(
            "/api/agent-config",
            get(desktop_routes::handle_agent_config_get)
                .put(desktop_routes::handle_agent_config_put),
        )
        .route(
            "/api/agent-runtime",
            get(desktop_routes::handle_agent_runtime_get)
                .put(desktop_routes::handle_agent_runtime_put),
        )
        .route(
            "/api/web-search",
            get(desktop_routes::handle_web_search_get)
                .put(desktop_routes::handle_web_search_put),
        )
        .route(
            "/api/web-fetch",
            get(desktop_routes::handle_web_fetch_get)
                .put(desktop_routes::handle_web_fetch_put),
        )
        .route(
            "/api/browser-config",
            get(desktop_routes::handle_browser_config_get)
                .put(desktop_routes::handle_browser_config_put),
        )
        .route(
            "/api/auto-dream",
            get(desktop_routes::handle_auto_dream_get)
                .put(desktop_routes::handle_auto_dream_put),
        )
        .route(
            "/api/auto-dream/tasks",
            post(desktop_routes::handle_auto_dream_task_create),
        )
        .route(
            "/api/auto-dream/tasks/{id}",
            put(desktop_routes::handle_auto_dream_task_update)
                .delete(desktop_routes::handle_auto_dream_task_delete),
        )
        .route(
            "/api/guardrails",
            put(desktop_routes::handle_guardrails_put),
        )
        .route(
            "/api/agents/{name}",
            get(desktop_routes::handle_agent_get)
                .put(desktop_routes::handle_agent_update)
                .delete(desktop_routes::handle_agent_delete),
        )
        .route(
            "/api/agents",
            get(desktop_routes::handle_agents_list).post(desktop_routes::handle_agent_create),
        )
        .route(
            "/api/custom-tools",
            get(desktop_routes::handle_custom_tools_list)
                .post(desktop_routes::handle_custom_tools_create),
        )
        .route(
            "/api/custom-tools/{name}",
            put(desktop_routes::handle_custom_tools_update)
                .delete(desktop_routes::handle_custom_tools_delete),
        )
        .route("/api/usage", get(desktop_routes::handle_usage_get))
        .route(
            "/api/evolution/overview",
            get(evolution_routes::handle_overview),
        )
        .route(
            "/api/evolution/lessons",
            get(evolution_routes::handle_lessons_list),
        )
        .route(
            "/api/evolution/lessons/{id}",
            put(evolution_routes::handle_lesson_put)
                .delete(evolution_routes::handle_lesson_delete),
        )
        .route(
            "/api/evolution/thumbs",
            post(evolution_routes::handle_thumbs),
        )
        .route(
            "/api/evolution/distill",
            post(evolution_routes::handle_distill),
        )
        .route(
            "/api/evolution/rescore",
            post(evolution_routes::handle_rescore),
        )
        .route(
            "/api/evolution/config",
            get(evolution_routes::handle_config_get).put(evolution_routes::handle_config_put),
        )
        .route(
            "/api/evolution/persistence",
            get(evolution_routes::handle_persistence_get)
                .put(evolution_routes::handle_persistence_put),
        )
        .route(
            "/api/evolution/persistence/purge",
            post(evolution_routes::handle_persistence_purge),
        )
        .route(
            "/api/evolution/export/formats",
            get(evolution_routes::handle_export_formats),
        )
        .route(
            "/api/evolution/exports",
            get(evolution_routes::handle_export_list)
                .post(evolution_routes::handle_export_create),
        )
        .route(
            "/api/evolution/exports/{id}",
            axum::routing::delete(evolution_routes::handle_export_delete),
        )
        .route(
            "/api/evolution/cloud/targets",
            get(evolution_routes::handle_cloud_targets_list)
                .post(evolution_routes::handle_cloud_targets_upsert),
        )
        .route(
            "/api/evolution/cloud/targets/{id}",
            put(evolution_routes::handle_cloud_targets_upsert)
                .delete(evolution_routes::handle_cloud_target_delete),
        )
        .route(
            "/api/evolution/cloud/push",
            post(evolution_routes::handle_cloud_push),
        )
        .route(
            "/api/evolution/cloud/history",
            get(evolution_routes::handle_push_history),
        )
        .route(
            "/api/evolution/recycling/config",
            get(evolution_routes::handle_recycling_config_get)
                .put(evolution_routes::handle_recycling_config_put),
        )
        .route(
            "/api/evolution/recycling/recent",
            get(evolution_routes::handle_recycling_recent),
        )
        .route(
            "/api/evolution/recycling/purge",
            post(evolution_routes::handle_recycling_purge),
        )
        .route(
            "/api/evolution/reflection/config",
            get(evolution_routes::handle_reflection_config_get)
                .put(evolution_routes::handle_reflection_config_put),
        )
        .route(
            "/api/evolution/reflection/runs",
            get(evolution_routes::handle_reflection_runs),
        )
        .route(
            "/api/evolution/reflection/run",
            post(evolution_routes::handle_reflection_run),
        )
        .route(
            "/api/mcp",
            get(desktop_routes::handle_mcp_list).post(desktop_routes::handle_mcp_create),
        )
        .route(
            "/api/mcp/{name}",
            put(desktop_routes::handle_mcp_update).delete(desktop_routes::handle_mcp_delete),
        )
        .route(
            "/api/mcp/{name}/status",
            get(desktop_routes::handle_mcp_status),
        )
        .route(
            "/api/mcp/{name}/toggle",
            post(desktop_routes::handle_mcp_toggle),
        )
        .route(
            "/api/mcp/{name}/reconnect",
            post(desktop_routes::handle_mcp_reconnect),
        )

        .route(
            "/api/lsp",
            get(desktop_routes::handle_lsp_list).put(desktop_routes::handle_lsp_global_put),
        )
        .route(
            "/api/lsp/preferences",
            get(desktop_routes::handle_lsp_preferences_get)
                .put(desktop_routes::handle_lsp_preferences_put),
        )
        .route(
            "/api/lsp/servers",
            post(desktop_routes::handle_lsp_create),
        )
        .route(
            "/api/lsp/servers/{id}",
            put(desktop_routes::handle_lsp_update).delete(desktop_routes::handle_lsp_delete),
        )
        .route(
            "/api/lsp/servers/{id}/toggle",
            post(desktop_routes::handle_lsp_toggle),
        )
        .route(
            "/api/lsp/servers/{id}/install",
            post(desktop_routes::handle_lsp_install),
        )
        .route(
            "/api/lsp/servers/{id}/restart",
            post(desktop_routes::handle_lsp_restart),
        )
        .route(
            "/api/lsp/textdocument",
            post(desktop_routes::handle_lsp_notify),
        )
        .route(
            "/api/lsp/request",
            post(desktop_routes::handle_lsp_request),
        )
        .route("/api/plugins", get(desktop_routes::handle_plugins_list))
        .route("/api/plugins/detail", get(desktop_routes::handle_plugins_detail))
        .route("/api/plugins/enable", post(desktop_routes::handle_plugins_enable))
        .route("/api/plugins/disable", post(desktop_routes::handle_plugins_disable))
        .route("/api/plugins/update", post(desktop_routes::handle_plugins_update))
        .route("/api/plugins/uninstall", post(desktop_routes::handle_plugins_uninstall))
        .route("/api/plugins/reload", post(desktop_routes::handle_plugins_reload))
        .route("/api/teams", get(desktop_routes::handle_teams_list))
        .route(
            "/api/teams/{name}",
            get(desktop_routes::handle_teams_get).delete(desktop_routes::handle_teams_delete),
        )
        .route(
            "/api/teams/{name}/members/{agent}/transcript",
            get(desktop_routes::handle_teams_member_transcript),
        )
        .route(
            "/api/teams/{name}/members/{agent}/messages",
            post(desktop_routes::handle_teams_member_send),
        )
        .route(
            "/api/adapters",
            get(desktop_routes::handle_adapters_get).put(desktop_routes::handle_adapters_put),
        )
        .route(
            "/api/channels/restart",
            post(desktop_routes::handle_channels_restart),
        )
        .route("/api/haha-oauth", get(desktop_routes::handle_haha_oauth_status).delete(desktop_routes::handle_haha_oauth_logout))
        .route("/api/haha-oauth/start", post(desktop_routes::handle_haha_oauth_start))
        .route(
            "/api/filesystem/browse",
            get(desktop_routes::handle_filesystem_browse),
        )
        .route("/api/search", post(desktop_routes::handle_search_files))
        .route("/api/search/sessions", post(desktop_routes::handle_search_sessions))

        .route("/api/workspace/tree", get(workspace_files::handle_workspace_tree))
        .route(
            "/api/workspace/structure-doc",
            get(workspace_files::handle_workspace_structure_doc),
        )
        .route("/api/workspace/file", get(workspace_files::handle_workspace_file_get))
        .route(
            "/api/workspace/raw-handle",
            get(workspace_files::handle_workspace_raw_handle),
        )
        .route(
            "/api/workspace/raw/{raw_id}/{*path}",
            get(workspace_files::handle_workspace_raw_get),
        )
        .route("/api/workspace/dir", post(workspace_files::handle_workspace_dir_post))
        .route("/api/workspace/move", post(workspace_files::handle_workspace_move))
        .route("/api/workspace/copy", post(workspace_files::handle_workspace_copy))
        .route("/api/workspace/entry", delete(workspace_files::handle_workspace_delete))
        .route("/api/workspace/search", get(workspace_files::handle_workspace_search))
        .route("/api/workspace/watch", get(workspace_files::handle_workspace_watch))
        .route("/api/git/status", get(git_routes::handle_git_status))
        .route("/api/python/status", get(python_env_routes::handle_status))
        .route("/api/python/discover", get(python_env_routes::handle_discover))
        .route("/api/python/create", post(python_env_routes::handle_create))
        .route("/api/python/select", post(python_env_routes::handle_select))
        .route(
            "/api/python/install_requirements",
            post(python_env_routes::handle_install_requirements),
        )
        .route(
            "/api/python/install",
            post(python_env_routes::handle_install_smart),
        )
        .route("/api/python/purge", post(python_env_routes::handle_purge))
        .route(
            "/api/python/activation",
            get(python_env_routes::handle_activation),
        )
        .route("/api/python/events", get(python_env_routes::handle_events))
        .route(
            "/api/settings/user",
            get(desktop_routes::handle_settings_user_get).put(desktop_routes::handle_settings_user_put),
        )
        .route(
            "/api/permissions/mode",
            get(desktop_routes::handle_permissions_mode_get).put(desktop_routes::handle_permissions_mode_put),
        )
        .route(
            "/api/permissions/autonomy",
            get(desktop_routes::handle_permissions_autonomy_get).put(desktop_routes::handle_permissions_autonomy_put),
        )
        .route(
            "/api/background-shell/stream",
            get(desktop_routes::handle_background_shell_stream),
        )
        .route(
            "/api/coding-modes",
            get(desktop_routes::handle_coding_modes_list),
        )
        .route(
            "/api/coding-mode",
            get(desktop_routes::handle_coding_mode_get).put(desktop_routes::handle_coding_mode_put),
        )
        .route(
            "/api/settings/cli-launcher",
            get(desktop_routes::handle_settings_cli_launcher),
        )
        .route("/api/suggestions", get(desktop_routes::handle_suggestions))
        .route(
            "/api/designer/submodes",
            get(desktop_routes::handle_designer_submodes),
        )
        .route(
            "/api/designer/design-systems",
            get(desktop_routes::handle_designer_design_systems),
        )
        .route(
            "/api/designer/prompt-templates",
            get(desktop_routes::handle_designer_prompt_templates),
        )
        .route(
            "/api/designer/html-templates",
            get(desktop_routes::handle_designer_html_templates),
        )
        .route(
            "/api/settings/sync/export",
            get(desktop_routes::handle_settings_sync_export),
        )
        .route(
            "/api/settings/sync/import",
            post(desktop_routes::handle_settings_sync_import),
        )
        .route(
            "/api/scheduled-tasks",
            get(desktop_routes::handle_scheduled_tasks_list)
                .post(desktop_routes::handle_scheduled_tasks_create),
        )
        .route(
            "/api/scheduled-tasks/runs",
            get(desktop_routes::handle_scheduled_tasks_runs),
        )
        .route(
            "/api/scheduled-tasks/{id}",
            put(desktop_routes::handle_scheduled_tasks_update)
                .delete(desktop_routes::handle_scheduled_tasks_delete),
        )
        .route(
            "/api/scheduled-tasks/{id}/run",
            post(desktop_routes::handle_scheduled_tasks_run),
        )
        .route(
            "/api/scheduled-tasks/{id}/runs",
            get(desktop_routes::handle_scheduled_tasks_task_runs),
        )
        .route("/api/tasks", get(desktop_routes::handle_cli_tasks_list_all))
        .route("/api/tasks/lists", get(desktop_routes::handle_cli_task_lists))
        .route(
            "/api/tasks/lists/{list_id}",
            get(desktop_routes::handle_cli_tasks_for_list),
        )
        .route(
            "/api/tasks/lists/{list_id}/{task_id}",
            get(desktop_routes::handle_cli_task_get),
        )
        .route(
            "/api/tasks/lists/{list_id}/reset",
            post(desktop_routes::handle_cli_tasks_reset),
        )
        .route(
            "/api/conversations",
            get(desktop_routes::handle_conversations_list),
        )
        .route("/api/desktop/status", get(desktop_routes::handle_status))
        .route(
            "/api/runtime/snapshot",
            get(desktop_routes::handle_runtime_snapshot),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            api::auth_middleware,
        ));

    let inner = Router::new()

        .route("/admin/shutdown", post(handle_admin_shutdown))
        .route("/admin/paircode", get(handle_admin_paircode))
        .route("/admin/paircode/new", post(handle_admin_paircode_new))

        .route("/health", get(handle_health))
        .route("/metrics", get(handle_metrics))
        .route("/pair", post(handle_pair))
        .route("/pair/code", get(handle_pair_code))
        .route("/webhook", post(handle_webhook))
        .route("/whatsapp", get(handle_whatsapp_verify))
        .route("/whatsapp", post(handle_whatsapp_message))
        .route("/linq", post(handle_linq_webhook))
        .route("/wati", get(handle_wati_verify))
        .route("/wati", post(handle_wati_webhook))
        .route("/nextcloud-talk", post(handle_nextcloud_talk_webhook))
        .route("/webhook/gmail", post(handle_gmail_push_webhook))

        .merge(api_routes);

    #[cfg(feature = "webauthn")]
    let inner = inner
        .route(
            "/api/webauthn/register/start",
            post(api::webauthn::handle_register_start),
        )
        .route(
            "/api/webauthn/register/finish",
            post(api::webauthn::handle_register_finish),
        )
        .route(
            "/api/webauthn/auth/start",
            post(api::webauthn::handle_auth_start),
        )
        .route(
            "/api/webauthn/auth/finish",
            post(api::webauthn::handle_auth_finish),
        )
        .route(
            "/api/webauthn/credentials",
            get(api::webauthn::handle_list_credentials),
        )
        .route(
            "/api/webauthn/credentials/{id}",
            delete(api::webauthn::handle_delete_credential),
        );

    #[cfg(feature = "plugins-wasm")]
    let inner = inner.route(
        "/api/plugins",
        get(api::plugins::plugin_routes::list_plugins),
    );

    let inner = inner

        .route("/api/suggestions", post(api::handle_api_suggestions))
        .route("/api/guardrails", get(api::handle_api_guardrails_get))
        .route("/api/tool-groups", get(api::handle_api_tool_groups))
        .route("/api/reinforcement", get(api::handle_api_reinforcement))
        .route(
            "/api/learning-features",
            get(api::handle_api_learning_features),
        )

        .route("/api/multi-agent/agents", get(api::handle_api_agents_list))
        .route("/api/multi-agent/agents/status", get(api::handle_api_agents_status))

        .route("/api/multi-agent/tasks", get(api::handle_api_tasks_status))
        .route("/api/coordination/locks", get(api::handle_api_coordination_locks))
        .route("/api/multi-agent/status", get(api::handle_api_multi_agent_status))

        .route("/api/hardware/boards", get(hardware_context::handle_hardware_boards))
        .route("/api/hardware/pin", post(hardware_context::handle_hardware_pin))
        .route(
            "/api/hardware/context",
            get(hardware_context::handle_hardware_context_get)
                .post(hardware_context::handle_hardware_context_post),
        )
        .route("/api/hardware/reload", post(hardware_context::handle_hardware_reload))
        .route("/api/rbac/status", get(api::handle_api_rbac_status))
        .route("/api/rbac/users", get(api::handle_api_rbac_users_list).post(api::handle_api_rbac_users_create))
        .route("/api/rbac/users/{user_id}", get(api::handle_api_rbac_user_get).put(api::handle_api_rbac_user_update).delete(api::handle_api_rbac_user_delete))
        .route("/api/rbac/roles", get(api::handle_api_rbac_roles_list))
        .route("/api/rbac/check", post(api::handle_api_rbac_check))
        .route("/ws/canvas/{id}", get(canvas::handle_ws_canvas))

        .route("/ws/chat", get(ws::handle_ws_chat))

        .route("/ws/{session_id}", get(ws::desktop::handle_ws_desktop))

        .route("/ws/desktop-bridge", get(desktop_bridge::handle_bridge_ws))

        .route("/api/debug/test-target", post(desktop_routes::handle_debug_test_target))

        .route("/approval/{id}/respond", post(ws::handle_approval_respond))

        .route("/ws/nodes", get(nodes::handle_ws_nodes))

        .merge(config_put_router)
        .merge(workspace_files_writes_router)
        .with_state(state)

        .merge(crate::workers::router::router())
        .merge(agent_turn_router)
        .merge(a2a_router)
        .layer(RequestBodyLimitLayer::new(MAX_BODY_SIZE))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(gateway_request_timeout_secs()),
        ))
        .layer(desktop_cors_layer());

    let app = if let Some(prefix) = path_prefix {
        let redirect_target = prefix.to_string();
        Router::new().nest(prefix, inner).route(
            &format!("{prefix}/"),
            get(|| async move { axum::response::Redirect::permanent(&redirect_target) }),
        )
    } else {
        inner
    };

    let tls_acceptor = match &config.gateway.tls {
        Some(tls_cfg) if tls_cfg.enabled => {
            let has_mtls = tls_cfg.client_auth.as_ref().is_some_and(|ca| ca.enabled);
            if has_mtls {
                tracing::info!("TLS enabled with mutual TLS (mTLS) client verification");
            } else {
                tracing::info!("TLS enabled (no client certificate requirement)");
            }
            match tls::build_tls_acceptor(tls_cfg) {
                Ok(acceptor) => Some(acceptor),
                Err(err) => {
                    tracing::warn!(
                        cert_path = %tls_cfg.cert_path,
                        key_path = %tls_cfg.key_path,
                        error = %err,
                        "gateway startup: TLS acceptor build failed (likely missing or unreadable cert/key); \
                         continuing without TLS so the desktop can still reach the local gateway. \
                         Provide valid cert paths in [gateway.tls] and restart to re-enable TLS"
                    );
                    None
                }
            }
        }
        _ => None,
    };

    crate::health::mark_component_ok("gateway");

    if let Some(tls_acceptor) = tls_acceptor {

        let app = app.into_make_service_with_connect_info::<SocketAddr>();
        let mut app = app;

        let mut shutdown_signal = shutdown_rx;
        loop {
            tokio::select! {
                conn = listener.accept() => {
                    let (tcp_stream, remote_addr) = match conn {
                        Ok(c) => c,
                        Err(e) => {
                            tracing::warn!("gateway TLS accept error (continuing): {e}");
                            continue;
                        }
                    };
                    let tls_acceptor = tls_acceptor.clone();
                    let svc = match tower::MakeService::<
                        SocketAddr,
                        hyper::Request<hyper::body::Incoming>,
                    >::make_service(&mut app, remote_addr)
                    .await
                    {
                        Ok(svc) => svc,
                        Err(e) => {
                            tracing::warn!("gateway make_service failed for {remote_addr} (continuing): {e}");
                            continue;
                        }
                    };

                    let remote_addr_clone = remote_addr;
                    let _conn_task = crate::runtime::spawn_supervised(
                        format!("gateway.tls_connection.{}", remote_addr_clone),
                        async move {
                            let tls_stream = match tls_acceptor.accept(tcp_stream).await {
                                Ok(s) => s,
                                Err(e) => {
                                    tracing::debug!("TLS handshake failed from {remote_addr_clone}: {e}");
                                    return;
                                }
                            };
                            let io = hyper_util::rt::TokioIo::new(tls_stream);
                            let hyper_svc = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                                let mut svc = svc.clone();
                                async move {
                                    tower::Service::call(&mut svc, req).await
                                }
                            });
                            if let Err(e) = hyper_util::server::conn::auto::Builder::new(
                                hyper_util::rt::TokioExecutor::new(),
                            )
                            .serve_connection(io, hyper_svc)
                            .await
                            {
                                tracing::debug!("connection error from {remote_addr_clone}: {e}");
                            }
                        },
                    );
                }
                _ = shutdown_signal.changed() => {
                    tracing::info!("\u{1F6D1} SenWeaverCoding Gateway shutting down...");
                    break;
                }
            }
        }
    } else {

        let mut shutdown_for_force = shutdown_rx.clone();
        let serve_future = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let _ = shutdown_rx.changed().await;
            tracing::info!("\u{1F6D1} SenWeaverCoding Gateway shutting down...");
        });

        let force_after = std::time::Duration::from_secs(6);
        let force_abort = async move {
            let _ = shutdown_for_force.changed().await;
            tokio::time::sleep(force_after).await;
        };

        tokio::select! {
            serve_result = serve_future => {
                serve_result?;
            }
            _ = force_abort => {
                tracing::warn!(
                    "gateway shutdown: graceful drain exceeded {}s after shutdown signal; aborting remaining connections",
                    force_after.as_secs()
                );
            }
        }
    }

    run_gateway_post_shutdown_cleanup().await;

    drop(_running_guard);

    Ok(())
}

async fn run_gateway_post_shutdown_cleanup() {
    let cleanup_started = std::time::Instant::now();

    let persist_drained = crate::gateway::ws::desktop::wait_persist_drained(
        std::time::Duration::from_secs(5),
    )
    .await;
    if persist_drained {
        tracing::info!("gateway shutdown: session persist queue drained");
    }

    if let Some(svc) = crate::services::try_get_services() {
        let lsp = svc.lsp.clone();
        let lsp_done = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            lsp.shutdown_all(),
        )
        .await;
        if lsp_done.is_err() {
            tracing::warn!("gateway shutdown: LSP shutdown_all exceeded 5s deadline");
        }
    }

    let aborted = crate::runtime::task_manager::abort_all();
    if aborted > 0 {
        tracing::info!(
            count = aborted,
            "gateway shutdown: aborted supervised background tasks"
        );
    }

    tracing::info!(
        elapsed_ms = cleanup_started.elapsed().as_millis() as u64,
        "gateway shutdown: post-shutdown cleanup complete"
    );
}

async fn handle_health(State(state): State<AppState>) -> impl IntoResponse {
    let snapshot = crate::health::snapshot();
    let degraded: Vec<&str> = snapshot
        .components
        .iter()
        .filter(|(_, health)| health.status == "error")
        .map(|(name, _)| name.as_str())
        .collect();
    let status = if degraded.is_empty() { "ok" } else { "degraded" };
    let body = serde_json::json!({
        "status": status,
        "degraded_components": degraded,
        "paired": state.pairing.is_paired(),
        "require_pairing": state.pairing.require_pairing(),
        "runtime": serde_json::to_value(&snapshot).unwrap_or_else(|_| serde_json::json!({
            "status": "error",
            "message": "failed to serialize health snapshot"
        })),
    });
    Json(body)
}

const PROMETHEUS_CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

fn prometheus_disabled_hint() -> String {
    String::from(
        "# Prometheus backend not enabled. Set [observability] backend = \"prometheus\" in config.\n",
    )
}

#[cfg(feature = "observability-prometheus")]
fn prometheus_observer_from_state(
    observer: &dyn crate::observability::Observer,
) -> Option<&crate::observability::PrometheusObserver> {
    observer
        .as_any()
        .downcast_ref::<crate::observability::PrometheusObserver>()
        .or_else(|| {
            observer
                .as_any()
                .downcast_ref::<sse::BroadcastObserver>()
                .and_then(|broadcast| {
                    broadcast
                        .inner()
                        .as_any()
                        .downcast_ref::<crate::observability::PrometheusObserver>()
                })
        })
}

async fn handle_metrics(State(state): State<AppState>) -> impl IntoResponse {
    let body = {
        #[cfg(feature = "observability-prometheus")]
        {
            if let Some(prom) = prometheus_observer_from_state(state.observer.as_ref()) {
                prom.encode()
            } else {
                prometheus_disabled_hint()
            }
        }
        #[cfg(not(feature = "observability-prometheus"))]
        {
            let _ = &state;
            prometheus_disabled_hint()
        }
    };

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, PROMETHEUS_CONTENT_TYPE)],
        body,
    )
}

#[axum::debug_handler]
async fn handle_pair(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let rate_key =
        client_key_from_request(Some(peer_addr), &headers, state.trust_forwarded_headers);
    if !state.rate_limiter.allow_pair(&rate_key) {
        tracing::warn!("/pair rate limit exceeded");
        let err = serde_json::json!({
            "error": "Too many pairing requests. Please retry later.",
            "retry_after": RATE_LIMIT_WINDOW_SECS,
        });
        return (StatusCode::TOO_MANY_REQUESTS, Json(err));
    }

    if let Err(e) = state.auth_limiter.check_rate_limit(&rate_key) {
        tracing::warn!(" - ? Pairing auth rate limit exceeded for {rate_key}");
        let err = serde_json::json!({
            "error": format!("Too many auth attempts. Try again in {}s.", e.retry_after_secs),
            "retry_after": e.retry_after_secs,
        });
        return (StatusCode::TOO_MANY_REQUESTS, Json(err));
    }

    let code = headers
        .get("X-Pairing-Code")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    match state.pairing.try_pair(code, &rate_key).await {
        Ok(Some(token)) => {
            tracing::info!(" - ? New client paired successfully");
            if let Err(err) =
                Box::pin(persist_pairing_tokens(state.config.clone(), &state.pairing)).await
            {
                tracing::error!(" - ? Pairing succeeded but token persistence failed: {err:#}");
                let body = serde_json::json!({
                    "paired": true,
                    "persisted": false,
                    "token": token,
                    "message": "Paired for this process, but failed to persist token to config.toml. Check config path and write permissions.",
                });
                return (StatusCode::OK, Json(body));
            }

            let body = serde_json::json!({
                "paired": true,
                "persisted": true,
                "token": token,
                "message": "Save this token  -  use it as Authorization: Bearer <token>"
            });
            (StatusCode::OK, Json(body))
        }
        Ok(None) => {
            state.auth_limiter.record_attempt(&rate_key);
            tracing::warn!(" - ? Pairing attempt with invalid code");
            let err = serde_json::json!({"error": "Invalid pairing code"});
            (StatusCode::FORBIDDEN, Json(err))
        }
        Err(lockout_secs) => {
            tracing::warn!(
                " - ? Pairing locked out  -  too many failed attempts ({lockout_secs}s remaining)"
            );
            let err = serde_json::json!({
                "error": format!("Too many failed attempts. Try again in {lockout_secs}s."),
                "retry_after": lockout_secs
            });
            (StatusCode::TOO_MANY_REQUESTS, Json(err))
        }
    }
}

async fn persist_pairing_tokens(config: Arc<Mutex<Config>>, pairing: &PairingGuard) -> Result<()> {
    let paired_tokens = pairing.tokens();

    let mut updated_cfg = { config.lock().clone() };
    updated_cfg.gateway.paired_tokens = paired_tokens;
    updated_cfg
        .save()
        .await
        .context("Failed to persist paired tokens to config.toml")?;

    *config.lock() = updated_cfg;
    Ok(())
}

async fn run_gateway_chat_simple(state: &AppState, message: &str) -> anyhow::Result<String> {
    let user_messages = vec![ChatMessage::user(message)];

    let current_model = state.current_model();
    let system_prompt = {
        let (workspace_dir, identity) = {
            let config_guard = state.config.lock();
            (
                config_guard.workspace_dir.clone(),
                config_guard.identity.clone(),
            )
        };
        let model_owned = current_model.clone();
        tokio::task::spawn_blocking(move || {
            crate::channels::build_system_prompt(
                &workspace_dir,
                &model_owned,
                &[],
                &[],
                Some(&identity),
                None,
            )
        })
        .await
        .map_err(|e| anyhow::anyhow!("build_system_prompt join error: {e}"))?
    };

    let mut messages = Vec::with_capacity(1 + user_messages.len());
    messages.push(ChatMessage::system(system_prompt));
    messages.extend(user_messages);

    let multimodal_config = state.config.lock().multimodal.clone();
    let prepared =
        crate::multimodal::prepare_messages_for_provider(&messages, &multimodal_config).await?;

    let provider = state.current_provider();
    provider
        .chat_with_history(&prepared.messages, &current_model, state.temperature)
        .await
}

async fn run_gateway_chat_with_tools(
    state: &AppState,
    message: &str,
    session_id: Option<&str>,
) -> anyhow::Result<String> {
    let config = state.config.lock().clone();
    Box::pin(crate::agent::process_message(config, message, session_id)).await
}

#[derive(serde::Deserialize)]
pub struct WebhookBody {
    pub message: String,
}

async fn handle_webhook(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Result<Json<WebhookBody>, axum::extract::rejection::JsonRejection>,
) -> impl IntoResponse {
    let rate_key =
        client_key_from_request(Some(peer_addr), &headers, state.trust_forwarded_headers);
    if !state.rate_limiter.allow_webhook(&rate_key) {
        tracing::warn!("/webhook rate limit exceeded");
        let err = serde_json::json!({
            "error": "Too many webhook requests. Please retry later.",
            "retry_after": RATE_LIMIT_WINDOW_SECS,
        });
        return (StatusCode::TOO_MANY_REQUESTS, Json(err));
    }

    if state.pairing.require_pairing() {
        if let Err(e) = state.auth_limiter.check_rate_limit(&rate_key) {
            tracing::warn!("Webhook: auth rate limit exceeded for {rate_key}");
            let err = serde_json::json!({
                "error": format!("Too many auth attempts. Try again in {}s.", e.retry_after_secs),
                "retry_after": e.retry_after_secs,
            });
            return (StatusCode::TOO_MANY_REQUESTS, Json(err));
        }
        let auth = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let token = auth.strip_prefix("Bearer ").unwrap_or("");
        if !state.pairing.is_authenticated(token) {
            state.auth_limiter.record_attempt(&rate_key);
            tracing::warn!("Webhook: rejected  -  not paired / invalid bearer token");
            let err = serde_json::json!({
                "error": "Unauthorized  -  pair first via POST /pair, then send Authorization: Bearer <token>"
            });
            return (StatusCode::UNAUTHORIZED, Json(err));
        }
    }

    if let Some(ref secret_hash) = state.webhook_secret_hash {
        let header_hash = headers
            .get("X-Webhook-Secret")
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(hash_webhook_secret);
        match header_hash {
            Some(val) if constant_time_eq(&val, secret_hash.as_ref()) => {}
            _ => {
                tracing::warn!("Webhook: rejected request  -  invalid or missing X-Webhook-Secret");
                let err = serde_json::json!({"error": "Unauthorized  -  invalid or missing X-Webhook-Secret header"});
                return (StatusCode::UNAUTHORIZED, Json(err));
            }
        }
    }

    let Json(webhook_body) = match body {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("Webhook JSON parse error: {e}");
            let err = serde_json::json!({
                "error": "Invalid JSON body. Expected: {\"message\": \"...\"}"
            });
            return (StatusCode::BAD_REQUEST, Json(err));
        }
    };

    if let Some(idempotency_key) = headers
        .get("X-Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if !state.idempotency_store.record_if_new(idempotency_key) {
            tracing::info!("Webhook duplicate ignored (idempotency key: {idempotency_key})");
            let body = serde_json::json!({
                "status": "duplicate",
                "idempotent": true,
                "message": "Request already processed for this idempotency key"
            });
            return (StatusCode::OK, Json(body));
        }
    }

    let message = &webhook_body.message;
    let session_id = webhook_session_id(&headers);

    if state.auto_save && !memory::should_skip_autosave_content(message) {
        let key = webhook_memory_key();
        let _ = state
            .mem
            .store(
                &key,
                message,
                MemoryCategory::Conversation,
                session_id.as_deref(),
            )
            .await;
    }

    let provider_label = state
        .config
        .lock()
        .default_provider
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let model_label = state.current_model();
    let started_at = Instant::now();

    state
        .observer
        .record_event(&crate::observability::ObserverEvent::AgentStart {
            provider: provider_label.clone(),
            model: model_label.clone(),
        });
    state
        .observer
        .record_event(&crate::observability::ObserverEvent::LlmRequest {
            provider: provider_label.clone(),
            model: model_label.clone(),
            messages_count: 1,
        });

    match run_gateway_chat_simple(&state, message).await {
        Ok(response) => {
            let duration = started_at.elapsed();
            state
                .observer
                .record_event(&crate::observability::ObserverEvent::LlmResponse {
                    provider: provider_label.clone(),
                    model: model_label.clone(),
                    duration,
                    success: true,
                    error_message: None,
                    input_tokens: None,
                    output_tokens: None,
                });
            state.observer.record_metric(
                &crate::observability::traits::ObserverMetric::RequestLatency(duration),
            );
            state
                .observer
                .record_event(&crate::observability::ObserverEvent::AgentEnd {
                    provider: provider_label,
                    model: model_label,
                    duration,
                    tokens_used: None,
                    cost_usd: None,
                });

            let body = serde_json::json!({"response": response, "model": state.current_model()});
            (StatusCode::OK, Json(body))
        }
        Err(e) => {
            let duration = started_at.elapsed();
            let sanitized = providers::sanitize_api_error(&e.to_string());

            state
                .observer
                .record_event(&crate::observability::ObserverEvent::LlmResponse {
                    provider: provider_label.clone(),
                    model: model_label.clone(),
                    duration,
                    success: false,
                    error_message: Some(sanitized.clone()),
                    input_tokens: None,
                    output_tokens: None,
                });
            state.observer.record_metric(
                &crate::observability::traits::ObserverMetric::RequestLatency(duration),
            );
            state
                .observer
                .record_event(&crate::observability::ObserverEvent::Error {
                    component: "gateway".to_string(),
                    message: sanitized.clone(),
                });
            state
                .observer
                .record_event(&crate::observability::ObserverEvent::AgentEnd {
                    provider: provider_label,
                    model: model_label,
                    duration,
                    tokens_used: None,
                    cost_usd: None,
                });

            tracing::error!("Webhook provider error: {}", sanitized);
            let err = serde_json::json!({"error": "LLM request failed"});
            (StatusCode::INTERNAL_SERVER_ERROR, Json(err))
        }
    }
}

#[derive(serde::Deserialize)]
pub struct WhatsAppVerifyQuery {
    #[serde(rename = "hub.mode")]
    pub mode: Option<String>,
    #[serde(rename = "hub.verify_token")]
    pub verify_token: Option<String>,
    #[serde(rename = "hub.challenge")]
    pub challenge: Option<String>,
}

async fn handle_whatsapp_verify(
    State(state): State<AppState>,
    Query(params): Query<WhatsAppVerifyQuery>,
) -> impl IntoResponse {
    let Some(ref wa) = state.whatsapp else {
        return (StatusCode::NOT_FOUND, "WhatsApp not configured".to_string());
    };

    let token_matches = params
        .verify_token
        .as_deref()
        .is_some_and(|t| constant_time_eq(t, wa.verify_token()));
    if params.mode.as_deref() == Some("subscribe") && token_matches {
        if let Some(ch) = params.challenge {
            tracing::info!("WhatsApp webhook verified successfully");
            return (StatusCode::OK, ch);
        }
        return (StatusCode::BAD_REQUEST, "Missing hub.challenge".to_string());
    }

    tracing::warn!("WhatsApp webhook verification failed  -  token mismatch");
    (StatusCode::FORBIDDEN, "Forbidden".to_string())
}

pub fn verify_whatsapp_signature(app_secret: &str, body: &[u8], signature_header: &str) -> bool {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let Some(hex_sig) = signature_header.strip_prefix("sha256=") else {
        return false;
    };

    let Ok(expected) = hex::decode(hex_sig) else {
        return false;
    };

    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(app_secret.as_bytes()) else {
        return false;
    };
    mac.update(body);

    mac.verify_slice(&expected).is_ok()
}

async fn handle_whatsapp_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let Some(ref wa) = state.whatsapp else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "WhatsApp not configured"})),
        );
    };

    if let Some(ref app_secret) = state.whatsapp_app_secret {
        let signature = headers
            .get("X-Hub-Signature-256")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if !verify_whatsapp_signature(app_secret, &body, signature) {
            tracing::warn!(
                "WhatsApp webhook signature verification failed (signature: {})",
                if signature.is_empty() {
                    "missing"
                } else {
                    "invalid"
                }
            );
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Invalid signature"})),
            );
        }
    }

    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid JSON payload"})),
        );
    };

    let messages = wa.parse_webhook_payload(&payload);

    if messages.is_empty() {

        return (StatusCode::OK, Json(serde_json::json!({"status": "ok"})));
    }

    for msg in &messages {
        tracing::info!(
            "WhatsApp message from {}: {}",
            msg.sender,
            truncate_with_ellipsis(&msg.content, 50)
        );
        let session_id = sender_session_id("whatsapp", msg);

        if state.auto_save && !memory::should_skip_autosave_content(&msg.content) {
            let key = whatsapp_memory_key(msg);
            let _ = state
                .mem
                .store(
                    &key,
                    &msg.content,
                    MemoryCategory::Conversation,
                    Some(&session_id),
                )
                .await;
        }

        match Box::pin(run_gateway_chat_with_tools(
            &state,
            &msg.content,
            Some(&session_id),
        ))
        .await
        {
            Ok(response) => {

                if let Err(e) = wa
                    .send(&SendMessage::new(response, &msg.reply_target))
                    .await
                {
                    tracing::error!("Failed to send WhatsApp reply: {e}");
                }
            }
            Err(e) => {
                tracing::error!("LLM error for WhatsApp message: {e:#}");
                let _ = wa
                    .send(&SendMessage::new(
                        "Sorry, I couldn't process your message right now.",
                        &msg.reply_target,
                    ))
                    .await;
            }
        }
    }

    (StatusCode::OK, Json(serde_json::json!({"status": "ok"})))
}

async fn handle_linq_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let Some(ref linq) = state.linq else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Linq not configured"})),
        );
    };

    let body_str = String::from_utf8_lossy(&body);

    if let Some(ref signing_secret) = state.linq_signing_secret {
        let timestamp = headers
            .get("X-Webhook-Timestamp")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let signature = headers
            .get("X-Webhook-Signature")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if !crate::channels::linq::verify_linq_signature(
            signing_secret,
            &body_str,
            timestamp,
            signature,
        ) {
            tracing::warn!(
                "Linq webhook signature verification failed (signature: {})",
                if signature.is_empty() {
                    "missing"
                } else {
                    "invalid"
                }
            );
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Invalid signature"})),
            );
        }
    }

    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid JSON payload"})),
        );
    };

    let messages = linq.parse_webhook_payload(&payload);

    if messages.is_empty() {

        return (StatusCode::OK, Json(serde_json::json!({"status": "ok"})));
    }

    for msg in &messages {
        tracing::info!(
            "Linq message from {}: {}",
            msg.sender,
            truncate_with_ellipsis(&msg.content, 50)
        );
        let session_id = sender_session_id("linq", msg);

        if state.auto_save && !memory::should_skip_autosave_content(&msg.content) {
            let key = linq_memory_key(msg);
            let _ = state
                .mem
                .store(
                    &key,
                    &msg.content,
                    MemoryCategory::Conversation,
                    Some(&session_id),
                )
                .await;
        }

        match Box::pin(run_gateway_chat_with_tools(
            &state,
            &msg.content,
            Some(&session_id),
        ))
        .await
        {
            Ok(response) => {

                if let Err(e) = linq
                    .send(&SendMessage::new(response, &msg.reply_target))
                    .await
                {
                    tracing::error!("Failed to send Linq reply: {e}");
                }
            }
            Err(e) => {
                tracing::error!("LLM error for Linq message: {e:#}");
                let _ = linq
                    .send(&SendMessage::new(
                        "Sorry, I couldn't process your message right now.",
                        &msg.reply_target,
                    ))
                    .await;
            }
        }
    }

    (StatusCode::OK, Json(serde_json::json!({"status": "ok"})))
}

async fn handle_wati_verify(
    State(state): State<AppState>,
    Query(params): Query<WatiVerifyQuery>,
) -> impl IntoResponse {
    if state.wati.is_none() {
        return (StatusCode::NOT_FOUND, "WATI not configured".to_string());
    }

    if let Some(challenge) = params.challenge {
        tracing::info!("WATI webhook verified successfully");
        return (StatusCode::OK, challenge);
    }

    (StatusCode::BAD_REQUEST, "Missing hub.challenge".to_string())
}

#[derive(Debug, serde::Deserialize)]
pub struct WatiVerifyQuery {
    #[serde(rename = "hub.challenge")]
    pub challenge: Option<String>,
}

async fn handle_wati_webhook(State(state): State<AppState>, body: Bytes) -> impl IntoResponse {
    let Some(ref wati) = state.wati else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "WATI not configured"})),
        );
    };

    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid JSON payload"})),
        );
    };

    let msg_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");

    let messages = if matches!(msg_type, "audio" | "voice") {

        if let Some(transcript) = wati.try_transcribe_audio(&payload).await {
            wati.parse_audio_as_message(&payload, transcript)
        } else {
            vec![]
        }
    } else {
        wati.parse_webhook_payload(&payload)
    };

    if messages.is_empty() {
        return (StatusCode::OK, Json(serde_json::json!({"status": "ok"})));
    }

    for msg in &messages {
        tracing::info!(
            "WATI message from {}: {}",
            msg.sender,
            truncate_with_ellipsis(&msg.content, 50)
        );
        let session_id = sender_session_id("wati", msg);

        if state.auto_save && !memory::should_skip_autosave_content(&msg.content) {
            let key = wati_memory_key(msg);
            let _ = state
                .mem
                .store(
                    &key,
                    &msg.content,
                    MemoryCategory::Conversation,
                    Some(&session_id),
                )
                .await;
        }

        match Box::pin(run_gateway_chat_with_tools(
            &state,
            &msg.content,
            Some(&session_id),
        ))
        .await
        {
            Ok(response) => {

                if let Err(e) = wati
                    .send(&SendMessage::new(response, &msg.reply_target))
                    .await
                {
                    tracing::error!("Failed to send WATI reply: {e}");
                }
            }
            Err(e) => {
                tracing::error!("LLM error for WATI message: {e:#}");
                let _ = wati
                    .send(&SendMessage::new(
                        "Sorry, I couldn't process your message right now.",
                        &msg.reply_target,
                    ))
                    .await;
            }
        }
    }

    (StatusCode::OK, Json(serde_json::json!({"status": "ok"})))
}

async fn handle_nextcloud_talk_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let Some(ref nextcloud_talk) = state.nextcloud_talk else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Nextcloud Talk not configured"})),
        );
    };

    let body_str = String::from_utf8_lossy(&body);

    if let Some(ref webhook_secret) = state.nextcloud_talk_webhook_secret {
        let random = headers
            .get("X-Nextcloud-Talk-Random")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let signature = headers
            .get("X-Nextcloud-Talk-Signature")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if !crate::channels::nextcloud_talk::verify_nextcloud_talk_signature(
            webhook_secret,
            random,
            &body_str,
            signature,
        ) {
            tracing::warn!(
                "Nextcloud Talk webhook signature verification failed (signature: {})",
                if signature.is_empty() {
                    "missing"
                } else {
                    "invalid"
                }
            );
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Invalid signature"})),
            );
        }
    }

    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid JSON payload"})),
        );
    };

    let messages = nextcloud_talk.parse_webhook_payload(&payload);
    if messages.is_empty() {

        return (StatusCode::OK, Json(serde_json::json!({"status": "ok"})));
    }

    for msg in &messages {
        tracing::info!(
            "Nextcloud Talk message from {}: {}",
            msg.sender,
            truncate_with_ellipsis(&msg.content, 50)
        );
        let session_id = sender_session_id("nextcloud_talk", msg);

        if state.auto_save && !memory::should_skip_autosave_content(&msg.content) {
            let key = nextcloud_talk_memory_key(msg);
            let _ = state
                .mem
                .store(
                    &key,
                    &msg.content,
                    MemoryCategory::Conversation,
                    Some(&session_id),
                )
                .await;
        }

        match Box::pin(run_gateway_chat_with_tools(
            &state,
            &msg.content,
            Some(&session_id),
        ))
        .await
        {
            Ok(response) => {
                if let Err(e) = nextcloud_talk
                    .send(&SendMessage::new(response, &msg.reply_target))
                    .await
                {
                    tracing::error!("Failed to send Nextcloud Talk reply: {e}");
                }
            }
            Err(e) => {
                tracing::error!("LLM error for Nextcloud Talk message: {e:#}");
                let _ = nextcloud_talk
                    .send(&SendMessage::new(
                        "Sorry, I couldn't process your message right now.",
                        &msg.reply_target,
                    ))
                    .await;
            }
        }
    }

    (StatusCode::OK, Json(serde_json::json!({"status": "ok"})))
}

const GMAIL_WEBHOOK_MAX_BODY: usize = 1024 * 1024;

async fn handle_gmail_push_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let Some(ref gmail_push) = state.gmail_push else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Gmail push not configured"})),
        );
    };

    if body.len() > GMAIL_WEBHOOK_MAX_BODY {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(serde_json::json!({"error": "Request body too large"})),
        );
    }

    let secret = gmail_push.resolve_webhook_secret();
    if !secret.is_empty() {
        let provided = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|auth| auth.strip_prefix("Bearer "))
            .unwrap_or("");

        if provided != secret {
            tracing::warn!("Gmail push webhook: unauthorized request");
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Unauthorized"})),
            );
        }
    }

    let body_str = String::from_utf8_lossy(&body);
    let envelope: crate::channels::gmail_push::PubSubEnvelope =
        match serde_json::from_str(&body_str) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Gmail push webhook: invalid payload: {e}");
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "Invalid Pub/Sub envelope"})),
                );
            }
        };

    let channel = Arc::clone(gmail_push);
    let _ = crate::runtime::spawn_supervised("gateway.gmail_push_notification", async move {
        if let Err(e) = channel.handle_notification(&envelope).await {
            tracing::error!("Gmail push notification processing failed: {e:#}");
        }
    });

    (StatusCode::OK, Json(serde_json::json!({"status": "ok"})))
}

#[derive(serde::Serialize)]
struct AdminResponse {
    success: bool,
    message: String,
}

fn require_localhost(peer: &SocketAddr) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if peer.ip().is_loopback() {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "Admin endpoints are restricted to localhost"
            })),
        ))
    }
}

async fn handle_admin_shutdown(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    require_localhost(&peer)?;
    tracing::info!(" - ? Admin shutdown request received  -  initiating graceful shutdown");

    let body = AdminResponse {
        success: true,
        message: "Gateway shutdown initiated".to_string(),
    };

    let _ = state.shutdown_tx.send(true);

    Ok((StatusCode::OK, Json(body)))
}

async fn handle_admin_paircode(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    require_localhost(&peer)?;
    let code = state.pairing.pairing_code();

    let body = if let Some(c) = code {
        serde_json::json!({
            "success": true,
            "pairing_required": state.pairing.require_pairing(),
            "pairing_code": c,
            "message": "Use this one-time code to pair"
        })
    } else {
        serde_json::json!({
            "success": true,
            "pairing_required": state.pairing.require_pairing(),
            "pairing_code": null,
            "message": if state.pairing.require_pairing() {
                "Pairing is active but no new code available (already paired or code expired)"
            } else {
                "Pairing is disabled for this gateway"
            }
        })
    };

    Ok((StatusCode::OK, Json(body)))
}

async fn handle_admin_paircode_new(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    require_localhost(&peer)?;
    match state.pairing.generate_new_pairing_code() {
        Some(code) => {
            tracing::info!(" - ? New pairing code generated via admin endpoint");
            let body = serde_json::json!({
                "success": true,
                "pairing_required": state.pairing.require_pairing(),
                "pairing_code": code,
                "message": "New pairing code generated  -  use this one-time code to pair"
            });
            Ok((StatusCode::OK, Json(body)))
        }
        None => {
            let body = serde_json::json!({
                "success": false,
                "pairing_required": false,
                "pairing_code": null,
                "message": "Pairing is disabled for this gateway"
            });
            Ok((StatusCode::BAD_REQUEST, Json(body)))
        }
    }
}

async fn handle_pair_code(State(state): State<AppState>) -> impl IntoResponse {
    let require = state.pairing.require_pairing();
    let is_paired = state.pairing.is_paired();

    let code = if require && !is_paired {
        state.pairing.pairing_code()
    } else {
        None
    };

    let body = serde_json::json!({
        "success": true,
        "pairing_required": require,
        "pairing_code": code,
    });

    (StatusCode::OK, Json(body))
}
