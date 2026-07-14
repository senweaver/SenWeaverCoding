// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Json},
};
use serde::{Deserialize, Serialize};

const MASKED_SECRET: &str = "***MASKED***";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayAuthLevel {

    Anonymous,

    LocalOnly,

    Bearer,

    Mutual,
}

impl GatewayAuthLevel {

    pub fn enforce(
        self,
        state: &AppState,
        headers: &HeaderMap,
    ) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
        match self {
            GatewayAuthLevel::Anonymous => Ok(()),
            GatewayAuthLevel::LocalOnly => {
                if is_request_from_localhost(headers) {
                    Ok(())
                } else {
                    require_bearer(state, headers)
                }
            }
            GatewayAuthLevel::Bearer | GatewayAuthLevel::Mutual => require_bearer(state, headers),
        }
    }
}

fn require_bearer(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let token = extract_bearer_token(headers).unwrap_or("");
    if state.pairing.is_authenticated(token) {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "Unauthorized  - a valid Bearer token is required for this endpoint"
            })),
        ))
    }
}

fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|auth| auth.strip_prefix("Bearer "))
}

pub(in crate::gateway) fn require_auth(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    require_auth_with_peer(state, headers, None)
}

pub(in crate::gateway) fn require_auth_with_peer(
    state: &AppState,
    headers: &HeaderMap,
    peer: Option<std::net::SocketAddr>,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if state.exposed {
        let token = extract_bearer_token(headers).unwrap_or("");
        if !state.pairing.is_authenticated_strict(token) {
            return Err(unauthorized(
                "Unauthorized  - this gateway is exposed (public bind/tunnel); a valid Bearer \
                 token is mandatory. Pair via POST /pair, then send Authorization: Bearer <token>",
            ));
        }
        if let Some(secret) = state.signing_secret.as_deref() {
            verify_request_signature(secret, headers)?;
        }
        return Ok(());
    }

    if !state.pairing.require_pairing() && peer_is_loopback(headers, peer) {
        return Ok(());
    }

    let token = extract_bearer_token(headers).unwrap_or("");
    if state.pairing.is_authenticated(token) {
        Ok(())
    } else {
        Err(unauthorized(
            "Unauthorized  - pair first via POST /pair, then send Authorization: Bearer <token>",
        ))
    }
}

fn unauthorized(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": msg })),
    )
}

fn peer_is_loopback(headers: &HeaderMap, peer: Option<std::net::SocketAddr>) -> bool {
    if let Some(addr) = peer {
        return addr.ip().is_loopback();
    }
    is_request_from_localhost(headers)
}

fn verify_request_signature(
    secret: &str,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let ts = headers
        .get("x-sen-timestamp")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let sig = headers
        .get("x-sen-signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if ts.is_empty() || sig.is_empty() {
        return Err(unauthorized(
            "Unauthorized  - request signing is required on this exposed gateway; send \
             X-Sen-Timestamp and X-Sen-Signature headers",
        ));
    }

    let ts_secs: i64 = ts.parse().unwrap_or(0);
    let now = chrono::Utc::now().timestamp();
    if (now - ts_secs).abs() > 300 {
        return Err(unauthorized(
            "Unauthorized  - request signature timestamp is stale or invalid (replay protection)",
        ));
    }

    let mut mac = match Hmac::<Sha256>::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return Err(unauthorized("Unauthorized  - signing misconfigured")),
    };
    mac.update(ts.as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());
    if constant_time_eq(expected.as_bytes(), sig.as_bytes()) {
        Ok(())
    } else {
        Err(unauthorized(
            "Unauthorized  - request signature verification failed",
        ))
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn is_request_from_localhost(headers: &HeaderMap) -> bool {
    if let Some(fwd) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        let first_ip = fwd.split(',').next().unwrap_or("").trim();
        return first_ip == "127.0.0.1" || first_ip == "::1";
    }
    // No peer socket and no XFF header: we cannot prove the request is
    // loopback, so do NOT grant the no-auth localhost shortcut. Genuine local
    // callers reach auth through require_auth_with_peer, which sees the real
    // ConnectInfo socket address.
    false
}

pub async fn auth_middleware(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, (StatusCode, Json<serde_json::Value>)> {
    let peer = request
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0);
    require_auth_with_peer(&state, request.headers(), peer)?;
    Ok(next.run(request).await)
}

#[derive(Deserialize)]
pub struct MemoryQuery {
    pub query: Option<String>,
    pub category: Option<String>,

    pub since: Option<String>,

    pub until: Option<String>,
}

#[derive(Deserialize)]
pub struct MemoryStoreBody {
    pub key: String,
    pub content: String,
    pub category: Option<String>,
}

#[derive(Deserialize)]
pub struct CronRunsQuery {
    pub limit: Option<u32>,
}

#[derive(Deserialize)]
pub struct CronAddBody {
    pub name: Option<String>,
    pub schedule: String,
    pub command: Option<String>,
    pub job_type: Option<String>,
    pub prompt: Option<String>,
    pub delivery: Option<crate::cron::DeliveryConfig>,
    pub session_target: Option<String>,
    pub model: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub delete_after_run: Option<bool>,
}

#[derive(Deserialize)]
pub struct CronPatchBody {
    pub name: Option<String>,
    pub schedule: Option<String>,
    pub command: Option<String>,
    pub prompt: Option<String>,
}

pub async fn handle_api_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock().clone();
    let health = crate::health::snapshot();

    let mut channels = serde_json::Map::new();

    for (channel, present) in config.channels_config.channels() {
        channels.insert(channel.name().to_string(), serde_json::Value::Bool(present));
    }

    let body = serde_json::json!({
        "provider": config.default_provider,
        "model": state.current_model(),
        "temperature": state.temperature,
        "uptime_seconds": health.uptime_seconds,
        "gateway_port": config.gateway.port,
        "locale": "en",
        "memory_backend": state.mem.name(),
        "paired": state.pairing.is_paired(),
        "channels": channels,
        "health": health,
    });

    Json(body).into_response()
}

pub async fn handle_api_config_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock().clone();

    let masked_config = mask_sensitive_fields(&config);
    let toml_str = match toml::to_string_pretty(&masked_config) {
        Ok(s) => s,
        Err(_e) => {

            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to serialize configuration"})),
            )
                .into_response();
        }
    };

    Json(serde_json::json!({
        "format": "toml",
        "content": toml_str,
    }))
    .into_response()
}

pub async fn handle_api_config_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let incoming: crate::config::Config = match toml::from_str(&body) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Config PUT: invalid TOML: {e}");
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid configuration format"})),
            )
                .into_response();
        }
    };

    let current_config = state.config.lock().clone();
    let new_config = hydrate_config_for_save(incoming, &current_config);

    if let Err(e) = new_config.validate() {
        tracing::warn!("Config PUT: validation failed: {e}");
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Configuration validation failed"})),
        )
            .into_response();
    }

    if let Err(e) = new_config.save().await {
        tracing::error!("Config PUT: save failed: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to save configuration"})),
        )
            .into_response();
    }

    let provider_relevant_changed = new_config.default_provider != current_config.default_provider
        || new_config.default_model != current_config.default_model
        || new_config.api_key != current_config.api_key
        || new_config.api_url != current_config.api_url
        || new_config.api_path != current_config.api_path
        || serde_json::to_value(&new_config.model_providers).ok()
            != serde_json::to_value(&current_config.model_providers).ok()
        || serde_json::to_value(&new_config.model_routes).ok()
            != serde_json::to_value(&current_config.model_routes).ok()
        || serde_json::to_value(&new_config.reliability).ok()
            != serde_json::to_value(&current_config.reliability).ok();

    *state.config.lock() = new_config.clone();
    state.push_live_config(new_config);
    if provider_relevant_changed {
        state.rebuild_runtime_from_config_async().await;
    }

    Json(serde_json::json!({"status": "ok"})).into_response()
}

#[derive(serde::Deserialize)]
pub struct ProviderUpdateRequest {
    pub provider: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    pub api_url: Option<String>,
    pub gateway_port: Option<u16>,
    pub gateway_host: Option<String>,
    pub gateway_require_pairing: Option<bool>,
}

pub async fn handle_api_provider_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock();

    let masked_api_key = config
        .api_key
        .as_ref()
        .map(|k| crate::security::SecretStore::mask_secret(k));

    Json(serde_json::json!({
        "provider": config.default_provider,
        "model": config.default_model,
        "api_key": masked_api_key,
        "api_url": config.api_url,
        "gateway_port": config.gateway.port,
        "gateway_host": config.gateway.host,
        "gateway_require_pairing": config.gateway.require_pairing,
    }))
    .into_response()
}

pub async fn handle_api_channels_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock();
    let mut list: Vec<serde_json::Value> = Vec::new();

    if let Some(cfg) = config.channels_config.telegram.as_ref() {
        push_channel_to_list(&mut list, "telegram", cfg);
    }
    if let Some(cfg) = config.channels_config.discord.as_ref() {
        push_channel_to_list(&mut list, "discord", cfg);
    }
    if let Some(cfg) = config.channels_config.slack.as_ref() {
        push_channel_to_list(&mut list, "slack", cfg);
    }
    if let Some(cfg) = config.channels_config.mattermost.as_ref() {
        push_channel_to_list(&mut list, "mattermost", cfg);
    }
    if let Some(cfg) = config.channels_config.webhook.as_ref() {
        push_channel_to_list(&mut list, "webhook", cfg);
    }
    if let Some(cfg) = config.channels_config.matrix.as_ref() {
        push_channel_to_list(&mut list, "matrix", cfg);
    }
    if let Some(cfg) = config.channels_config.whatsapp.as_ref() {
        push_channel_to_list(&mut list, "whatsapp", cfg);
    }
    if let Some(cfg) = config.channels_config.linq.as_ref() {
        push_channel_to_list(&mut list, "linq", cfg);
    }
    if let Some(cfg) = config.channels_config.nextcloud_talk.as_ref() {
        push_channel_to_list(&mut list, "nextcloud_talk", cfg);
    }
    if let Some(cfg) = config.channels_config.wati.as_ref() {
        push_channel_to_list(&mut list, "wati", cfg);
    }
    if let Some(cfg) = config.channels_config.irc.as_ref() {
        push_channel_to_list(&mut list, "irc", cfg);
    }
    if let Some(cfg) = config.channels_config.lark.as_ref() {
        push_channel_to_list(&mut list, "lark", cfg);
    }
    if let Some(cfg) = config.channels_config.feishu.as_ref() {
        push_channel_to_list(&mut list, "feishu", cfg);
    }
    if let Some(cfg) = config.channels_config.dingtalk.as_ref() {
        push_channel_to_list(&mut list, "dingtalk", cfg);
    }
    if let Some(cfg) = config.channels_config.wecom.as_ref() {
        push_channel_to_list(&mut list, "wecom", cfg);
    }
    if let Some(cfg) = config.channels_config.qq.as_ref() {
        push_channel_to_list(&mut list, "qq", cfg);
    }
    if let Some(cfg) = config.channels_config.twitter.as_ref() {
        push_channel_to_list(&mut list, "twitter", cfg);
    }
    if let Some(cfg) = config.channels_config.reddit.as_ref() {
        push_channel_to_list(&mut list, "reddit", cfg);
    }
    if let Some(cfg) = config.channels_config.bluesky.as_ref() {
        push_channel_to_list(&mut list, "bluesky", cfg);
    }
    if let Some(cfg) = config.channels_config.email.as_ref() {
        push_channel_to_list(&mut list, "email", cfg);
    }
    if let Some(cfg) = config.channels_config.gmail_push.as_ref() {
        push_channel_to_list(&mut list, "gmail_push", cfg);
    }
    if let Some(cfg) = config.channels_config.signal.as_ref() {
        push_channel_to_list(&mut list, "signal", cfg);
    }
    if let Some(cfg) = config.channels_config.voice_call.as_ref() {
        push_channel_to_list(&mut list, "voice_call", cfg);
    }

    Json(serde_json::json!({ "channels": list })).into_response()
}

pub async fn handle_api_channels_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let Some(channels) = body.get("channels").and_then(|v| v.as_array()) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "missing channels array"})),
        )
            .into_response();
    };

    let mut config = state.config.lock().clone();

    for entry in channels {
        let name = match entry.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => continue,
        };
        let cfg = match entry.get("config") {
            Some(c) => c,
            None => continue,
        };

        match name {
            "telegram" => {
                if let Ok(parsed) =
                    serde_json::from_value::<crate::config::schema::TelegramConfig>(cfg.clone())
                {
                    config.channels_config.telegram = Some(parsed);
                }
            }
            "discord" => {
                if let Ok(parsed) =
                    serde_json::from_value::<crate::config::schema::DiscordConfig>(cfg.clone())
                {
                    config.channels_config.discord = Some(parsed);
                }
            }
            "slack" => {
                if let Ok(parsed) =
                    serde_json::from_value::<crate::config::schema::SlackConfig>(cfg.clone())
                {
                    config.channels_config.slack = Some(parsed);
                }
            }
            "mattermost" => {
                if let Ok(parsed) =
                    serde_json::from_value::<crate::config::schema::MattermostConfig>(cfg.clone())
                {
                    config.channels_config.mattermost = Some(parsed);
                }
            }
            "webhook" => {
                if let Ok(parsed) =
                    serde_json::from_value::<crate::config::schema::WebhookConfig>(cfg.clone())
                {
                    config.channels_config.webhook = Some(parsed);
                }
            }
            "matrix" => {
                if let Ok(parsed) =
                    serde_json::from_value::<crate::config::schema::MatrixConfig>(cfg.clone())
                {
                    config.channels_config.matrix = Some(parsed);
                }
            }
            "whatsapp" => {
                if let Ok(parsed) =
                    serde_json::from_value::<crate::config::schema::WhatsAppConfig>(cfg.clone())
                {
                    config.channels_config.whatsapp = Some(parsed);
                }
            }
            "linq" => {
                if let Ok(parsed) =
                    serde_json::from_value::<crate::config::schema::LinqConfig>(cfg.clone())
                {
                    config.channels_config.linq = Some(parsed);
                }
            }
            "nextcloud_talk" => {
                if let Ok(parsed) = serde_json::from_value::<
                    crate::config::schema::NextcloudTalkConfig,
                >(cfg.clone())
                {
                    config.channels_config.nextcloud_talk = Some(parsed);
                }
            }
            "wati" => {
                if let Ok(parsed) =
                    serde_json::from_value::<crate::config::schema::WatiConfig>(cfg.clone())
                {
                    config.channels_config.wati = Some(parsed);
                }
            }
            _ => {}
        }
    }

    if let Err(e) = config.validate() {
        tracing::warn!("Provider PUT: invalid config: {e}");
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid configuration"})),
        )
            .into_response();
    }

    if let Err(e) = config.save().await {
        tracing::error!("Provider PUT: save failed: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to save configuration"})),
        )
            .into_response();
    }

    *state.config.lock() = config.clone();
    state.push_live_config(config);

    Json(serde_json::json!({"status": "ok"})).into_response()
}

pub async fn handle_api_provider_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::extract::Json<ProviderUpdateRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let mut config = state.config.lock().clone();

    if let Some(p) = &body.provider {
        if !p.is_empty() {
            config.default_provider = Some(p.clone());
        }
    }
    if let Some(m) = &body.model {
        if !m.is_empty() {
            config.default_model = Some(m.clone());
        }
    }
    if let Some(u) = &body.api_url {
        config.api_url = if u.is_empty() { None } else { Some(u.clone()) };
    }

    match &body.api_key {
        Some(k) if k.is_empty() => config.api_key = None,
        Some(k) => config.api_key = Some(k.clone()),
        None => {}
    }

    if let Some(port) = body.gateway_port {
        config.gateway.port = port;
    }
    if let Some(host) = &body.gateway_host {
        if !host.is_empty() {
            config.gateway.host = host.clone();
        }
    }
    if let Some(rp) = body.gateway_require_pairing {
        config.gateway.require_pairing = rp;
    }

    if let Err(e) = config.validate() {
        return (StatusCode::BAD_REQUEST, {
            tracing::warn!("Invalid config: {e}");
            Json(serde_json::json!({"error": "Invalid configuration"}))
        })
            .into_response();
    }

    if let Err(e) = config.save().await {
        return (StatusCode::INTERNAL_SERVER_ERROR, {
            tracing::error!("Failed to save config: {e}");
            Json(serde_json::json!({"error": "Failed to save configuration"}))
        })
            .into_response();
    }

    *state.config.lock() = config.clone();
    state.push_live_config(config);
    state.rebuild_runtime_from_config_async().await;

    Json(serde_json::json!({"status": "ok"})).into_response()
}

pub async fn handle_api_tools(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let tools: Vec<serde_json::Value> = state
        .tools_registry
        .iter()
        .map(|spec| {
            serde_json::json!({
                "name": spec.name,
                "description": spec.description,
                "parameters": spec.parameters,
            })
        })
        .collect();

    Json(serde_json::json!({"tools": tools})).into_response()
}

#[derive(Deserialize)]
pub struct SkillsPutBody {

    #[serde(default)]
    pub disabled_skills: Option<Vec<String>>,

    #[serde(default)]
    pub prompt_injection_mode: Option<String>,
}

pub async fn handle_api_skills_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SkillsPutBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let mut config = state.config.lock().clone();
    if let Some(disabled) = body.disabled_skills {
        config.skills.disabled_skills = disabled
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    if let Some(raw) = body.prompt_injection_mode.as_deref() {
        match raw.trim().to_ascii_lowercase().as_str() {
            "full" => {
                config.skills.prompt_injection_mode =
                    crate::config::SkillsPromptInjectionMode::Full;
            }
            "compact" => {
                config.skills.prompt_injection_mode =
                    crate::config::SkillsPromptInjectionMode::Compact;
            }
            other => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": format!("invalid prompt_injection_mode: {other}")
                    })),
                )
                    .into_response();
            }
        }
    }

    if let Err(e) = config.validate() {
        return (StatusCode::BAD_REQUEST, {
            tracing::warn!("Invalid config: {e}");
            Json(serde_json::json!({"error": "Invalid configuration"}))
        })
            .into_response();
    }

    if let Err(e) = config.save().await {
        return (StatusCode::INTERNAL_SERVER_ERROR, {
            tracing::error!("Failed to save config: {e}");
            Json(serde_json::json!({"error": "Failed to save configuration"}))
        })
            .into_response();
    }

    *state.config.lock() = config.clone();

    state.push_live_config(config);

    Json(serde_json::json!({"status": "ok"})).into_response()
}

pub async fn handle_api_cron_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock().clone();
    match crate::cron::list_jobs(&config) {
        Ok(jobs) => Json(serde_json::json!({"jobs": jobs})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, {
            tracing::error!("Failed to list cron jobs: {e}");
            Json(serde_json::json!({"error": "Failed to list cron jobs"}))
        })
            .into_response(),
    }
}

pub async fn handle_api_cron_add(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CronAddBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let CronAddBody {
        name,
        schedule,
        command,
        job_type,
        prompt,
        delivery,
        session_target,
        model,
        allowed_tools,
        delete_after_run,
    } = body;

    let config = state.config.lock().clone();
    let schedule = crate::cron::Schedule::Cron {
        expr: schedule,
        tz: None,
    };
    if let Err(e) = crate::cron::validate_delivery_config(delivery.as_ref()) {
        return (StatusCode::BAD_REQUEST, {
            tracing::error!("Failed to add cron job: {e}");
            Json(serde_json::json!({"error": "Failed to add cron job"}))
        })
            .into_response();
    }

    let is_agent =
        matches!(job_type.as_deref(), Some("agent")) || (job_type.is_none() && prompt.is_some());

    let result = if is_agent {
        let prompt = match prompt.as_deref() {
            Some(p) if !p.trim().is_empty() => p,
            _ => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "Missing 'prompt' for agent job"})),
                )
                    .into_response();
            }
        };

        let session_target = session_target
            .as_deref()
            .map(crate::cron::SessionTarget::parse)
            .unwrap_or_default();

        let default_delete = matches!(schedule, crate::cron::Schedule::At { .. });
        let delete_after_run = delete_after_run.unwrap_or(default_delete);

        crate::cron::add_agent_job(
            &config,
            name,
            schedule,
            prompt,
            crate::cron::AgentJobOptions {
                session_target,
                model,
                delivery,
                delete_after_run,
                allowed_tools,
                ..Default::default()
            },
        )
    } else {
        let command = match command.as_deref() {
            Some(c) if !c.trim().is_empty() => c,
            _ => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "Missing 'command' for shell job"})),
                )
                    .into_response();
            }
        };

        crate::cron::add_shell_job_with_approval(&config, name, schedule, command, delivery, false)
    };

    match result {
        Ok(job) => Json(serde_json::json!({"status": "ok", "job": job})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, {
            tracing::error!("Failed to add cron job: {e}");
            Json(serde_json::json!({"error": "Failed to add cron job"}))
        })
            .into_response(),
    }
}

pub async fn handle_api_cron_runs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(params): Query<CronRunsQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let limit = params.limit.unwrap_or(20).clamp(1, 100) as usize;
    let config = state.config.lock().clone();

    if let Err(e) = crate::cron::get_job(&config, &id) {
        return (StatusCode::NOT_FOUND, {
            tracing::warn!("Cron job not found: {e}");
            Json(serde_json::json!({"error": "Cron job not found"}))
        })
            .into_response();
    }

    match crate::cron::list_runs(&config, &id, limit) {
        Ok(runs) => {
            let runs_json: Vec<serde_json::Value> = runs
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.id,
                        "job_id": r.job_id,
                        "started_at": r.started_at.to_rfc3339(),
                        "finished_at": r.finished_at.to_rfc3339(),
                        "status": r.status,
                        "output": r.output,
                        "duration_ms": r.duration_ms,
                    })
                })
                .collect();
            Json(serde_json::json!({"runs": runs_json})).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, {
            tracing::error!("Failed to list cron runs: {e}");
            Json(serde_json::json!({"error": "Failed to list cron runs"}))
        })
            .into_response(),
    }
}

pub async fn handle_api_cron_patch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<CronPatchBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock().clone();

    let schedule = match body.schedule {
        Some(expr) if !expr.trim().is_empty() => Some(crate::cron::Schedule::Cron {
            expr: expr.trim().to_string(),
            tz: None,
        }),
        _ => None,
    };

    let existing = match crate::cron::get_job(&config, &id) {
        Ok(j) => j,
        Err(e) => {
            return (StatusCode::NOT_FOUND, {
                tracing::warn!("Cron job not found: {e}");
                Json(serde_json::json!({"error": "Cron job not found"}))
            })
                .into_response();
        }
    };
    let is_agent = matches!(existing.job_type, crate::cron::JobType::Agent);
    let (patch_command, patch_prompt) = if is_agent {
        (None, body.command.or(body.prompt))
    } else {
        (body.command.or(body.prompt), None)
    };

    let patch = crate::cron::CronJobPatch {
        name: body.name,
        schedule,
        command: patch_command,
        prompt: patch_prompt,
        ..crate::cron::CronJobPatch::default()
    };

    match crate::cron::update_shell_job_with_approval(&config, &id, patch, false) {
        Ok(job) => Json(serde_json::json!({"status": "ok", "job": job})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, {
            tracing::error!("Failed to update cron job: {e}");
            Json(serde_json::json!({"error": "Failed to update cron job"}))
        })
            .into_response(),
    }
}

pub async fn handle_api_cron_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock().clone();
    match crate::cron::remove_job(&config, &id) {
        Ok(()) => Json(serde_json::json!({"status": "ok"})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, {
            tracing::error!("Failed to remove cron job: {e}");
            Json(serde_json::json!({"error": "Failed to remove cron job"}))
        })
            .into_response(),
    }
}

pub async fn handle_api_cron_settings_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock().clone();
    Json(serde_json::json!({
        "enabled": config.cron.enabled,
        "catch_up_on_startup": config.cron.catch_up_on_startup,
        "max_run_history": config.cron.max_run_history,
    }))
    .into_response()
}

pub async fn handle_api_cron_settings_patch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let mut config = state.config.lock().clone();

    if let Some(v) = body.get("enabled").and_then(|v| v.as_bool()) {
        config.cron.enabled = v;
    }
    if let Some(v) = body.get("catch_up_on_startup").and_then(|v| v.as_bool()) {
        config.cron.catch_up_on_startup = v;
    }
    if let Some(v) = body.get("max_run_history").and_then(|v| v.as_u64()) {
        config.cron.max_run_history = u32::try_from(v).unwrap_or(u32::MAX);
    }

    if let Err(e) = config.save().await {
        return (StatusCode::INTERNAL_SERVER_ERROR, {
            tracing::error!("Failed to save config: {e}");
            Json(serde_json::json!({"error": "Failed to save configuration"}))
        })
            .into_response();
    }

    *state.config.lock() = config.clone();
    state.push_live_config(config.clone());

    Json(serde_json::json!({
        "status": "ok",
        "enabled": config.cron.enabled,
        "catch_up_on_startup": config.cron.catch_up_on_startup,
        "max_run_history": config.cron.max_run_history,
    }))
    .into_response()
}

pub async fn handle_api_integrations(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock().clone();
    let entries = crate::integrations::registry::all_integrations();

    let integrations: Vec<serde_json::Value> = entries
        .iter()
        .map(|entry| {
            let status = (entry.status_fn)(&config);
            serde_json::json!({
                "name": entry.name,
                "description": entry.description,
                "category": entry.category,
                "status": status,
            })
        })
        .collect();

    Json(serde_json::json!({"integrations": integrations})).into_response()
}

pub async fn handle_api_integrations_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock().clone();
    let entries = crate::integrations::registry::all_integrations();

    let mut settings = serde_json::Map::new();
    for entry in &entries {
        let status = (entry.status_fn)(&config);
        let enabled = matches!(status, crate::integrations::IntegrationStatus::Active);
        settings.insert(
            entry.name.to_string(),
            serde_json::json!({
                "enabled": enabled,
                "category": entry.category,
                "status": status,
            }),
        );
    }

    Json(serde_json::json!({"settings": settings})).into_response()
}

pub async fn handle_api_doctor(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock().clone();
    let results = tokio::task::spawn_blocking(move || crate::doctor::diagnose(&config))
        .await
        .unwrap_or_default();

    let ok_count = results
        .iter()
        .filter(|r| r.severity == crate::doctor::Severity::Ok)
        .count();
    let warn_count = results
        .iter()
        .filter(|r| r.severity == crate::doctor::Severity::Warn)
        .count();
    let error_count = results
        .iter()
        .filter(|r| r.severity == crate::doctor::Severity::Error)
        .count();

    Json(serde_json::json!({
        "results": results,
        "summary": {
            "ok": ok_count,
            "warnings": warn_count,
            "errors": error_count,
        }
    }))
    .into_response()
}

pub async fn handle_api_memory_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<MemoryQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    if params.query.is_some() || params.since.is_some() || params.until.is_some() {
        let query = params.query.as_deref().unwrap_or("");
        let since = params.since.as_deref();
        let until = params.until.as_deref();
        match state.mem.recall(query, 50, None, since, until).await {
            Ok(entries) => Json(serde_json::json!({"entries": entries})).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, {
                tracing::error!("Memory recall failed: {e}");
                Json(serde_json::json!({"error": "Memory recall failed"}))
            })
                .into_response(),
        }
    } else {

        let category = params.category.as_deref().map(|cat| match cat {
            "core" => crate::memory::MemoryCategory::Core,
            "daily" => crate::memory::MemoryCategory::Daily,
            "conversation" => crate::memory::MemoryCategory::Conversation,
            other => crate::memory::MemoryCategory::Custom(other.to_string()),
        });

        match state.mem.list(category.as_ref(), None).await {
            Ok(entries) => Json(serde_json::json!({"entries": entries})).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, {
                tracing::error!("Memory list failed: {e}");
                Json(serde_json::json!({"error": "Memory list failed"}))
            })
                .into_response(),
        }
    }
}

pub async fn handle_api_memory_store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<MemoryStoreBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let category = body
        .category
        .as_deref()
        .map(|cat| match cat {
            "core" => crate::memory::MemoryCategory::Core,
            "daily" => crate::memory::MemoryCategory::Daily,
            "conversation" => crate::memory::MemoryCategory::Conversation,
            other => crate::memory::MemoryCategory::Custom(other.to_string()),
        })
        .unwrap_or(crate::memory::MemoryCategory::Core);

    match state
        .mem
        .store(&body.key, &body.content, category, None)
        .await
    {
        Ok(()) => Json(serde_json::json!({"status": "ok"})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, {
            tracing::error!("Memory store failed: {e}");
            Json(serde_json::json!({"error": "Memory store failed"}))
        })
            .into_response(),
    }
}

pub async fn handle_api_memory_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(key): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    match state.mem.forget(&key).await {
        Ok(deleted) => {
            Json(serde_json::json!({"status": "ok", "deleted": deleted})).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, {
            tracing::error!("Memory forget failed: {e}");
            Json(serde_json::json!({"error": "Memory forget failed"}))
        })
            .into_response(),
    }
}

pub async fn handle_api_cost(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    if let Some(ref tracker) = state.cost_tracker {
        match tracker.get_summary() {
            Ok(summary) => Json(serde_json::json!({"cost": summary})).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, {
                tracing::error!("Cost summary failed: {e}");
                Json(serde_json::json!({"error": "Cost summary failed"}))
            })
                .into_response(),
        }
    } else {
        Json(serde_json::json!({
            "cost": {
                "session_cost_usd": 0.0,
                "daily_cost_usd": 0.0,
                "monthly_cost_usd": 0.0,
                "total_tokens": 0,
                "request_count": 0,
                "by_model": {},
            }
        }))
        .into_response()
    }
}

pub async fn handle_api_cli_tools(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let tools = tokio::task::spawn_blocking(|| {
        crate::tools::cli_discovery::discover_cli_tools(&[], &[])
    })
    .await
    .unwrap_or_default();

    Json(serde_json::json!({"cli_tools": tools})).into_response()
}

pub async fn handle_api_health(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let snapshot = crate::health::snapshot();
    Json(serde_json::json!({"health": snapshot})).into_response()
}

pub async fn handle_api_metrics(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let body = match crate::services::try_get_services() {
        Some(svc) => svc.agent_metrics.render_prometheus(),
        None => String::from("# HELP sen_bootstrap\n# TYPE sen_bootstrap gauge\nsen_bootstrap 0\n"),
    };

    (
        axum::http::StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
        .into_response()
}

fn is_masked_secret(value: &str) -> bool {
    value == MASKED_SECRET
}

fn mask_field(obj: &mut serde_json::Map<String, serde_json::Value>, key: &str) {
    if let Some(v) = obj.get_mut(key) {
        if let Some(s) = v.as_str() {
            if !s.is_empty() && !is_masked_secret(s) {
                *v = serde_json::json!(crate::security::SecretStore::mask_secret(s));
            }
        }
    }
}

fn push_channel_to_list(
    list: &mut Vec<serde_json::Value>,
    name: &str,
    cfg: &impl serde::Serialize,
) {
    let mut obj = match serde_json::to_value(cfg) {
        Ok(v) => v,
        Err(_) => return,
    };
    if let Some(obj) = obj.as_object_mut() {
        mask_field(obj, "bot_token");
        mask_field(obj, "app_token");
        mask_field(obj, "access_token");
        mask_field(obj, "secret");
        mask_field(obj, "api_token");
        mask_field(obj, "app_secret");
        mask_field(obj, "verify_token");
        mask_field(obj, "client_secret");
        mask_field(obj, "password");
        mask_field(obj, "private_key");
        mask_field(obj, "webhook_secret");
        mask_field(obj, "client_id");
    }
    list.push(serde_json::json!({ "name": name, "enabled": true, "config": obj }));
}

fn mask_optional_secret(value: &mut Option<String>) {
    if value.is_some() {
        *value = Some(MASKED_SECRET.to_string());
    }
}

fn mask_required_secret(value: &mut String) {
    if !value.is_empty() {
        *value = MASKED_SECRET.to_string();
    }
}

fn mask_vec_secrets(values: &mut [String]) {
    for value in values.iter_mut() {
        if !value.is_empty() {
            *value = MASKED_SECRET.to_string();
        }
    }
}

#[allow(clippy::ref_option)]
fn mask_sensitive_fields(config: &crate::config::Config) -> crate::config::Config {
    let mut masked = config.clone();

    mask_optional_secret(&mut masked.api_key);
    mask_vec_secrets(&mut masked.reliability.api_keys);
    mask_vec_secrets(&mut masked.gateway.paired_tokens);
    mask_optional_secret(&mut masked.composio.api_key);
    mask_optional_secret(&mut masked.web_search.brave_api_key);
    mask_optional_secret(&mut masked.storage.provider.config.db_url);
    mask_optional_secret(&mut masked.memory.qdrant.api_key);
    if let Some(cloudflare) = masked.tunnel.cloudflare.as_mut() {
        mask_required_secret(&mut cloudflare.token);
    }
    if let Some(ngrok) = masked.tunnel.ngrok.as_mut() {
        mask_required_secret(&mut ngrok.auth_token);
    }

    for agent in masked.agents.values_mut() {
        mask_optional_secret(&mut agent.api_key);
    }
    for route in &mut masked.model_routes {
        mask_optional_secret(&mut route.api_key);
    }
    for route in &mut masked.embedding_routes {
        mask_optional_secret(&mut route.api_key);
    }

    if let Some(telegram) = masked.channels_config.telegram.as_mut() {
        mask_required_secret(&mut telegram.bot_token);
    }
    if let Some(discord) = masked.channels_config.discord.as_mut() {
        mask_required_secret(&mut discord.bot_token);
    }
    if let Some(slack) = masked.channels_config.slack.as_mut() {
        mask_required_secret(&mut slack.bot_token);
        mask_optional_secret(&mut slack.app_token);
    }
    if let Some(mattermost) = masked.channels_config.mattermost.as_mut() {
        mask_required_secret(&mut mattermost.bot_token);
    }
    if let Some(webhook) = masked.channels_config.webhook.as_mut() {
        mask_optional_secret(&mut webhook.secret);
    }
    if let Some(matrix) = masked.channels_config.matrix.as_mut() {
        mask_required_secret(&mut matrix.access_token);
    }
    if let Some(whatsapp) = masked.channels_config.whatsapp.as_mut() {
        mask_optional_secret(&mut whatsapp.access_token);
        mask_optional_secret(&mut whatsapp.app_secret);
        mask_optional_secret(&mut whatsapp.verify_token);
    }
    if let Some(linq) = masked.channels_config.linq.as_mut() {
        mask_required_secret(&mut linq.api_token);
        mask_optional_secret(&mut linq.signing_secret);
    }
    if let Some(nextcloud) = masked.channels_config.nextcloud_talk.as_mut() {
        mask_required_secret(&mut nextcloud.app_token);
        mask_optional_secret(&mut nextcloud.webhook_secret);
    }
    if let Some(wati) = masked.channels_config.wati.as_mut() {
        mask_required_secret(&mut wati.api_token);
    }
    if let Some(irc) = masked.channels_config.irc.as_mut() {
        mask_optional_secret(&mut irc.server_password);
        mask_optional_secret(&mut irc.nickserv_password);
        mask_optional_secret(&mut irc.sasl_password);
    }
    if let Some(lark) = masked.channels_config.lark.as_mut() {
        mask_required_secret(&mut lark.app_secret);
        mask_optional_secret(&mut lark.encrypt_key);
    }
    if let Some(feishu) = masked.channels_config.feishu.as_mut() {
        mask_required_secret(&mut feishu.app_secret);
        mask_optional_secret(&mut feishu.encrypt_key);
        mask_optional_secret(&mut feishu.verification_token);
    }
    if let Some(dingtalk) = masked.channels_config.dingtalk.as_mut() {
        mask_required_secret(&mut dingtalk.client_secret);
    }
    if let Some(qq) = masked.channels_config.qq.as_mut() {
        mask_required_secret(&mut qq.app_secret);
    }
    #[cfg(feature = "channel-nostr")]
    if let Some(nostr) = masked.channels_config.nostr.as_mut() {
        mask_required_secret(&mut nostr.private_key);
    }
    if let Some(email) = masked.channels_config.email.as_mut() {
        mask_required_secret(&mut email.password);
    }
    mask_optional_secret(&mut masked.transcription.api_key);
    masked
}

#[allow(clippy::ref_option)]
fn restore_optional_secret(value: &mut Option<String>, current: &Option<String>) {
    if value.as_deref().is_some_and(is_masked_secret) {
        *value = current.clone();
    }
}

fn restore_required_secret(value: &mut String, current: &str) {
    if is_masked_secret(value) {
        *value = current.to_string();
    }
}

fn restore_vec_secrets(values: &mut [String], current: &[String]) {
    for (idx, value) in values.iter_mut().enumerate() {
        if is_masked_secret(value) {
            if let Some(existing) = current.get(idx) {
                *value = existing.clone();
            }
        }
    }
}

fn normalize_route_field(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn model_route_identity_matches(
    incoming: &crate::config::schema::ModelRouteConfig,
    current: &crate::config::schema::ModelRouteConfig,
) -> bool {
    normalize_route_field(&incoming.hint) == normalize_route_field(&current.hint)
        && normalize_route_field(&incoming.provider) == normalize_route_field(&current.provider)
        && normalize_route_field(&incoming.model) == normalize_route_field(&current.model)
}

fn model_route_provider_model_matches(
    incoming: &crate::config::schema::ModelRouteConfig,
    current: &crate::config::schema::ModelRouteConfig,
) -> bool {
    normalize_route_field(&incoming.provider) == normalize_route_field(&current.provider)
        && normalize_route_field(&incoming.model) == normalize_route_field(&current.model)
}

fn embedding_route_identity_matches(
    incoming: &crate::config::schema::EmbeddingRouteConfig,
    current: &crate::config::schema::EmbeddingRouteConfig,
) -> bool {
    normalize_route_field(&incoming.hint) == normalize_route_field(&current.hint)
        && normalize_route_field(&incoming.provider) == normalize_route_field(&current.provider)
        && normalize_route_field(&incoming.model) == normalize_route_field(&current.model)
}

fn embedding_route_provider_model_matches(
    incoming: &crate::config::schema::EmbeddingRouteConfig,
    current: &crate::config::schema::EmbeddingRouteConfig,
) -> bool {
    normalize_route_field(&incoming.provider) == normalize_route_field(&current.provider)
        && normalize_route_field(&incoming.model) == normalize_route_field(&current.model)
}

fn restore_model_route_api_keys(
    incoming: &mut [crate::config::schema::ModelRouteConfig],
    current: &[crate::config::schema::ModelRouteConfig],
) {
    let mut used_current = vec![false; current.len()];
    for incoming_route in incoming {
        if !incoming_route
            .api_key
            .as_deref()
            .is_some_and(is_masked_secret)
        {
            continue;
        }

        let exact_match_idx = current
            .iter()
            .enumerate()
            .find(|(idx, current_route)| {
                !used_current[*idx] && model_route_identity_matches(incoming_route, current_route)
            })
            .map(|(idx, _)| idx);

        let match_idx = exact_match_idx.or_else(|| {
            current
                .iter()
                .enumerate()
                .find(|(idx, current_route)| {
                    !used_current[*idx]
                        && model_route_provider_model_matches(incoming_route, current_route)
                })
                .map(|(idx, _)| idx)
        });

        if let Some(idx) = match_idx {
            used_current[idx] = true;
            incoming_route.api_key = current[idx].api_key.clone();
        } else {

            incoming_route.api_key = None;
        }
    }
}

fn restore_embedding_route_api_keys(
    incoming: &mut [crate::config::schema::EmbeddingRouteConfig],
    current: &[crate::config::schema::EmbeddingRouteConfig],
) {
    let mut used_current = vec![false; current.len()];
    for incoming_route in incoming {
        if !incoming_route
            .api_key
            .as_deref()
            .is_some_and(is_masked_secret)
        {
            continue;
        }

        let exact_match_idx = current
            .iter()
            .enumerate()
            .find(|(idx, current_route)| {
                !used_current[*idx]
                    && embedding_route_identity_matches(incoming_route, current_route)
            })
            .map(|(idx, _)| idx);

        let match_idx = exact_match_idx.or_else(|| {
            current
                .iter()
                .enumerate()
                .find(|(idx, current_route)| {
                    !used_current[*idx]
                        && embedding_route_provider_model_matches(incoming_route, current_route)
                })
                .map(|(idx, _)| idx)
        });

        if let Some(idx) = match_idx {
            used_current[idx] = true;
            incoming_route.api_key = current[idx].api_key.clone();
        } else {

            incoming_route.api_key = None;
        }
    }
}

fn restore_masked_sensitive_fields(
    incoming: &mut crate::config::Config,
    current: &crate::config::Config,
) {
    restore_optional_secret(&mut incoming.api_key, &current.api_key);
    restore_vec_secrets(
        &mut incoming.gateway.paired_tokens,
        &current.gateway.paired_tokens,
    );
    restore_vec_secrets(
        &mut incoming.reliability.api_keys,
        &current.reliability.api_keys,
    );
    restore_optional_secret(&mut incoming.composio.api_key, &current.composio.api_key);
    restore_optional_secret(
        &mut incoming.web_search.brave_api_key,
        &current.web_search.brave_api_key,
    );
    restore_optional_secret(
        &mut incoming.storage.provider.config.db_url,
        &current.storage.provider.config.db_url,
    );
    restore_optional_secret(
        &mut incoming.memory.qdrant.api_key,
        &current.memory.qdrant.api_key,
    );
    if let (Some(incoming_tunnel), Some(current_tunnel)) = (
        incoming.tunnel.cloudflare.as_mut(),
        current.tunnel.cloudflare.as_ref(),
    ) {
        restore_required_secret(&mut incoming_tunnel.token, &current_tunnel.token);
    }
    if let (Some(incoming_tunnel), Some(current_tunnel)) = (
        incoming.tunnel.ngrok.as_mut(),
        current.tunnel.ngrok.as_ref(),
    ) {
        restore_required_secret(&mut incoming_tunnel.auth_token, &current_tunnel.auth_token);
    }

    for (name, agent) in &mut incoming.agents {
        if let Some(current_agent) = current.agents.get(name) {
            restore_optional_secret(&mut agent.api_key, &current_agent.api_key);
        }
    }
    restore_model_route_api_keys(&mut incoming.model_routes, &current.model_routes);
    restore_embedding_route_api_keys(&mut incoming.embedding_routes, &current.embedding_routes);

    if let (Some(incoming_ch), Some(current_ch)) = (
        incoming.channels_config.telegram.as_mut(),
        current.channels_config.telegram.as_ref(),
    ) {
        restore_required_secret(&mut incoming_ch.bot_token, &current_ch.bot_token);
    }
    if let (Some(incoming_ch), Some(current_ch)) = (
        incoming.channels_config.discord.as_mut(),
        current.channels_config.discord.as_ref(),
    ) {
        restore_required_secret(&mut incoming_ch.bot_token, &current_ch.bot_token);
    }
    if let (Some(incoming_ch), Some(current_ch)) = (
        incoming.channels_config.slack.as_mut(),
        current.channels_config.slack.as_ref(),
    ) {
        restore_required_secret(&mut incoming_ch.bot_token, &current_ch.bot_token);
        restore_optional_secret(&mut incoming_ch.app_token, &current_ch.app_token);
    }
    if let (Some(incoming_ch), Some(current_ch)) = (
        incoming.channels_config.mattermost.as_mut(),
        current.channels_config.mattermost.as_ref(),
    ) {
        restore_required_secret(&mut incoming_ch.bot_token, &current_ch.bot_token);
    }
    if let (Some(incoming_ch), Some(current_ch)) = (
        incoming.channels_config.webhook.as_mut(),
        current.channels_config.webhook.as_ref(),
    ) {
        restore_optional_secret(&mut incoming_ch.secret, &current_ch.secret);
    }
    if let (Some(incoming_ch), Some(current_ch)) = (
        incoming.channels_config.matrix.as_mut(),
        current.channels_config.matrix.as_ref(),
    ) {
        restore_required_secret(&mut incoming_ch.access_token, &current_ch.access_token);
    }
    if let (Some(incoming_ch), Some(current_ch)) = (
        incoming.channels_config.whatsapp.as_mut(),
        current.channels_config.whatsapp.as_ref(),
    ) {
        restore_optional_secret(&mut incoming_ch.access_token, &current_ch.access_token);
        restore_optional_secret(&mut incoming_ch.app_secret, &current_ch.app_secret);
        restore_optional_secret(&mut incoming_ch.verify_token, &current_ch.verify_token);
    }
    if let (Some(incoming_ch), Some(current_ch)) = (
        incoming.channels_config.linq.as_mut(),
        current.channels_config.linq.as_ref(),
    ) {
        restore_required_secret(&mut incoming_ch.api_token, &current_ch.api_token);
        restore_optional_secret(&mut incoming_ch.signing_secret, &current_ch.signing_secret);
    }
    if let (Some(incoming_ch), Some(current_ch)) = (
        incoming.channels_config.nextcloud_talk.as_mut(),
        current.channels_config.nextcloud_talk.as_ref(),
    ) {
        restore_required_secret(&mut incoming_ch.app_token, &current_ch.app_token);
        restore_optional_secret(&mut incoming_ch.webhook_secret, &current_ch.webhook_secret);
    }
    if let (Some(incoming_ch), Some(current_ch)) = (
        incoming.channels_config.wati.as_mut(),
        current.channels_config.wati.as_ref(),
    ) {
        restore_required_secret(&mut incoming_ch.api_token, &current_ch.api_token);
    }
    if let (Some(incoming_ch), Some(current_ch)) = (
        incoming.channels_config.irc.as_mut(),
        current.channels_config.irc.as_ref(),
    ) {
        restore_optional_secret(
            &mut incoming_ch.server_password,
            &current_ch.server_password,
        );
        restore_optional_secret(
            &mut incoming_ch.nickserv_password,
            &current_ch.nickserv_password,
        );
        restore_optional_secret(&mut incoming_ch.sasl_password, &current_ch.sasl_password);
    }
    if let (Some(incoming_ch), Some(current_ch)) = (
        incoming.channels_config.lark.as_mut(),
        current.channels_config.lark.as_ref(),
    ) {
        restore_required_secret(&mut incoming_ch.app_secret, &current_ch.app_secret);
        restore_optional_secret(&mut incoming_ch.encrypt_key, &current_ch.encrypt_key);
        restore_optional_secret(
            &mut incoming_ch.verification_token,
            &current_ch.verification_token,
        );
    }
    if let (Some(incoming_ch), Some(current_ch)) = (
        incoming.channels_config.feishu.as_mut(),
        current.channels_config.feishu.as_ref(),
    ) {
        restore_required_secret(&mut incoming_ch.app_secret, &current_ch.app_secret);
        restore_optional_secret(&mut incoming_ch.encrypt_key, &current_ch.encrypt_key);
        restore_optional_secret(
            &mut incoming_ch.verification_token,
            &current_ch.verification_token,
        );
    }
    if let (Some(incoming_ch), Some(current_ch)) = (
        incoming.channels_config.dingtalk.as_mut(),
        current.channels_config.dingtalk.as_ref(),
    ) {
        restore_required_secret(&mut incoming_ch.client_secret, &current_ch.client_secret);
    }
    if let (Some(incoming_ch), Some(current_ch)) = (
        incoming.channels_config.qq.as_mut(),
        current.channels_config.qq.as_ref(),
    ) {
        restore_required_secret(&mut incoming_ch.app_secret, &current_ch.app_secret);
    }
    #[cfg(feature = "channel-nostr")]
    if let (Some(incoming_ch), Some(current_ch)) = (
        incoming.channels_config.nostr.as_mut(),
        current.channels_config.nostr.as_ref(),
    ) {
        restore_required_secret(&mut incoming_ch.private_key, &current_ch.private_key);
    }
    if let (Some(incoming_ch), Some(current_ch)) = (
        incoming.channels_config.email.as_mut(),
        current.channels_config.email.as_ref(),
    ) {
        restore_required_secret(&mut incoming_ch.password, &current_ch.password);
    }
    restore_optional_secret(
        &mut incoming.transcription.api_key,
        &current.transcription.api_key,
    );
}

fn hydrate_config_for_save(
    mut incoming: crate::config::Config,
    current: &crate::config::Config,
) -> crate::config::Config {
    restore_masked_sensitive_fields(&mut incoming, current);

    incoming.config_path = current.config_path.clone();
    incoming.workspace_dir = current.workspace_dir.clone();
    incoming
}

const GW_SESSION_PREFIX: &str = "gw_";

fn strip_gw_prefix(key: &str) -> Option<&str> {
    key.strip_prefix(GW_SESSION_PREFIX)
}

fn default_workspace_dir(state: &AppState) -> String {
    let raw = state
        .config
        .lock()
        .workspace_dir
        .to_string_lossy()
        .to_string();
    display_path(&raw)
}

pub(in crate::gateway) fn display_path(path: &str) -> String {
    #[cfg(windows)]
    {
        if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{rest}");
        }
        if let Some(rest) = path.strip_prefix(r"\\?\") {
            return rest.to_string();
        }
    }
    path.to_string()
}

fn map_chat_role_to_desktop(role: &str) -> &'static str {
    match role {
        "user" => "user",
        "assistant" => "assistant",
        "tool" => "tool_result",
        _ => "system",
    }
}

fn gateway_desktop_message_content(msg_ty: &'static str, raw: &str) -> serde_json::Value {
    if !(matches!(msg_ty, "assistant" | "tool_result")) {
        return serde_json::Value::String(raw.to_string());
    }
    let trimmed = raw.trim();
    match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(v) => match &v {
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => v.clone(),
            _ => serde_json::Value::String(raw.to_string()),
        },
        Err(_) => serde_json::Value::String(raw.to_string()),
    }
}

const MAX_ATTACHMENT_INLINE_BYTES: u64 = 20 * 1024 * 1024;

fn attachment_file_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| path.to_string())
}

fn image_mime_from_path(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".bmp") {
        "image/bmp"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else {
        "image/png"
    }
}

fn read_image_as_data_url(path: &str) -> Option<String> {
    use base64::Engine;
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_ATTACHMENT_INLINE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let mime = image_mime_from_path(path);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Some(format!("data:{mime};base64,{encoded}"))
}

fn parse_persisted_attachments(content: &str) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("[IMAGE:") {
            let Some(path) = rest.strip_suffix(']') else {
                continue;
            };
            let path = path.trim();
            if path.is_empty() {
                continue;
            }
            let mut attachment = serde_json::json!({
                "type": "image",
                "name": attachment_file_name(path),
                "path": path,
            });
            if let Some(data_url) = read_image_as_data_url(path) {
                if let Some(obj) = attachment.as_object_mut() {
                    obj.insert(
                        "mimeType".to_string(),
                        serde_json::Value::String(image_mime_from_path(path).to_string()),
                    );
                    obj.insert("data".to_string(), serde_json::Value::String(data_url));
                }
            }
            out.push(attachment);
        } else if let Some(rest) = trimmed.strip_prefix("[Attached file:") {
            let Some(path) = rest.strip_suffix(']') else {
                continue;
            };
            let path = path.trim();
            if path.is_empty() {
                continue;
            }
            out.push(serde_json::json!({
                "type": "file",
                "name": attachment_file_name(path),
                "path": path,
            }));
        }
    }
    out
}

fn message_entry(
    session_id: &str,
    index: usize,
    msg: &crate::providers::ChatMessage,
    fallback_ts: &str,
) -> serde_json::Value {
    let entry_id = format!("{session_id}-{index:04}");
    let ty = map_chat_role_to_desktop(&msg.role);
    let content = gateway_desktop_message_content(ty, &msg.content);
    let mut entry = serde_json::json!({
        "id": entry_id,
        "type": ty,
        "content": content,
        "timestamp": fallback_ts,
    });
    if ty == "user" {
        let attachments = parse_persisted_attachments(&msg.content);
        if !attachments.is_empty() {
            if let Some(obj) = entry.as_object_mut() {
                obj.insert(
                    "attachments".to_string(),
                    serde_json::Value::Array(attachments),
                );
            }
        }
        if let Some(display) = msg
            .metadata
            .get("display_content")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
        {
            if let Some(obj) = entry.as_object_mut() {
                obj.insert(
                    "displayContent".to_string(),
                    serde_json::Value::String(display.to_string()),
                );
            }
        }
    }
    if let Some(design_ref) = msg
        .metadata
        .get("design_ref")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        if let Some(obj) = entry.as_object_mut() {
            obj.insert(
                "designRef".to_string(),
                serde_json::Value::String(design_ref.to_string()),
            );
            if let Some(name) = msg
                .metadata
                .get("design_ref_name")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                obj.insert(
                    "designRefName".to_string(),
                    serde_json::Value::String(name.to_string()),
                );
            }
            if let Some(element) = msg
                .metadata
                .get("design_ref_element")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                obj.insert(
                    "designRefElement".to_string(),
                    serde_json::Value::String(element.to_string()),
                );
                if let Some(label) = msg
                    .metadata
                    .get("design_ref_element_label")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                {
                    obj.insert(
                        "designRefElementLabel".to_string(),
                        serde_json::Value::String(label.to_string()),
                    );
                }
            }
        }
    }
    entry
}

fn message_entry_with_tombstone(
    session_id: &str,
    index: usize,
    msg: &crate::providers::ChatMessage,
    fallback_ts: &str,
    tombstoned: bool,
    user_message_index: Option<usize>,
) -> serde_json::Value {
    let mut v = message_entry(session_id, index, msg, fallback_ts);
    if let Some(obj) = v.as_object_mut() {
        if tombstoned {
            obj.insert("tombstoned".to_string(), serde_json::Value::Bool(true));
        }
        if let Some(umi) = user_message_index {
            obj.insert(
                "userMessageIndex".to_string(),
                serde_json::Value::from(umi as u64),
            );
        }
    }
    v
}

pub async fn handle_api_sessions_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let Some(ref backend) = state.session_backend else {
        return Json(serde_json::json!({
            "sessions": [],
            "total": 0,
        }))
        .into_response();
    };

    let all_metadata = {
        let backend_arc = std::sync::Arc::clone(backend);
        tokio::task::spawn_blocking(move || backend_arc.list_sessions_with_metadata())
            .await
            .unwrap_or_default()
    };
    let default_wd = default_workspace_dir(&state);
    let running_set: std::collections::HashSet<String> = state
        .session_run_state
        .snapshot()
        .into_iter()
        .collect();

    let sessions: Vec<serde_json::Value> = tokio::task::spawn_blocking(move || {
        all_metadata
            .into_iter()
            .filter_map(|meta| {
                let session_id = strip_gw_prefix(&meta.key)?.to_string();
                let title = meta.name.clone().unwrap_or_default();
                let work_dir = meta
                    .work_dir
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| default_wd.clone());
                let work_dir_exists = std::path::Path::new(&work_dir).exists();
                let running = running_set.contains(&session_id);
                Some(serde_json::json!({
                    "id": session_id,
                    "title": title,
                    "createdAt": meta.created_at.to_rfc3339(),
                    "modifiedAt": meta.last_activity.to_rfc3339(),
                    "messageCount": meta.message_count,
                    "projectPath": work_dir,
                    "workDir": work_dir,
                    "workDirExists": work_dir_exists,
                    "running": running,
                }))
            })
            .collect()
    })
    .await
    .unwrap_or_default();

    let total = sessions.len();
    Json(serde_json::json!({ "sessions": sessions, "total": total })).into_response()
}

pub async fn handle_api_session_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<serde_json::Value>>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let session_id = uuid::Uuid::new_v4().to_string();
    let session_key = format!("{GW_SESSION_PREFIX}{session_id}");

    let explicit_title = body
        .as_ref()
        .and_then(|Json(v)| v.get("title").and_then(|t| t.as_str()))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());

    if let Some(ref backend) = state.session_backend {
        let backend_arc = std::sync::Arc::clone(backend);
        let session_key_owned = session_key.clone();
        let title_opt = explicit_title.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let bootstrap = crate::providers::ChatMessage {
                role: "system".to_string(),
                content: String::new(),
                metadata: Default::default(),
            };
            if let Err(e) = backend_arc.append(&session_key_owned, &bootstrap) {
                tracing::warn!("create_session: bootstrap append failed: {e}");
            }
            if let Some(title) = title_opt {
                if let Err(e) = backend_arc.set_session_name(&session_key_owned, &title) {
                    tracing::warn!("create_session: set name failed: {e}");
                }
            }
        })
        .await;
    }

    let work_dir = body
        .as_ref()
        .and_then(|Json(v)| v.get("workDir").and_then(|t| t.as_str()))
        .map(|s| s.to_string())
        .unwrap_or_else(|| default_workspace_dir(&state));

    if let Some(ref backend) = state.session_backend {
        let backend_arc = std::sync::Arc::clone(backend);
        let session_key_owned = session_key.clone();
        let work_dir_owned = work_dir.clone();
        let _ = tokio::task::spawn_blocking(move || {
            if let Err(e) = backend_arc.set_session_work_dir(&session_key_owned, &work_dir_owned) {
                tracing::warn!("create_session: set work_dir failed: {e}");
            }
        })
        .await;
    }

    Json(serde_json::json!({
        "sessionId": session_id,
        "workDir": work_dir,
    }))
    .into_response()
}

pub async fn handle_api_session_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let Some(ref backend) = state.session_backend else {
        return Json(serde_json::json!({ "messages": [] })).into_response();
    };

    let limit_param: Option<usize> = params.get("limit").and_then(|v| v.parse().ok());
    let before_param: Option<usize> = params.get("before").and_then(|v| v.parse().ok());

    let session_key = format!("{GW_SESSION_PREFIX}{id}");
    let (loaded, first_index, total_rows, last_activity) = {
        let backend_arc = std::sync::Arc::clone(backend);
        let session_key_owned = session_key.clone();
        tokio::task::spawn_blocking(move || {
            let last_activity = backend_arc
                .get_session_metadata(&session_key_owned)
                .map(|m| m.last_activity.to_rfc3339())
                .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
            match limit_param {
                Some(limit) => {
                    let total = backend_arc.count_messages(&session_key_owned);
                    let end = before_param.unwrap_or(total).min(total);
                    let start = end.saturating_sub(limit.max(1));
                    let loaded = backend_arc.load_with_tombstones_range(
                        &session_key_owned,
                        start,
                        end - start,
                    );
                    (loaded, start, total, last_activity)
                }
                None => {
                    let loaded = backend_arc.load_with_tombstones(&session_key_owned);
                    let total = loaded.len();
                    (loaded, 0, total, last_activity)
                }
            }
        })
        .await
        .unwrap_or_else(|_| (Vec::new(), 0, 0, chrono::Utc::now().to_rfc3339()))
    };

    let base_user_index: usize = match loaded.first().map(|m| m.id) {
        Some(first_row_id) => {
            let backend_arc = std::sync::Arc::clone(backend);
            let session_key_owned = session_key.clone();
            tokio::task::spawn_blocking(move || {
                backend_arc.count_live_user_messages_before_id(&session_key_owned, first_row_id)
            })
            .await
            .unwrap_or(0)
        }
        None => 0,
    };

    let messages: Vec<serde_json::Value> = {
        let id_for_map = id.clone();
        let last_activity_for_map = last_activity.clone();
        tokio::task::spawn_blocking(move || {
            let mut running_user_index = base_user_index;
            let mut out: Vec<serde_json::Value> = Vec::new();
            for (i, lm) in loaded
                .iter()
                .enumerate()
                .map(|(i, lm)| (first_index + i, lm))
                .filter(|(_, lm)| !lm.hidden_for_ui)
                .filter(|(_, lm)| !(lm.message.role == "system" && lm.message.content.is_empty()))
            {
                let is_live_user = lm.message.role == "user" && lm.tombstoned_at.is_none();
                let user_message_index = is_live_user.then_some(running_user_index);
                out.push(message_entry_with_tombstone(
                    &id_for_map,
                    i,
                    &lm.message,
                    &last_activity_for_map,
                    lm.tombstoned_at.is_some(),
                    user_message_index,
                ));
                if is_live_user {
                    running_user_index += 1;
                }
            }
            out
        })
        .await
        .unwrap_or_default()
    };

    let pending_rewind = {
        let backend_arc = std::sync::Arc::clone(backend);
        let state_cl = state.clone();
        let session_key_owned = session_key.clone();
        tokio::task::spawn_blocking(move || {
            backend_arc
                .latest_rewind_stash_for_session(&session_key_owned)
                .map(|stash| {
                    let entries: Vec<RewindStashEntry> =
                        serde_json::from_str(&stash.stash_json).unwrap_or_default();
                    let files_changed: Vec<String> =
                        entries.iter().map(|e| e.rel_path.clone()).collect();
                    let workspace = resolve_session_workspace(&state_cl, &session_key_owned);
                    let history = crate::tools::edit_history::EditHistory::new(workspace.clone());
                    let batches: Vec<String> = entries
                        .iter()
                        .flat_map(|e| e.edit_batch_ids.clone())
                        .collect::<std::collections::BTreeSet<_>>()
                        .into_iter()
                        .collect();
                    let (_files, insertions, deletions) =
                        summarise_batches(&workspace, &history, &batches);
                    serde_json::json!({
                        "rewindId": stash.rewind_id,
                        "userMessageIndex": stash.user_message_index,
                        "filesChanged": files_changed,
                        "insertions": insertions,
                        "deletions": deletions,
                    })
                })
        })
        .await
        .ok()
        .flatten()
    };

    let mut body = serde_json::json!({
        "messages": messages,
        "totalMessages": total_rows,
        "firstIndex": first_index,
        "hasMore": first_index > 0,
    });
    if let Some(pr) = pending_rewind {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("pendingRewind".to_string(), pr);
        } else {
            tracing::warn!("session messages body is not a JSON object; skipping pendingRewind");
        }
    }
    Json(body).into_response()
}

pub async fn handle_api_session_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let Some(ref backend) = state.session_backend else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Session persistence is disabled"})),
        )
            .into_response();
    };

    let session_key = format!("{GW_SESSION_PREFIX}{id}");
    let result = {
        let backend_arc = std::sync::Arc::clone(backend);
        let session_key_owned = session_key.clone();
        tokio::task::spawn_blocking(move || backend_arc.delete_session(&session_key_owned))
            .await
            .unwrap_or_else(|e| Err(std::io::Error::other(e.to_string())))
    };
    match result {
        Ok(true) => Json(serde_json::json!({ "ok": true })).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Session not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to delete session: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to delete session"})),
            )
                .into_response()
        }
    }
}

pub async fn handle_api_sessions_delete_batch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SessionDeleteBatchBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let Some(ref backend) = state.session_backend else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Session persistence is disabled"})),
        )
            .into_response();
    };

    let session_keys: Vec<String> = body
        .ids
        .iter()
        .filter_map(|id| {
            let trimmed = id.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(format!("{GW_SESSION_PREFIX}{trimmed}"))
            }
        })
        .collect();

    if session_keys.is_empty() {
        return Json(serde_json::json!({ "ok": true, "deleted": 0 })).into_response();
    }

    let result = {
        let backend_arc = std::sync::Arc::clone(backend);
        tokio::task::spawn_blocking(move || backend_arc.delete_sessions(&session_keys))
            .await
            .unwrap_or_else(|e| Err(std::io::Error::other(e.to_string())))
    };

    match result {
        Ok(deleted) => Json(serde_json::json!({ "ok": true, "deleted": deleted })).into_response(),
        Err(e) => {
            tracing::error!("Failed to batch-delete sessions: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to delete sessions"})),
            )
                .into_response()
        }
    }
}

pub async fn handle_api_session_rename(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let Some(ref backend) = state.session_backend else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Session persistence is disabled"})),
        )
            .into_response();
    };

    let title = body
        .get("title")
        .and_then(|v| v.as_str())
        .or_else(|| body.get("name").and_then(|v| v.as_str()))
        .unwrap_or("")
        .trim()
        .to_string();

    if title.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "title is required"})),
        )
            .into_response();
    }

    let session_key = format!("{GW_SESSION_PREFIX}{id}");
    let result = {
        let backend_arc = std::sync::Arc::clone(backend);
        let session_key_owned = session_key.clone();
        let title_owned = title.clone();
        tokio::task::spawn_blocking(move || {
            let sessions = backend_arc.list_sessions();
            if !sessions.contains(&session_key_owned) {
                return Ok(false);
            }
            backend_arc
                .set_session_name(&session_key_owned, &title_owned)
                .map(|_| true)
        })
        .await
        .unwrap_or_else(|e| Err(std::io::Error::other(e.to_string())))
    };
    match result {
        Ok(true) => Json(serde_json::json!({ "ok": true })).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Session not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to rename session: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to rename session"})),
            )
                .into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SessionsRecentProjectsQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

struct RecentProjectAgg {
    real_path: String,
    last_activity: chrono::DateTime<chrono::Utc>,
    session_count: usize,
}

fn build_recent_projects(
    metas: Vec<crate::channels::session::backend::SessionMetadata>,
    default_dir: &str,
    offset: usize,
    limit: usize,
) -> (Vec<serde_json::Value>, usize) {
    use std::collections::HashMap;

    let canonical = |dir: &str| -> String {
        std::path::Path::new(dir)
            .canonicalize()
            .map(|p| display_path(&p.to_string_lossy()))
            .unwrap_or_else(|_| dir.to_string())
    };

    let mut map: HashMap<String, RecentProjectAgg> = HashMap::new();
    for meta in metas {
        let Some(dir) = meta.work_dir.as_deref().map(str::trim).filter(|d| !d.is_empty())
        else {
            continue;
        };
        let real = canonical(dir);
        let entry = map.entry(real.clone()).or_insert_with(|| RecentProjectAgg {
            real_path: real.clone(),
            last_activity: meta.last_activity,
            session_count: 0,
        });
        entry.session_count += 1;
        if meta.last_activity > entry.last_activity {
            entry.last_activity = meta.last_activity;
        }
    }

    let mut entries: Vec<RecentProjectAgg> = map.into_values().collect();
    entries.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));

    let default_real = canonical(default_dir);
    if let Some(pos) = entries.iter().position(|e| e.real_path == default_real) {
        let pinned = entries.remove(pos);
        entries.insert(0, pinned);
    } else if !default_real.is_empty() {
        entries.insert(
            0,
            RecentProjectAgg {
                real_path: default_real,
                last_activity: chrono::Utc::now(),
                session_count: 0,
            },
        );
    }

    let total = entries.len();
    let projects: Vec<serde_json::Value> = entries
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|entry| {
            let path = std::path::Path::new(&entry.real_path);
            let project_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "workspace".to_string());
            let (is_git, repo_name, branch) = git_repo_info(path);
            serde_json::json!({
                "projectPath": entry.real_path,
                "realPath": entry.real_path,
                "projectName": project_name,
                "isGit": is_git,
                "repoName": repo_name,
                "branch": branch,
                "modifiedAt": entry.last_activity.to_rfc3339(),
                "sessionCount": entry.session_count,
            })
        })
        .collect();

    (projects, total)
}

pub async fn handle_api_sessions_recent_projects(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<SessionsRecentProjectsQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let default_dir = default_workspace_dir(&state);
    let limit = q.limit.unwrap_or(10).clamp(1, 500);
    let offset = q.offset.unwrap_or(0);

    let metas = if let Some(backend) = state.session_backend.as_ref() {
        let backend_arc = std::sync::Arc::clone(backend);
        tokio::task::spawn_blocking(move || {
            backend_arc
                .list_sessions_with_metadata()
                .into_iter()
                .filter(|m| m.key.starts_with(GW_SESSION_PREFIX))
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default()
    } else {
        Vec::new()
    };

    let (projects, total) =
        tokio::task::spawn_blocking(move || build_recent_projects(metas, &default_dir, offset, limit))
            .await
            .unwrap_or_else(|_| (Vec::new(), 0));

    Json(serde_json::json!({
        "projects": projects,
        "total": total,
    }))
    .into_response()
}

pub async fn handle_api_session_git_info(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let session_key = format!("{GW_SESSION_PREFIX}{id}");
    let work_dir = if let Some(ref backend) = state.session_backend {
        let backend_arc = std::sync::Arc::clone(backend);
        let session_key_owned = session_key.clone();
        let result = tokio::task::spawn_blocking(move || {
            backend_arc.get_session_work_dir(&session_key_owned)
        })
        .await
        .unwrap_or(Ok(None));
        match result {
            Ok(Some(dir)) => dir,
            Ok(None) | Err(_) => default_workspace_dir(&state),
        }
    } else {
        default_workspace_dir(&state)
    };
    let (repo_name, branch, changed_files) = {
        let path_buf = std::path::PathBuf::from(&work_dir);
        tokio::task::spawn_blocking(move || {
            let path = path_buf.as_path();
            let (_is_git, repo_name, branch) = git_repo_info(path);
            let changed_files = git_changed_file_count(path);
            (repo_name, branch, changed_files)
        })
        .await
        .unwrap_or((None, None, 0))
    };

    Json(serde_json::json!({
        "branch": branch,
        "repoName": repo_name,
        "workDir": work_dir,
        "changedFiles": changed_files,
    }))
    .into_response()
}

pub async fn handle_api_session_slash_commands(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(_id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let registry = crate::commands::registry::CommandRegistry::from_inventory();
    let commands: Vec<serde_json::Value> = registry
        .list(None)
        .into_iter()
        .map(|c| {
            serde_json::json!({
                "name": c.name,
                "description": c.description,
            })
        })
        .collect();

    Json(serde_json::json!({ "commands": commands })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct SessionRewindBody {
    #[serde(rename = "userMessageIndex")]
    pub user_message_index: i64,
    #[serde(default, rename = "dryRun")]
    pub dry_run: bool,

    #[serde(default, rename = "revertFiles")]
    pub revert_files: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct SessionRewindIdBody {
    #[serde(rename = "rewindId")]
    pub rewind_id: String,
}

#[derive(Debug, Deserialize)]
pub struct SessionRevertBatchesBody {
    #[serde(rename = "editBatchIds")]
    pub edit_batch_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct SessionDeleteBatchBody {
    pub ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RewindStashEntry {
    rel_path: String,
    post_sha256: String,
    edit_batch_ids: Vec<String>,
}

fn resolve_session_workspace(state: &AppState, session_key: &str) -> std::path::PathBuf {
    let dir = state
        .session_backend
        .as_ref()
        .and_then(|b| b.get_session_work_dir(session_key).ok().flatten())
        .unwrap_or_else(|| default_workspace_dir(state));
    std::path::PathBuf::from(dir)
}

fn summarise_batches(
    workspace: &std::path::Path,
    history: &crate::tools::edit_history::EditHistory,
    batch_ids: &[String],
) -> (Vec<String>, u64, u64) {
    use std::collections::BTreeMap;

    let mut earliest_per_path: BTreeMap<String, crate::tools::edit_history::FileSnapshot> =
        BTreeMap::new();
    for batch_id in batch_ids {
        for (rel_path, snap) in history.snapshots_for_batch(batch_id) {
            earliest_per_path.entry(rel_path).or_insert(snap);
        }
    }
    let mut insertions: u64 = 0;
    let mut deletions: u64 = 0;
    for (rel_path, snap) in &earliest_per_path {
        let abs = workspace.join(rel_path);
        let pre = history
            .read_blob(&snap.sha256)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default();
        let post = std::fs::read_to_string(&abs).unwrap_or_default();
        let pre_lines: u64 = pre.lines().count() as u64;
        let post_lines: u64 = post.lines().count() as u64;

        if post_lines >= pre_lines {
            insertions += post_lines - pre_lines;
        } else {
            deletions += pre_lines - post_lines;
        }
    }
    let files_changed: Vec<String> = earliest_per_path.into_keys().collect();
    (files_changed, insertions, deletions)
}

pub async fn handle_api_session_rewind(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<SessionRewindBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let Some(ref backend) = state.session_backend else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Session persistence is disabled"})),
        )
            .into_response();
    };

    let session_key = format!("{GW_SESSION_PREFIX}{id}");
    let loaded = {
        let backend_arc = std::sync::Arc::clone(backend);
        let session_key_owned = session_key.clone();
        tokio::task::spawn_blocking(move || backend_arc.load_with_tombstones(&session_key_owned))
            .await
            .unwrap_or_default()
    };
    if loaded.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Session has no messages"})),
        )
            .into_response();
    }

    let live: Vec<(usize, &crate::channels::session::backend::LoadedMessage)> = loaded
        .iter()
        .enumerate()
        .filter(|(_, m)| m.tombstoned_at.is_none() && !m.hidden_for_ui)
        .collect();

    let user_positions: Vec<usize> = live
        .iter()
        .filter_map(|(i, m)| (m.message.role == "user").then_some(*i))
        .collect();
    let user_count = user_positions.len();

    if body.user_message_index < 0 || (body.user_message_index as usize) >= user_count {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "userMessageIndex out of range"})),
        )
            .into_response();
    }

    let target_user_idx = body.user_message_index;
    let target_loaded_idx = user_positions[target_user_idx as usize];
    let target_row = &loaded[target_loaded_idx];
    let first_db_id = target_row.id;

    let live_tail_len = live.len() - live.iter().position(|(i, _)| *i == target_loaded_idx).unwrap_or(0);

    let (batches_after, workspace, files_changed, insertions, deletions) = {
        let backend_arc = std::sync::Arc::clone(backend);
        let state_cl = state.clone();
        let sk = session_key.clone();
        tokio::task::spawn_blocking(move || {
            let batches_after = backend_arc.edit_batches_after(&sk, target_user_idx);
            let workspace = resolve_session_workspace(&state_cl, &sk);
            let history = crate::tools::edit_history::EditHistory::new(workspace.clone());
            let (files_changed, insertions, deletions) =
                summarise_batches(&workspace, &history, &batches_after);
            (batches_after, workspace, files_changed, insertions, deletions)
        })
        .await
        .unwrap_or_else(|_| {
            (
                Vec::new(),
                std::path::PathBuf::from(default_workspace_dir(&state)),
                Vec::new(),
                0,
                0,
            )
        })
    };
    let any_batches = !batches_after.is_empty();

    let removed_message_ids: Vec<String> = (target_loaded_idx..loaded.len())
        .map(|i| format!("{id}-{i:04}"))
        .collect();

    if body.dry_run {

        return Json(serde_json::json!({
            "target": {
                "userMessageIndex": target_user_idx,
                "userMessageCount": user_count,
            },
            "conversation": {
                "messagesRemoved": live_tail_len,
                "tombstonedCount": 0,
                "removedMessageIds": removed_message_ids,
            },
            "code": {
                "available": any_batches,
                "reason": if any_batches { "edit_batches_after" } else { "no_edits_to_revert" },
                "filesChanged": files_changed,
                "insertions": insertions,
                "deletions": deletions,
            },
        }))
        .into_response();
    }

    let want_revert = body.revert_files.unwrap_or(true);

    let (rewind_id, tombstoned) = {
        let backend_arc = std::sync::Arc::clone(backend);
        let sk = session_key.clone();
        let workspace = workspace.clone();
        let batches_after = batches_after.clone();
        tokio::task::spawn_blocking(move || {
            let history = crate::tools::edit_history::EditHistory::new(workspace.clone());
            let rewind_id = format!("rw-{}", uuid::Uuid::new_v4().simple());

            let mut inherited_blobs: std::collections::BTreeMap<String, String> =
                std::collections::BTreeMap::new();
            while let Some(prev) = backend_arc.latest_rewind_stash_for_session(&sk) {
                if let Ok(entries) =
                    serde_json::from_str::<Vec<RewindStashEntry>>(&prev.stash_json)
                {
                    for entry in entries {
                        inherited_blobs
                            .entry(entry.rel_path)
                            .or_insert(entry.post_sha256);
                    }
                }

                let _ = backend_arc.take_rewind_stash(&prev.rewind_id);
            }

            let stash_batch_id = format!("rewind-stash-{rewind_id}");
            if want_revert {
                let mut affected_paths: std::collections::BTreeSet<String> =
                    std::collections::BTreeSet::new();
                for batch_id in &batches_after {
                    for (rel_path, _snap) in history.snapshots_for_batch(batch_id) {
                        affected_paths.insert(rel_path);
                    }
                }
                for rel_path in &affected_paths {
                    if inherited_blobs.contains_key(rel_path) {
                        continue;
                    }
                    let abs = workspace.join(rel_path);
                    if abs.exists() {
                        if let Err(e) = history.snapshot_before_write_with_batch(
                            &abs,
                            "rewind-stash",
                            &format!("rewind {rewind_id}"),
                            Some(stash_batch_id.clone()),
                        ) {
                            tracing::warn!(target: "rewind", "stash snapshot failed for {rel_path}: {e}");
                        }
                    }
                }
            }

            let mut entries_by_path: std::collections::BTreeMap<String, String> =
                inherited_blobs.clone();
            if want_revert {
                for (rel_path, snap) in history.snapshots_for_batch(&stash_batch_id) {
                    entries_by_path.entry(rel_path).or_insert(snap.sha256);
                }
            }
            let stash_entries: Vec<RewindStashEntry> = entries_by_path
                .into_iter()
                .map(|(rel_path, post_sha256)| RewindStashEntry {
                    rel_path,
                    post_sha256,
                    edit_batch_ids: batches_after.clone(),
                })
                .collect();
            let stash_json =
                serde_json::to_string(&stash_entries).unwrap_or_else(|_| "[]".to_string());
            if let Err(e) =
                backend_arc.save_rewind_stash(&rewind_id, &sk, target_user_idx, &stash_json)
            {
                tracing::warn!(target: "rewind", "save_rewind_stash failed: {e}");
            }

            if want_revert {
                for batch_id in batches_after.iter().rev() {
                    match history.revert_batch(batch_id) {
                        Ok(reverted) => {
                            tracing::info!(
                                target: "rewind",
                                "reverted batch {batch_id}: {} files",
                                reverted.len()
                            );
                        }
                        Err(e) => {
                            tracing::warn!(target: "rewind", "revert_batch({batch_id}) failed: {e}");
                        }
                    }
                }
            }

            let tombstoned = match backend_arc.tombstone_from(&sk, first_db_id) {
                Ok(n) => n,
                Err(e) => {
                    tracing::warn!(target: "rewind", "tombstone_from failed: {e}");
                    0
                }
            };
            (rewind_id, tombstoned)
        })
        .await
        .unwrap_or_else(|_| (String::new(), 0))
    };

    Json(serde_json::json!({
        "rewindId": rewind_id,
        "target": {
            "userMessageIndex": target_user_idx,
            "userMessageCount": user_count,
        },
        "conversation": {
            "messagesRemoved": live_tail_len,
            "tombstonedCount": tombstoned,
            "removedMessageIds": removed_message_ids,
        },
        "code": {
            "available": any_batches && want_revert,
            "reason": if !any_batches {
                "no_edits_to_revert"
            } else if want_revert {
                "reverted"
            } else {
                "kept_intentionally"
            },
            "filesChanged": files_changed,
            "insertions": insertions,
            "deletions": deletions,
        },
    }))
    .into_response()
}

pub async fn handle_api_session_rewind_restore(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<SessionRewindIdBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let Some(ref backend) = state.session_backend else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Session persistence is disabled"})),
        )
            .into_response();
    };

    let session_key = format!("{GW_SESSION_PREFIX}{id}");

    let stash = {
        let backend_arc = std::sync::Arc::clone(backend);
        let rewind_id_owned = body.rewind_id.clone();
        tokio::task::spawn_blocking(move || backend_arc.take_rewind_stash(&rewind_id_owned))
            .await
            .ok()
            .flatten()
    };
    let Some(stash) = stash else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Rewind stash not found"})),
        )
            .into_response();
    };
    if stash.session_key != session_key {

        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Rewind stash does not belong to this session"})),
        )
            .into_response();
    }

    let entries: Vec<RewindStashEntry> =
        serde_json::from_str(&stash.stash_json).unwrap_or_default();

    let (restored, cleared) = {
        let backend_arc = std::sync::Arc::clone(backend);
        let state_cl = state.clone();
        let sk = session_key.clone();
        tokio::task::spawn_blocking(move || {
            let workspace = resolve_session_workspace(&state_cl, &sk);
            let history = crate::tools::edit_history::EditHistory::new(workspace.clone());

            let mut restored: Vec<String> = Vec::new();
            for entry in &entries {
                match history.read_blob(&entry.post_sha256) {
                    Ok(bytes) => {
                        let abs = workspace.join(&entry.rel_path);
                        if let Some(parent) = abs.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        if let Err(e) = std::fs::write(&abs, &bytes) {
                            tracing::warn!(
                                target: "rewind",
                                "restore: write {} failed: {e}",
                                entry.rel_path
                            );
                        } else {
                            restored.push(entry.rel_path.clone());
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "rewind",
                            "restore: blob {} missing: {e}",
                            entry.post_sha256
                        );
                    }
                }
            }

            let cleared = match backend_arc.clear_tombstones(&sk) {
                Ok(n) => n,
                Err(e) => {
                    tracing::warn!(target: "rewind", "clear_tombstones failed: {e}");
                    0
                }
            };
            (restored, cleared)
        })
        .await
        .unwrap_or_else(|_| (Vec::new(), 0))
    };

    Json(serde_json::json!({
        "ok": true,
        "rewindId": body.rewind_id,
        "restoredCount": restored.len(),
        "clearedTombstones": cleared,
        "filesChanged": restored,
    }))
    .into_response()
}

pub async fn handle_api_session_rewind_commit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<SessionRewindIdBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let Some(ref backend) = state.session_backend else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Session persistence is disabled"})),
        )
            .into_response();
    };

    let session_key = format!("{GW_SESSION_PREFIX}{id}");

    let purged = {
        let backend_arc = std::sync::Arc::clone(backend);
        let sk = session_key.clone();
        let rewind_id_owned = body.rewind_id.clone();
        tokio::task::spawn_blocking(move || {
            let stash = backend_arc.take_rewind_stash(&rewind_id_owned);
            let user_message_index = stash.as_ref().map(|s| s.user_message_index).unwrap_or(0);

            let purged = match backend_arc.purge_tombstoned(&sk) {
                Ok(n) => n,
                Err(e) => {
                    tracing::warn!(target: "rewind", "purge_tombstoned failed: {e}");
                    0
                }
            };
            if stash.is_some() {
                let _ = backend_arc.drop_edit_batches_after(&sk, user_message_index);
            }
            purged
        })
        .await
        .unwrap_or(0)
    };

    Json(serde_json::json!({
        "ok": true,
        "rewindId": body.rewind_id,
        "purgedCount": purged,
    }))
    .into_response()
}

pub async fn handle_api_session_revert_batches(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<SessionRevertBatchesBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let session_key = format!("{GW_SESSION_PREFIX}{id}");
    let (reverted_paths, failed_batch_ids) = {
        let state_cl = state.clone();
        let sk = session_key.clone();
        let batch_ids = body.edit_batch_ids.clone();
        tokio::task::spawn_blocking(move || {
            let workspace = resolve_session_workspace(&state_cl, &sk);
            let history = crate::tools::edit_history::EditHistory::new(workspace.clone());

            let mut reverted_paths: Vec<String> = Vec::new();
            let mut failed_batch_ids: Vec<String> = Vec::new();
            for batch_id in batch_ids.iter().rev() {
                match history.revert_batch(batch_id) {
                    Ok(paths) => {
                        if paths.is_empty() {
                            failed_batch_ids.push(batch_id.clone());
                        } else {
                            reverted_paths.extend(paths);
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "rewind",
                            "revert_batches: revert_batch({batch_id}) failed: {e}"
                        );
                        failed_batch_ids.push(batch_id.clone());
                    }
                }
            }
            (reverted_paths, failed_batch_ids)
        })
        .await
        .unwrap_or_else(|_| (Vec::new(), Vec::new()))
    };

    Json(serde_json::json!({
        "ok": failed_batch_ids.is_empty(),
        "revertedPaths": reverted_paths,
        "failedBatchIds": failed_batch_ids,
    }))
    .into_response()
}

fn run_git(workdir: &std::path::Path, args: &[&str]) -> Option<String> {
    let output = crate::util::hidden_sync_command("git")
        .current_dir(workdir)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn git_repo_info(workdir: &std::path::Path) -> (bool, Option<String>, Option<String>) {
    if run_git(workdir, &["rev-parse", "--is-inside-work-tree"]).as_deref() != Some("true") {
        return (false, None, None);
    }
    let branch = run_git(workdir, &["rev-parse", "--abbrev-ref", "HEAD"]);
    let toplevel = run_git(workdir, &["rev-parse", "--show-toplevel"]);
    let repo_name = toplevel.as_ref().and_then(|p| {
        std::path::Path::new(p)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
    });
    (true, repo_name, branch)
}

fn git_changed_file_count(workdir: &std::path::Path) -> usize {
    let Some(output) = run_git(workdir, &["status", "--porcelain"]) else {
        return 0;
    };
    output.lines().filter(|l| !l.is_empty()).count()
}

pub async fn handle_api_suggestions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SuggestionsBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock().clone();
    let tool_names: Vec<String> = state
        .tools_registry
        .iter()
        .map(|t| t.name.clone())
        .collect();

    let suggestions = crate::agent::suggestions::generate_rule_based_suggestions(
        &body.user_message,
        &body.assistant_response,
        &tool_names,
        &config.suggestions,
    );

    Json(serde_json::json!({
        "suggestions": suggestions,
        "count": suggestions.len(),
    }))
    .into_response()
}

#[derive(Deserialize)]
pub struct SuggestionsBody {
    pub user_message: String,
    pub assistant_response: String,
}

pub async fn handle_api_workflows_validate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(workflow): Json<crate::workflows::Workflow>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    match crate::workflows::validate_workflow(&workflow) {
        Ok(()) => Json(serde_json::json!({ "valid": true })).into_response(),
        Err(err) => Json(serde_json::json!({
            "valid": false,
            "error": err.to_string(),
        }))
        .into_response(),
    }
}

pub async fn handle_api_workflows_execute(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(workflow): Json<crate::workflows::Workflow>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    if let Err(err) = crate::workflows::validate_workflow(&workflow) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response();
    }

    let mut run = crate::workflows::WorkflowRun::new(workflow.id.clone(), "");
    run.variables = workflow.variables.clone();

    let cfg = state.config.lock().clone();
    let temperature = cfg.default_temperature;

    fn resolve_step_agent(
        agent: &crate::workflows::StepAgent,
    ) -> Option<crate::agent::registry::AgentInfo> {
        let rt = crate::agent::multi_agent_runtime::global_runtime()?;
        match agent {
            crate::workflows::StepAgent::Default => None,
            crate::workflows::StepAgent::ById { id } => rt.registry.get(id),
            crate::workflows::StepAgent::ByName { name } => {
                rt.registry.all().into_iter().find(|a| a.name == *name)
            }
        }
    }

    let resolver = |agent: &crate::workflows::StepAgent| -> Option<(String, String)> {
        resolve_step_agent(agent).map(|info| (info.id, info.name))
    };
    let executor = move |agent: crate::workflows::StepAgent, prompt: String| {
        let cfg = cfg.clone();
        let resolved = resolve_step_agent(&agent);
        async move {
            let (provider_override, model_override) = match resolved {
                Some(info) => (
                    (!info.provider.trim().is_empty()).then_some(info.provider),
                    (!info.model.trim().is_empty()).then_some(info.model),
                ),
                None => (None, None),
            };
            match crate::agent::run(
                cfg,
                Some(prompt.clone()),
                provider_override,
                model_override,
                temperature,
                Vec::new(),
                false,
                None,
                None,
                None,
            )
            .await
            {
                Ok(output) => {
                    let approx_in = (prompt.len() / 4) as u64;
                    let approx_out = (output.len() / 4) as u64;
                    Ok((output, approx_in, approx_out))
                }
                Err(e) => Err(format!("{e:#}")),
            }
        }
    };

    let engine = crate::workflows::WorkflowEngine::new();
    let result = engine.execute_run(&workflow, run, resolver, executor).await;

    Json(serde_json::json!({
        "status": format!("{:?}", result.status),
        "steps": result.step_results.len(),
        "output": result.output,
    }))
    .into_response()
}

pub async fn handle_api_rbac_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock().clone();

    if let Some(ref engine) = state.rbac {
        let roles: Vec<serde_json::Value> = engine
            .list_roles()
            .iter()
            .map(|r| {
                serde_json::json!({
                    "name": r.name,
                    "description": r.description,
                    "builtin": r.builtin,
                    "tools_count": r.allowed_tools.len(),
                })
            })
            .collect();
        let users = engine.list_users();

        Json(serde_json::json!({
            "enabled": true,
            "default_role": config.rbac.default_role,
            "cli_is_admin": config.rbac.cli_is_admin,
            "roles": roles,
            "users_count": users.len(),
        }))
        .into_response()
    } else {
        Json(serde_json::json!({
            "enabled": false,
            "message": "RBAC is disabled. Set [rbac] enabled = true in config to activate.",
        }))
        .into_response()
    }
}

pub async fn handle_api_rbac_users_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let Some(ref engine) = state.rbac else {
        return Json(serde_json::json!({
            "error": "RBAC is disabled",
            "users": []
        }))
        .into_response();
    };

    let users = engine.list_users();
    Json(serde_json::json!({ "users": users })).into_response()
}

pub async fn handle_api_rbac_user_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let Some(ref engine) = state.rbac else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "RBAC is disabled"})),
        )
            .into_response();
    };

    match engine.get_user(&user_id) {
        Some(user) => Json(serde_json::json!({ "user": user })).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("User '{}' not found", user_id)})),
        )
            .into_response(),
    }
}

pub async fn handle_api_rbac_users_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<crate::security::rbac::UserRecord>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let Some(ref engine) = state.rbac else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "RBAC is disabled"})),
        )
            .into_response();
    };

    match engine.create_user(body) {
        Ok(()) => Json(serde_json::json!({"status": "ok"})).into_response(),
        Err(e) => (StatusCode::CONFLICT, Json(serde_json::json!({"error": e}))).into_response(),
    }
}

pub async fn handle_api_rbac_user_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
    Json(mut body): Json<crate::security::rbac::UserRecord>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let Some(ref engine) = state.rbac else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "RBAC is disabled"})),
        )
            .into_response();
    };

    body.user_id = user_id;
    match engine.update_user(body) {
        Ok(()) => Json(serde_json::json!({"status": "ok"})).into_response(),
        Err(e) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": e}))).into_response(),
    }
}

pub async fn handle_api_rbac_user_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let Some(ref engine) = state.rbac else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "RBAC is disabled"})),
        )
            .into_response();
    };

    match engine.delete_user(&user_id) {
        Ok(()) => Json(serde_json::json!({"status": "ok"})).into_response(),
        Err(e) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": e}))).into_response(),
    }
}

pub async fn handle_api_rbac_roles_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let Some(ref engine) = state.rbac else {
        return Json(serde_json::json!({
            "error": "RBAC is disabled",
            "roles": []
        }))
        .into_response();
    };

    let roles: Vec<serde_json::Value> = engine
        .list_roles()
        .iter()
        .map(|r| {
            serde_json::json!({
                "name": r.name,
                "description": r.description,
                "allowed_tools": r.allowed_tools,
                "allowed_workspaces": r.allowed_workspaces,
                "builtin": r.builtin,
            })
        })
        .collect();

    Json(serde_json::json!({ "roles": roles })).into_response()
}

pub async fn handle_api_rbac_check(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RbacCheckBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let Some(ref engine) = state.rbac else {
        return Json(serde_json::json!({
            "allowed": true,
            "reason": "RBAC is disabled  - all access is permitted",
        }))
        .into_response();
    };

    let identity = crate::security::rbac::CallerIdentity {
        user_id: body.user_id.clone(),
        display_name: None,
        roles: vec![],
        auth_source: crate::security::rbac::AuthSource::ApiKey,
        channel: None,
        mfa_verified: false,
    };

    let result = engine.authorize_tool(&identity, &body.tool_name);

    Json(serde_json::json!({
        "allowed": result.allowed,
        "reason": result.reason,
        "user_id": body.user_id,
        "tool": body.tool_name,
    }))
    .into_response()
}

#[derive(Deserialize)]
pub struct RbacCheckBody {
    pub user_id: String,
    pub tool_name: String,
    pub roles: Option<Vec<String>>,
}

pub async fn handle_api_guardrails_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock().clone();
    Json(serde_json::json!({
        "enabled": config.guardrails.enabled,
        "default_policy": config.guardrails.default_policy,
        "rules_count": config.guardrails.rules.len(),
        "rate_limits_count": config.guardrails.rate_limits.len(),
        "max_calls_per_session": config.guardrails.max_calls_per_session,
        "bypass_tools": config.guardrails.bypass_tools,
        "rules": config.guardrails.rules,
    }))
    .into_response()
}

pub async fn handle_api_tool_groups(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock().clone();
    let registry = crate::tools::handler::groups::ToolGroupRegistry::from_config(&config.tool_groups)
        .with_defaults();

    let groups: Vec<serde_json::Value> = registry
        .list_groups()
        .iter()
        .map(|g| {
            serde_json::json!({
                "name": g.name,
                "description": g.description,
                "tools": g.tools,
                "enabled": g.enabled,
                "priority": g.priority,
                "active": registry.active_group_names().contains(&g.name),
            })
        })
        .collect();

    Json(serde_json::json!({
        "groups": groups,
        "active_tools": registry.active_tools(),
    }))
    .into_response()
}

pub async fn handle_api_reinforcement(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock().clone();

    match crate::agent::reward::reinforcement::global_reinforcement_engine() {
        Some(engine) => {
            let adjustment = engine.get_policy_adjustment();
            let baselines = engine.baselines();
            Json(serde_json::json!({
                "enabled": config.reinforcement.enabled,
                "total_turns": engine.total_turns(),
                "baselines": baselines,
                "trend": format!("{:?}", adjustment.trend),
                "confidence": adjustment.confidence,
                "temperature_delta": adjustment.temperature_delta,
                "model_hint": adjustment.model_hint,
                "category_count": adjustment.category_strategies.len(),
            }))
            .into_response()
        }
        None => Json(serde_json::json!({
            "enabled": config.reinforcement.enabled,
            "total_turns": 0,
            "baselines": serde_json::json!({}),
            "trend": "InsufficientData",
            "confidence": 0.0,
            "temperature_delta": 0.0,
            "model_hint": serde_json::Value::Null,
            "category_count": 0,
        }))
        .into_response(),
    }
}

pub async fn handle_api_learning_features(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock().clone();

    Json(serde_json::json!({
        "self_eval": {
            "enabled": config.self_eval.enabled,
            "eval_votes": config.self_eval.eval_votes,
            "accept_threshold": config.self_eval.accept_threshold,
        },
        "feedback": {
            "enabled": config.feedback.enabled,
            "max_entries": config.feedback.max_entries,
        },
        "experience": {
            "enabled": config.experience.enabled,
            "capacity": config.experience.capacity,
            "few_shot_count": config.experience.few_shot_count,
        },
        "self_reflection": {
            "enabled": config.self_reflection.enabled,
            "reflect_interval": config.self_reflection.reflect_interval,
            "llm_reflection": config.self_reflection.llm_reflection,
        },
        "prompt_optimizer": {
            "enabled": config.prompt_optimizer.enabled,
            "min_samples": config.prompt_optimizer.min_samples,
        },
        "skill_evolution": {
            "enabled": config.skill_evolution.enabled,
        },
        "reinforcement": {
            "enabled": config.reinforcement.enabled,
            "learning_rate": config.reinforcement.learning_rate,
            "adaptive_routing": config.reinforcement.adaptive_routing,
            "adaptive_temperature": config.reinforcement.adaptive_temperature,
        },
    }))
    .into_response()
}

pub async fn handle_api_agents_list(
    State(_state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&_state, &headers) {
        return e.into_response();
    }

    if let Some(rt) = crate::agent::multi_agent_runtime::global_runtime() {
        let agents = rt.registry.all();
        let agents_json: Vec<serde_json::Value> = agents
            .iter()
            .map(|a| {
                serde_json::json!({
                    "id": a.id,
                    "name": a.name,
                    "role": a.role,
                    "state": format!("{:?}", a.state),
                    "capabilities": a.capabilities.iter().map(|c| &c.name).collect::<Vec<_>>(),
                    "current_task": a.current_task,
                    "tasks_completed": a.tasks_completed,
                    "tasks_failed": a.tasks_failed,
                    "last_heartbeat": a.last_heartbeat.to_rfc3339(),
                })
            })
            .collect();
        Json(serde_json::json!({"agents": agents_json})).into_response()
    } else {
        Json(serde_json::json!({"agents": []})).into_response()
    }
}

pub async fn handle_api_agents_status(
    State(_state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&_state, &headers) {
        return e.into_response();
    }

    if let Some(rt) = crate::agent::multi_agent_runtime::global_runtime() {
        let report = rt.supervisor.health_report();
        Json(serde_json::json!({
            "total_agents": report.total_agents,
            "healthy": report.healthy,
            "unhealthy": report.unhealthy,
            "shutting_down": report.shutting_down,
            "state_summary": report.state_summary,
        }))
        .into_response()
    } else {
        Json(serde_json::json!({
            "total_agents": 0,
            "healthy": 0,
            "unhealthy": 0,
            "shutting_down": 0,
            "state_summary": {},
        }))
        .into_response()
    }
}

pub async fn handle_api_tasks_status(
    State(_state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&_state, &headers) {
        return e.into_response();
    }

    if let Some(rt) = crate::agent::multi_agent_runtime::global_runtime() {
        let summary = rt.task_queue.inner().status_summary();
        Json(serde_json::json!({
            "pending": rt.task_queue.pending_count(),
            "running": rt.task_queue.running_count(),
            "status_summary": summary,
        }))
        .into_response()
    } else {
        Json(serde_json::json!({
            "pending": 0,
            "running": 0,
            "status_summary": {},
        }))
        .into_response()
    }
}

pub async fn handle_api_coordination_locks(
    State(_state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&_state, &headers) {
        return e.into_response();
    }

    if let Some(rt) = crate::agent::multi_agent_runtime::global_runtime() {
        let locks = rt.coordinator.locks().all_locks();
        let locks_json: Vec<serde_json::Value> = locks
            .iter()
            .map(|(resource, owner, reason)| {
                serde_json::json!({
                    "resource": resource,
                    "owner": owner,
                    "reason": reason,
                })
            })
            .collect();
        Json(serde_json::json!({"locks": locks_json, "count": locks.len()})).into_response()
    } else {
        Json(serde_json::json!({"locks": [], "count": 0})).into_response()
    }
}

pub async fn handle_api_multi_agent_status(
    State(_state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&_state, &headers) {
        return e.into_response();
    }

    if let Some(rt) = crate::agent::multi_agent_runtime::global_runtime() {
        let summary = rt.health_summary();
        Json(serde_json::json!({
            "initialized": true,
            "total_agents": summary.total_agents,
            "healthy_agents": summary.healthy_agents,
            "unhealthy_agents": summary.unhealthy_agents,
            "pending_tasks": summary.pending_tasks,
            "running_tasks": summary.running_tasks,
            "blackboard_entries": summary.blackboard_entries,
        }))
        .into_response()
    } else {
        Json(serde_json::json!({
            "initialized": false,
            "total_agents": 0,
            "healthy_agents": 0,
            "unhealthy_agents": 0,
            "pending_tasks": 0,
            "running_tasks": 0,
            "blackboard_entries": 0,
        }))
        .into_response()
    }
}

pub async fn handle_api_sessions_events(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> axum::response::Response {
    use axum::response::sse::{Event, KeepAlive, Sse};
    use futures_util::stream::StreamExt;
    use std::convert::Infallible;
    use std::time::Duration;
    use tokio_stream::wrappers::BroadcastStream;

    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let snapshot_ids = state.session_run_state.snapshot();
    let rx = state.session_run_state.subscribe();

    let snapshot_event = Event::default()
        .event("snapshot")
        .json_data(serde_json::json!({ "running": snapshot_ids }))
        .unwrap_or_else(|_| Event::default().event("snapshot").data("{\"running\":[]}"));
    let snapshot_stream =
        futures_util::stream::once(async move { Ok::<Event, Infallible>(snapshot_event) });

    let delta_stream = BroadcastStream::new(rx).filter_map(|item| async move {
        match item {
            Ok(evt) => match Event::default().event("run_state").json_data(&evt) {
                Ok(ev) => Some(Ok::<Event, Infallible>(ev)),
                Err(_) => None,
            },
            Err(_) => None,
        }
    });

    let stream = snapshot_stream.chain(delta_stream);

    Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(25))
                .text("keep-alive"),
        )
        .into_response()
}

pub async fn handle_api_tips_next(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(svc) = crate::services::try_get_services() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "services unavailable"})),
        )
            .into_response();
    };
    let mut tips = svc.tips.lock();
    if let Some(tip) = tips.next_tip().cloned() {
        tips.mark_shown(&tip.id);
        Json(serde_json::json!({ "tip": tip })).into_response()
    } else {
        Json(serde_json::json!({ "tip": null })).into_response()
    }
}

#[derive(Debug, Deserialize)]
pub struct TipDismissBody {
    pub id: String,
}

pub async fn handle_api_tips_dismiss(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(body): Json<TipDismissBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    if let Some(svc) = crate::services::try_get_services() {
        svc.tips.lock().dismiss(&body.id);
    }
    Json(serde_json::json!({"ok": true})).into_response()
}

#[derive(Debug, Deserialize)]
pub struct RemoteSessionRegisterBody {
    pub session_id: String,
    pub url: String,
    #[serde(default)]
    pub auth_token: Option<String>,
    #[serde(default)]
    pub signing_secret: Option<String>,
    #[serde(default)]
    pub connect: bool,
}

pub async fn handle_api_remote_sessions_list(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let sessions = if let Some(svc) = crate::services::try_get_services() {
        svc.remote_sessions.list_sessions().await
    } else {
        Vec::new()
    };
    Json(serde_json::json!({ "sessions": sessions })).into_response()
}

pub async fn handle_api_remote_sessions_register(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(body): Json<RemoteSessionRegisterBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(svc) = crate::services::try_get_services() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "services unavailable"})),
        )
            .into_response();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let session = crate::remote::manager::RemoteSession {
        session_id: body.session_id.clone(),
        url: body.url.clone(),
        status: crate::remote::manager::RemoteSessionStatus::Disconnected,
        created_at_ms: now,
        last_activity_ms: now,
        auth_token: body.auth_token.clone(),
        signing_secret: body.signing_secret.clone(),
    };
    svc.remote_sessions.register_session(session).await;
    if body.connect {
        if let Err(e) = svc.remote_sessions.connect_session(&body.session_id).await {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    }
    Json(serde_json::json!({"ok": true, "session_id": body.session_id})).into_response()
}

pub async fn handle_claude_code_hook(
    Json(_payload): Json<serde_json::Value>,
) -> impl axum::response::IntoResponse {
    axum::http::StatusCode::NOT_IMPLEMENTED
}
