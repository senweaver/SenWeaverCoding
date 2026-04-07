// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! REST API handlers for the web dashboard.
//!
//! All `/api/*` routes require bearer token authentication (PairingGuard).

use super::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Json},
};
use serde::Deserialize;

const MASKED_SECRET: &str = "***MASKED***";

// ── Bearer token auth extractor ─────────────────────────────────

/// Extract and validate bearer token from Authorization header.
fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|auth| auth.strip_prefix("Bearer "))
}

/// Verify bearer token against PairingGuard. Returns error response if unauthorized.
pub(super) fn require_auth(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if !state.pairing.require_pairing() {
        return Ok(());
    }

    let token = extract_bearer_token(headers).unwrap_or("");
    if state.pairing.is_authenticated(token) {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "Unauthorized — pair first via POST /pair, then send Authorization: Bearer <token>"
            })),
        ))
    }
}

// ── Query parameters ─────────────────────────────────────────────

#[derive(Deserialize)]
pub struct MemoryQuery {
    pub query: Option<String>,
    pub category: Option<String>,
    /// Filter memories created at or after (RFC 3339 / ISO 8601)
    pub since: Option<String>,
    /// Filter memories created at or before (RFC 3339 / ISO 8601)
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

// ── Handlers ────────────────────────────────────────────────────

/// GET /api/status — system status overview
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
        "model": state.model,
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

/// GET /api/config — current config (api_key masked)
pub async fn handle_api_config_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock().clone();

    // Serialize to TOML after masking sensitive fields.
    // SECURITY: Error messages don't expose TOML internals to prevent information disclosure.
    let masked_config = mask_sensitive_fields(&config);
    let toml_str = match toml::to_string_pretty(&masked_config) {
        Ok(s) => s,
        Err(_e) => {
            // Return generic error without exposing TOML parse details
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

/// PUT /api/config — update config from TOML body
pub async fn handle_api_config_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    // Parse the incoming TOML
    let incoming: crate::config::Config = match toml::from_str(&body) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("Invalid TOML: {e}")})),
            )
                .into_response();
        }
    };

    let current_config = state.config.lock().clone();
    let new_config = hydrate_config_for_save(incoming, &current_config);

    if let Err(e) = new_config.validate() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("Invalid config: {e}")})),
        )
            .into_response();
    }

    // Save to disk
    if let Err(e) = new_config.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to save config: {e}")})),
        )
            .into_response();
    }

    // Update in-memory config
    *state.config.lock() = new_config;

    Json(serde_json::json!({"status": "ok"})).into_response()
}

// ── Provider API ────────────────────────────────────────────────────────────

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

/// GET /api/provider — current provider config (api_key masked)
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

/// GET /api/channels — list channel configs (tokens masked)
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

/// PUT /api/channels — update channel configs (partial merge)
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
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("Invalid config: {e}")})),
        )
            .into_response();
    }

    if let Err(e) = config.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to save: {e}")})),
        )
            .into_response();
    }

    *state.config.lock() = config;

    Json(serde_json::json!({"status": "ok"})).into_response()
}

/// PUT /api/provider — update provider config
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
    // api_key: empty string clears it; None leaves it unchanged; explicit value sets it.
    match &body.api_key {
        Some(k) if k.is_empty() => config.api_key = None,
        Some(k) => config.api_key = Some(k.clone()),
        None => {}
    }

    // Gateway settings
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
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("Invalid config: {e}")})),
        )
            .into_response();
    }

    if let Err(e) = config.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to save config: {e}")})),
        )
            .into_response();
    }

    *state.config.lock() = config;

    Json(serde_json::json!({"status": "ok"})).into_response()
}

/// GET /api/tools — list registered tool specs
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
    /// Exact skill names (as in SKILL.toml / frontmatter) to exclude from the agent.
    #[serde(default)]
    pub disabled_skills: Vec<String>,
}

/// GET /api/skills — list discovered skills and `[skills].disabled_skills`
pub async fn handle_api_skills_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock().clone();
    let skills = crate::skills::discover_skills(&config.workspace_dir, &config);
    let disabled: std::collections::HashSet<String> = config
        .skills
        .disabled_skills
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let workspace_skills_dir = config.workspace_dir.join("skills");
    let list: Vec<serde_json::Value> = skills
        .into_iter()
        .map(|s| {
            let enabled = !disabled.contains(&s.name);
            let path_str = s.location.as_ref().map(|p| p.display().to_string());
            serde_json::json!({
                "name": s.name,
                "description": s.description,
                "version": s.version,
                "author": s.author,
                "tags": s.tags,
                "tools_count": s.tools.len(),
                "prompts_count": s.prompts.len(),
                "enabled": enabled,
                "path": path_str,
            })
        })
        .collect();

    Json(serde_json::json!({
        "workspace_skills_dir": workspace_skills_dir.display().to_string(),
        "open_skills_enabled": config.skills.open_skills_enabled,
        "allow_scripts": config.skills.allow_scripts,
        "disabled_skills": config.skills.disabled_skills,
        "skills": list,
    }))
    .into_response()
}

/// PUT /api/skills — set `[skills].disabled_skills` (replaces list)
pub async fn handle_api_skills_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SkillsPutBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let mut config = state.config.lock().clone();
    config.skills.disabled_skills = body
        .disabled_skills
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if let Err(e) = config.validate() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("Invalid config: {e}")})),
        )
            .into_response();
    }

    if let Err(e) = config.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to save config: {e}")})),
        )
            .into_response();
    }

    *state.config.lock() = config;

    Json(serde_json::json!({"status": "ok"})).into_response()
}

/// GET /api/cron — list cron jobs
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
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to list cron jobs: {e}")})),
        )
            .into_response(),
    }
}

/// POST /api/cron — add a new cron job
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
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("Failed to add cron job: {e}")})),
        )
            .into_response();
    }

    // Determine job type: explicit field, or infer "agent" when prompt is provided.
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
            session_target,
            model,
            delivery,
            delete_after_run,
            allowed_tools,
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
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to add cron job: {e}")})),
        )
            .into_response(),
    }
}

/// GET /api/cron/:id/runs — list recent runs for a cron job
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

    // Verify the job exists before listing runs.
    if let Err(e) = crate::cron::get_job(&config, &id) {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("Cron job not found: {e}")})),
        )
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
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to list cron runs: {e}")})),
        )
            .into_response(),
    }
}

/// PATCH /api/cron/:id — update an existing cron job
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

    // Build the schedule from the provided expression string (if any).
    let schedule = match body.schedule {
        Some(expr) if !expr.trim().is_empty() => Some(crate::cron::Schedule::Cron {
            expr: expr.trim().to_string(),
            tz: None,
        }),
        _ => None,
    };

    // Route the edited text to the correct field based on the job's stored type.
    // The frontend sends a single textarea value; for agent jobs it is the prompt,
    // for shell jobs it is the command.
    let existing = match crate::cron::get_job(&config, &id) {
        Ok(j) => j,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": format!("Cron job not found: {e}")})),
            )
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
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to update cron job: {e}")})),
        )
            .into_response(),
    }
}

/// DELETE /api/cron/:id — remove a cron job
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
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to remove cron job: {e}")})),
        )
            .into_response(),
    }
}

/// GET /api/cron/settings — return cron subsystem settings
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

/// PATCH /api/cron/settings — update cron subsystem settings
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
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to save config: {e}")})),
        )
            .into_response();
    }

    *state.config.lock() = config.clone();

    Json(serde_json::json!({
        "status": "ok",
        "enabled": config.cron.enabled,
        "catch_up_on_startup": config.cron.catch_up_on_startup,
        "max_run_history": config.cron.max_run_history,
    }))
    .into_response()
}

/// GET /api/integrations — list all integrations with status
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

/// GET /api/integrations/settings — return per-integration settings (enabled + category)
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

/// POST /api/doctor — run diagnostics
pub async fn handle_api_doctor(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock().clone();
    let results = crate::doctor::diagnose(&config);

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

/// GET /api/memory — list or search memory entries
pub async fn handle_api_memory_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<MemoryQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    // Use recall when query or time range is provided
    if params.query.is_some() || params.since.is_some() || params.until.is_some() {
        let query = params.query.as_deref().unwrap_or("");
        let since = params.since.as_deref();
        let until = params.until.as_deref();
        match state.mem.recall(query, 50, None, since, until).await {
            Ok(entries) => Json(serde_json::json!({"entries": entries})).into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Memory recall failed: {e}")})),
            )
                .into_response(),
        }
    } else {
        // List mode
        let category = params.category.as_deref().map(|cat| match cat {
            "core" => crate::memory::MemoryCategory::Core,
            "daily" => crate::memory::MemoryCategory::Daily,
            "conversation" => crate::memory::MemoryCategory::Conversation,
            other => crate::memory::MemoryCategory::Custom(other.to_string()),
        });

        match state.mem.list(category.as_ref(), None).await {
            Ok(entries) => Json(serde_json::json!({"entries": entries})).into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Memory list failed: {e}")})),
            )
                .into_response(),
        }
    }
}

/// POST /api/memory — store a memory entry
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
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Memory store failed: {e}")})),
        )
            .into_response(),
    }
}

/// DELETE /api/memory/:key — delete a memory entry
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
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Memory forget failed: {e}")})),
        )
            .into_response(),
    }
}

/// GET /api/cost — cost summary
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
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Cost summary failed: {e}")})),
            )
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

/// GET /api/cli-tools — discovered CLI tools
pub async fn handle_api_cli_tools(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let tools = crate::tools::cli_discovery::discover_cli_tools(&[], &[]);

    Json(serde_json::json!({"cli_tools": tools})).into_response()
}

/// GET /api/health — component health snapshot
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

// ── Helpers ─────────────────────────────────────────────────────

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
    mask_optional_secret(&mut masked.browser.computer_use.api_key);
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
    if let Some(clawdtalk) = masked.channels_config.clawdtalk.as_mut() {
        mask_required_secret(&mut clawdtalk.api_key);
        mask_optional_secret(&mut clawdtalk.webhook_secret);
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
            // Never persist UI placeholders to disk when no safe restore target exists.
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
            // Never persist UI placeholders to disk when no safe restore target exists.
            incoming_route.api_key = None;
        }
    }
}

/// GET /api/channels — list channel configs (tokens masked)

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
        &mut incoming.browser.computer_use.api_key,
        &current.browser.computer_use.api_key,
    );
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
        incoming.channels_config.clawdtalk.as_mut(),
        current.channels_config.clawdtalk.as_ref(),
    ) {
        restore_required_secret(&mut incoming_ch.api_key, &current_ch.api_key);
        restore_optional_secret(&mut incoming_ch.webhook_secret, &current_ch.webhook_secret);
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
    // These are runtime-computed fields skipped from TOML serialization.
    incoming.config_path = current.config_path.clone();
    incoming.workspace_dir = current.workspace_dir.clone();
    incoming
}

// ── Session API handlers ─────────────────────────────────────────

/// GET /api/sessions — list gateway sessions
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
            "message": "Session persistence is disabled"
        }))
        .into_response();
    };

    let all_metadata = backend.list_sessions_with_metadata();
    let gw_sessions: Vec<serde_json::Value> = all_metadata
        .into_iter()
        .filter_map(|meta| {
            let session_id = meta.key.strip_prefix("gw_")?;
            let mut entry = serde_json::json!({
                "session_id": session_id,
                "created_at": meta.created_at.to_rfc3339(),
                "last_activity": meta.last_activity.to_rfc3339(),
                "message_count": meta.message_count,
            });
            if let Some(name) = meta.name {
                entry["name"] = serde_json::Value::String(name);
            }
            Some(entry)
        })
        .collect();

    Json(serde_json::json!({ "sessions": gw_sessions })).into_response()
}

/// GET /api/sessions/{id}/messages — load persisted gateway WebSocket chat transcript
pub async fn handle_api_session_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let Some(ref backend) = state.session_backend else {
        return Json(serde_json::json!({
            "session_id": id,
            "messages": [],
            "session_persistence": false,
        }))
        .into_response();
    };

    let session_key = format!("gw_{id}");
    let msgs = backend.load(&session_key);
    let messages: Vec<serde_json::Value> = msgs
        .into_iter()
        .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
        .collect();

    Json(serde_json::json!({
        "session_id": id,
        "messages": messages,
        "session_persistence": true,
    }))
    .into_response()
}

/// DELETE /api/sessions/{id} — delete a gateway session
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

    let session_key = format!("gw_{id}");
    match backend.delete_session(&session_key) {
        Ok(true) => Json(serde_json::json!({"deleted": true, "session_id": id})).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Session not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to delete session: {e}")})),
        )
            .into_response(),
    }
}

/// PUT /api/sessions/{id} — rename a gateway session
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

    let name = body["name"].as_str().unwrap_or("").trim();
    if name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "name is required"})),
        )
            .into_response();
    }

    let session_key = format!("gw_{id}");

    // Verify the session exists before renaming
    let sessions = backend.list_sessions();
    if !sessions.contains(&session_key) {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Session not found"})),
        )
            .into_response();
    }

    match backend.set_session_name(&session_key, name) {
        Ok(()) => Json(serde_json::json!({"session_id": id, "name": name})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to rename session: {e}")})),
        )
            .into_response(),
    }
}

// ── Claude Code hook endpoint ────────────────────────────────────

/// POST /hooks/claude-code-typescript-src— receives HTTP hook events from Claude Code
/// sessions spawned by [`ClaudeCodeRunnerTool`].
///
/// Claude Code posts structured JSON describing tool executions, completions,
/// and errors. This handler logs the event and (when a Slack channel is
/// configured) could be wired to update a Slack message in-place.
pub async fn handle_claude_code_hook(
    State(state): State<AppState>,
    Json(payload): Json<crate::tools::claude_code_runner::ClaudeCodeHookEvent>,
) -> impl IntoResponse {
    // Do not require bearer-token auth: Claude Code subprocesses cannot easily
    // obtain a pairing token, and the hook carries a session_id that ties it
    // back to a session we spawned.
    let _ = &state; // retained for future Slack update wiring

    tracing::info!(
        session_id = %payload.session_id,
        event_type = %payload.event_type,
        tool_name = ?payload.tool_name,
        summary = ?payload.summary,
        "Claude Code hook event received"
    );

    Json(serde_json::json!({ "ok": true }))
}

// ── Suggestions API ─────────────────────────────────────────────

/// POST /api/suggestions — generate context-aware suggestions
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

// ── Workflows API ───────────────────────────────────────────────

/// POST /api/workflows/validate — validate a workflow JSON body (no execution).
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

/// POST /api/workflows/execute — validate and execute a workflow, returning results.
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

    let resolver = |_agent: &crate::workflows::StepAgent| -> Option<(String, String)> { None };
    let executor = |agent: crate::workflows::StepAgent, prompt: String| async move {
        Ok((
            format!("Executed ({agent:?}): {prompt}"),
            prompt.len() as u64 / 4,
            100u64,
        ))
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

// ── RBAC API ────────────────────────────────────────────────────

/// GET /api/rbac/status — RBAC system status and configuration overview
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

/// GET /api/rbac/users — list all registered users
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

/// GET /api/rbac/users/{user_id} — get a specific user
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

/// POST /api/rbac/users — create a new user
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

/// PUT /api/rbac/users/{user_id} — update an existing user
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

/// DELETE /api/rbac/users/{user_id} — delete a user
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

/// GET /api/rbac/roles — list all role definitions
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

/// POST /api/rbac/check — check if a user can use a specific tool
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
            "reason": "RBAC is disabled — all access is permitted",
        }))
        .into_response();
    };

    // Never trust client-supplied roles — let the RBAC engine resolve
    // the user's actual roles from the UserStore / default_role config.
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

// ── Guardrails API ──────────────────────────────────────────────

/// GET /api/guardrails — get current guardrails configuration
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

// ── Tool Groups API ─────────────────────────────────────────────

/// GET /api/tool-groups — list tool groups and their status
pub async fn handle_api_tool_groups(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock().clone();
    let registry = crate::tools::tool_groups::ToolGroupRegistry::from_config(&config.tool_groups)
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

/// GET `/api/reinforcement` - Get reinforcement learning engine status.
pub async fn handle_api_reinforcement(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock().clone();

    let engine = crate::agent::reinforcement::ReinforcementEngine::new(&config.reinforcement);
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

/// GET `/api/evolution` - Get self-evolution system overview.
pub async fn handle_api_evolution(
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

// ── Multi-Agent Runtime API ───────────────────────────────────────

/// GET /api/agents — list all registered agents in the multi-agent runtime
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

/// GET /api/agents/status — agent registry status summary
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

/// GET /api/tasks — task queue status
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

/// GET /api/coordination/locks — list active distributed locks
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

/// GET /api/multi-agent/status — overall multi-agent runtime health
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::{AppState, GatewayRateLimiter, IdempotencyStore, nodes};
    use crate::memory::{Memory, MemoryCategory, MemoryEntry};
    use crate::providers::Provider;
    use crate::security::pairing::PairingGuard;
    use async_trait::async_trait;
    use axum::response::IntoResponse;
    use http_body_util::BodyExt;
    use parking_lot::Mutex;
    use std::sync::Arc;
    use std::time::Duration;

    struct MockMemory;

    #[async_trait]
    impl Memory for MockMemory {
        fn name(&self) -> &str {
            "mock"
        }

        async fn store(
            &self,
            _key: &str,
            _content: &str,
            _category: MemoryCategory,
            _session_id: Option<&str>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn recall(
            &self,
            _query: &str,
            _limit: usize,
            _session_id: Option<&str>,
            _since: Option<&str>,
            _until: Option<&str>,
        ) -> anyhow::Result<Vec<MemoryEntry>> {
            Ok(Vec::new())
        }

        async fn get(&self, _key: &str) -> anyhow::Result<Option<MemoryEntry>> {
            Ok(None)
        }

        async fn list(
            &self,
            _category: Option<&MemoryCategory>,
            _session_id: Option<&str>,
        ) -> anyhow::Result<Vec<MemoryEntry>> {
            Ok(Vec::new())
        }

        async fn forget(&self, _key: &str) -> anyhow::Result<bool> {
            Ok(false)
        }

        async fn count(&self) -> anyhow::Result<usize> {
            Ok(0)
        }

        async fn health_check(&self) -> bool {
            true
        }
    }

    struct MockProvider;

    #[async_trait]
    impl Provider for MockProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            Ok("ok".to_string())
        }
    }

    fn test_state(config: crate::config::Config) -> AppState {
        AppState {
            config: Arc::new(Mutex::new(config)),
            provider: Arc::new(MockProvider),
            model: "test-model".into(),
            temperature: 0.0,
            mem: Arc::new(MockMemory),
            auto_save: false,
            webhook_secret_hash: None,
            pairing: Arc::new(PairingGuard::new(false, &[])),
            trust_forwarded_headers: false,
            rate_limiter: Arc::new(GatewayRateLimiter::new(100, 100, 100)),
            auth_limiter: Arc::new(crate::gateway::auth_rate_limit::AuthRateLimiter::new()),
            idempotency_store: Arc::new(IdempotencyStore::new(Duration::from_secs(300), 1000)),
            whatsapp: None,
            whatsapp_app_secret: None,
            linq: None,
            linq_signing_secret: None,
            nextcloud_talk: None,
            nextcloud_talk_webhook_secret: None,
            wati: None,
            gmail_push: None,
            observer: Arc::new(crate::observability::NoopObserver),
            tools_registry: Arc::new(Vec::new()),
            cost_tracker: None,
            event_tx: tokio::sync::broadcast::channel(16).0,
            shutdown_tx: tokio::sync::watch::channel(false).0,
            node_registry: Arc::new(nodes::NodeRegistry::new(16)),
            session_backend: None,
            device_registry: None,
            pending_pairings: None,
            path_prefix: String::new(),
            rbac: None,
            canvas_store: crate::tools::canvas::CanvasStore::new(),
            #[cfg(feature = "webauthn")]
            webauthn: None,
        }
    }

    async fn response_json(response: axum::response::Response) -> serde_json::Value {
        let body = response
            .into_body()
            .collect()
            .await
            .expect("response body")
            .to_bytes();
        serde_json::from_slice(&body).expect("valid json response")
    }

    #[test]
    fn masking_keeps_toml_valid_and_preserves_api_keys_type() {
        let mut cfg = crate::config::Config::default();
        cfg.api_key = Some("sk-live-123".to_string());
        cfg.reliability.api_keys = vec!["rk-1".to_string(), "rk-2".to_string()];
        cfg.gateway.paired_tokens = vec!["pair-token-1".to_string()];
        cfg.tunnel.cloudflare = Some(crate::config::schema::CloudflareTunnelConfig {
            token: "cf-token".to_string(),
        });
        cfg.memory.qdrant.api_key = Some("qdrant-key".to_string());
        cfg.channels_config.wati = Some(crate::config::schema::WatiConfig {
            api_token: "wati-token".to_string(),
            api_url: "https://live-mt-server.wati.io".to_string(),
            tenant_id: None,
            allowed_numbers: vec![],
            proxy_url: None,
        });
        cfg.channels_config.feishu = Some(crate::config::schema::FeishuConfig {
            app_id: "cli_aabbcc".to_string(),
            app_secret: "feishu-secret".to_string(),
            encrypt_key: Some("feishu-encrypt".to_string()),
            verification_token: Some("feishu-verify".to_string()),
            allowed_users: vec!["*".to_string()],
            receive_mode: crate::config::schema::LarkReceiveMode::Websocket,
            port: None,
            proxy_url: None,
        });
        cfg.channels_config.email = Some(crate::channels::email_channel::EmailConfig {
            imap_host: "imap.example.com".to_string(),
            imap_port: 993,
            imap_folder: "INBOX".to_string(),
            smtp_host: "smtp.example.com".to_string(),
            smtp_port: 465,
            smtp_tls: true,
            username: "agent@example.com".to_string(),
            password: "email-password-secret".to_string(),
            from_address: "agent@example.com".to_string(),
            idle_timeout_secs: 1740,
            allowed_senders: vec!["*".to_string()],
            default_subject: "SenWeaverCoding Message".to_string(),
        });
        cfg.model_routes = vec![crate::config::schema::ModelRouteConfig {
            hint: "reasoning".to_string(),
            provider: "openrouter".to_string(),
            model: "anthropic/claude-sonnet-4.6".to_string(),
            api_key: Some("route-model-key".to_string()),
        }];
        cfg.embedding_routes = vec![crate::config::schema::EmbeddingRouteConfig {
            hint: "semantic".to_string(),
            provider: "openai".to_string(),
            model: "text-embedding-3-small".to_string(),
            dimensions: Some(1536),
            api_key: Some("route-embed-key".to_string()),
        }];

        let masked = mask_sensitive_fields(&cfg);
        let toml = toml::to_string_pretty(&masked).expect("masked config should serialize");
        let parsed: crate::config::Config =
            toml::from_str(&toml).expect("masked config should remain valid TOML for Config");

        assert_eq!(parsed.api_key.as_deref(), Some(MASKED_SECRET));
        assert_eq!(
            parsed.reliability.api_keys,
            vec![MASKED_SECRET.to_string(), MASKED_SECRET.to_string()]
        );
        assert_eq!(
            parsed.gateway.paired_tokens,
            vec![MASKED_SECRET.to_string()]
        );
        assert_eq!(
            parsed.tunnel.cloudflare.as_ref().map(|v| v.token.as_str()),
            Some(MASKED_SECRET)
        );
        assert_eq!(
            parsed
                .channels_config
                .wati
                .as_ref()
                .map(|v| v.api_token.as_str()),
            Some(MASKED_SECRET)
        );
        assert_eq!(parsed.memory.qdrant.api_key.as_deref(), Some(MASKED_SECRET));
        assert_eq!(
            parsed
                .channels_config
                .feishu
                .as_ref()
                .map(|v| v.app_secret.as_str()),
            Some(MASKED_SECRET)
        );
        assert_eq!(
            parsed
                .channels_config
                .feishu
                .as_ref()
                .and_then(|v| v.encrypt_key.as_deref()),
            Some(MASKED_SECRET)
        );
        assert_eq!(
            parsed
                .channels_config
                .feishu
                .as_ref()
                .and_then(|v| v.verification_token.as_deref()),
            Some(MASKED_SECRET)
        );
        assert_eq!(
            parsed
                .model_routes
                .first()
                .and_then(|v| v.api_key.as_deref()),
            Some(MASKED_SECRET)
        );
        assert_eq!(
            parsed
                .embedding_routes
                .first()
                .and_then(|v| v.api_key.as_deref()),
            Some(MASKED_SECRET)
        );
        assert_eq!(
            parsed
                .channels_config
                .email
                .as_ref()
                .map(|v| v.password.as_str()),
            Some(MASKED_SECRET)
        );
    }

    #[test]
    fn hydrate_config_for_save_restores_masked_secrets_and_paths() {
        let mut current = crate::config::Config::default();
        current.config_path = std::path::PathBuf::from("/tmp/current/config.toml");
        current.workspace_dir = std::path::PathBuf::from("/tmp/current/workspace");
        current.api_key = Some("real-key".to_string());
        current.reliability.api_keys = vec!["r1".to_string(), "r2".to_string()];
        current.gateway.paired_tokens = vec!["pair-1".to_string(), "pair-2".to_string()];
        current.tunnel.cloudflare = Some(crate::config::schema::CloudflareTunnelConfig {
            token: "cf-token-real".to_string(),
        });
        current.tunnel.ngrok = Some(crate::config::schema::NgrokTunnelConfig {
            auth_token: "ngrok-token-real".to_string(),
            domain: None,
        });
        current.memory.qdrant.api_key = Some("qdrant-real".to_string());
        current.channels_config.wati = Some(crate::config::schema::WatiConfig {
            api_token: "wati-real".to_string(),
            api_url: "https://live-mt-server.wati.io".to_string(),
            tenant_id: None,
            allowed_numbers: vec![],
            proxy_url: None,
        });
        current.channels_config.feishu = Some(crate::config::schema::FeishuConfig {
            app_id: "cli_current".to_string(),
            app_secret: "feishu-secret-real".to_string(),
            encrypt_key: Some("feishu-encrypt-real".to_string()),
            verification_token: Some("feishu-verify-real".to_string()),
            allowed_users: vec!["*".to_string()],
            receive_mode: crate::config::schema::LarkReceiveMode::Websocket,
            port: None,
            proxy_url: None,
        });
        current.channels_config.email = Some(crate::channels::email_channel::EmailConfig {
            imap_host: "imap.example.com".to_string(),
            imap_port: 993,
            imap_folder: "INBOX".to_string(),
            smtp_host: "smtp.example.com".to_string(),
            smtp_port: 465,
            smtp_tls: true,
            username: "agent@example.com".to_string(),
            password: "email-password-real".to_string(),
            from_address: "agent@example.com".to_string(),
            idle_timeout_secs: 1740,
            allowed_senders: vec!["*".to_string()],
            default_subject: "SenWeaverCoding Message".to_string(),
        });
        current.model_routes = vec![
            crate::config::schema::ModelRouteConfig {
                hint: "reasoning".to_string(),
                provider: "openrouter".to_string(),
                model: "anthropic/claude-sonnet-4.6".to_string(),
                api_key: Some("route-model-key-1".to_string()),
            },
            crate::config::schema::ModelRouteConfig {
                hint: "fast".to_string(),
                provider: "openrouter".to_string(),
                model: "openai/gpt-4.1-mini".to_string(),
                api_key: Some("route-model-key-2".to_string()),
            },
        ];
        current.embedding_routes = vec![
            crate::config::schema::EmbeddingRouteConfig {
                hint: "semantic".to_string(),
                provider: "openai".to_string(),
                model: "text-embedding-3-small".to_string(),
                dimensions: Some(1536),
                api_key: Some("route-embed-key-1".to_string()),
            },
            crate::config::schema::EmbeddingRouteConfig {
                hint: "archive".to_string(),
                provider: "custom:https://emb.example.com/v1".to_string(),
                model: "bge-m3".to_string(),
                dimensions: Some(1024),
                api_key: Some("route-embed-key-2".to_string()),
            },
        ];

        let mut incoming = mask_sensitive_fields(&current);
        incoming.default_model = Some("gpt-4.1-mini".to_string());
        // Simulate UI changing only one key and keeping the first masked.
        incoming.reliability.api_keys = vec![MASKED_SECRET.to_string(), "r2-new".to_string()];
        incoming.gateway.paired_tokens = vec![MASKED_SECRET.to_string(), "pair-2-new".to_string()];
        if let Some(cloudflare) = incoming.tunnel.cloudflare.as_mut() {
            cloudflare.token = MASKED_SECRET.to_string();
        }
        if let Some(ngrok) = incoming.tunnel.ngrok.as_mut() {
            ngrok.auth_token = MASKED_SECRET.to_string();
        }
        incoming.memory.qdrant.api_key = Some(MASKED_SECRET.to_string());
        if let Some(wati) = incoming.channels_config.wati.as_mut() {
            wati.api_token = MASKED_SECRET.to_string();
        }
        if let Some(feishu) = incoming.channels_config.feishu.as_mut() {
            feishu.app_secret = MASKED_SECRET.to_string();
            feishu.encrypt_key = Some(MASKED_SECRET.to_string());
            feishu.verification_token = Some("feishu-verify-new".to_string());
        }
        if let Some(email) = incoming.channels_config.email.as_mut() {
            email.password = MASKED_SECRET.to_string();
        }
        incoming.model_routes[1].api_key = Some("route-model-key-2-new".to_string());
        incoming.embedding_routes[1].api_key = Some("route-embed-key-2-new".to_string());

        let hydrated = hydrate_config_for_save(incoming, &current);

        assert_eq!(hydrated.config_path, current.config_path);
        assert_eq!(hydrated.workspace_dir, current.workspace_dir);
        assert_eq!(hydrated.api_key, current.api_key);
        assert_eq!(hydrated.default_model.as_deref(), Some("gpt-4.1-mini"));
        assert_eq!(
            hydrated.reliability.api_keys,
            vec!["r1".to_string(), "r2-new".to_string()]
        );
        assert_eq!(
            hydrated.gateway.paired_tokens,
            vec!["pair-1".to_string(), "pair-2-new".to_string()]
        );
        assert_eq!(
            hydrated
                .tunnel
                .cloudflare
                .as_ref()
                .map(|v| v.token.as_str()),
            Some("cf-token-real")
        );
        assert_eq!(
            hydrated
                .tunnel
                .ngrok
                .as_ref()
                .map(|v| v.auth_token.as_str()),
            Some("ngrok-token-real")
        );
        assert_eq!(
            hydrated.memory.qdrant.api_key.as_deref(),
            Some("qdrant-real")
        );
        assert_eq!(
            hydrated
                .channels_config
                .wati
                .as_ref()
                .map(|v| v.api_token.as_str()),
            Some("wati-real")
        );
        assert_eq!(
            hydrated
                .channels_config
                .feishu
                .as_ref()
                .map(|v| v.app_secret.as_str()),
            Some("feishu-secret-real")
        );
        assert_eq!(
            hydrated
                .channels_config
                .feishu
                .as_ref()
                .and_then(|v| v.encrypt_key.as_deref()),
            Some("feishu-encrypt-real")
        );
        assert_eq!(
            hydrated
                .channels_config
                .feishu
                .as_ref()
                .and_then(|v| v.verification_token.as_deref()),
            Some("feishu-verify-new")
        );
        assert_eq!(
            hydrated.model_routes[0].api_key.as_deref(),
            Some("route-model-key-1")
        );
        assert_eq!(
            hydrated.model_routes[1].api_key.as_deref(),
            Some("route-model-key-2-new")
        );
        assert_eq!(
            hydrated.embedding_routes[0].api_key.as_deref(),
            Some("route-embed-key-1")
        );
        assert_eq!(
            hydrated.embedding_routes[1].api_key.as_deref(),
            Some("route-embed-key-2-new")
        );
        assert_eq!(
            hydrated
                .channels_config
                .email
                .as_ref()
                .map(|v| v.password.as_str()),
            Some("email-password-real")
        );
    }

    #[test]
    fn hydrate_config_for_save_restores_route_keys_by_identity_and_clears_unmatched_masks() {
        let mut current = crate::config::Config::default();
        current.model_routes = vec![
            crate::config::schema::ModelRouteConfig {
                hint: "reasoning".to_string(),
                provider: "openrouter".to_string(),
                model: "anthropic/claude-sonnet-4.6".to_string(),
                api_key: Some("route-model-key-1".to_string()),
            },
            crate::config::schema::ModelRouteConfig {
                hint: "fast".to_string(),
                provider: "openrouter".to_string(),
                model: "openai/gpt-4.1-mini".to_string(),
                api_key: Some("route-model-key-2".to_string()),
            },
        ];
        current.embedding_routes = vec![
            crate::config::schema::EmbeddingRouteConfig {
                hint: "semantic".to_string(),
                provider: "openai".to_string(),
                model: "text-embedding-3-small".to_string(),
                dimensions: Some(1536),
                api_key: Some("route-embed-key-1".to_string()),
            },
            crate::config::schema::EmbeddingRouteConfig {
                hint: "archive".to_string(),
                provider: "custom:https://emb.example.com/v1".to_string(),
                model: "bge-m3".to_string(),
                dimensions: Some(1024),
                api_key: Some("route-embed-key-2".to_string()),
            },
        ];

        let mut incoming = mask_sensitive_fields(&current);
        incoming.model_routes.swap(0, 1);
        incoming.embedding_routes.swap(0, 1);
        incoming
            .model_routes
            .push(crate::config::schema::ModelRouteConfig {
                hint: "new".to_string(),
                provider: "openai".to_string(),
                model: "gpt-4.1".to_string(),
                api_key: Some(MASKED_SECRET.to_string()),
            });
        incoming
            .embedding_routes
            .push(crate::config::schema::EmbeddingRouteConfig {
                hint: "new-embed".to_string(),
                provider: "custom:https://emb2.example.com/v1".to_string(),
                model: "bge-small".to_string(),
                dimensions: Some(768),
                api_key: Some(MASKED_SECRET.to_string()),
            });

        let hydrated = hydrate_config_for_save(incoming, &current);

        assert_eq!(
            hydrated.model_routes[0].api_key.as_deref(),
            Some("route-model-key-2")
        );
        assert_eq!(
            hydrated.model_routes[1].api_key.as_deref(),
            Some("route-model-key-1")
        );
        assert_eq!(hydrated.model_routes[2].api_key, None);
        assert_eq!(
            hydrated.embedding_routes[0].api_key.as_deref(),
            Some("route-embed-key-2")
        );
        assert_eq!(
            hydrated.embedding_routes[1].api_key.as_deref(),
            Some("route-embed-key-1")
        );
        assert_eq!(hydrated.embedding_routes[2].api_key, None);
        assert!(
            hydrated
                .model_routes
                .iter()
                .all(|route| route.api_key.as_deref() != Some(MASKED_SECRET))
        );
        assert!(
            hydrated
                .embedding_routes
                .iter()
                .all(|route| route.api_key.as_deref() != Some(MASKED_SECRET))
        );
    }

    #[tokio::test]
    async fn cron_api_shell_roundtrip_includes_delivery() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config = crate::config::Config {
            workspace_dir: tmp.path().join("workspace"),
            config_path: tmp.path().join("config.toml"),
            ..crate::config::Config::default()
        };
        std::fs::create_dir_all(&config.workspace_dir).unwrap();
        let state = test_state(config);

        let add_response = handle_api_cron_add(
            State(state.clone()),
            HeaderMap::new(),
            Json(
                serde_json::from_value::<CronAddBody>(serde_json::json!({
                    "name": "test-job",
                    "schedule": "*/5 * * * *",
                    "command": "echo hello",
                    "delivery": {
                        "mode": "announce",
                        "channel": "discord",
                        "to": "1234567890",
                        "best_effort": true
                    }
                }))
                .expect("body should deserialize"),
            ),
        )
        .await
        .into_response();

        let add_json = response_json(add_response).await;
        assert_eq!(add_json["status"], "ok");
        assert_eq!(add_json["job"]["delivery"]["mode"], "announce");
        assert_eq!(add_json["job"]["delivery"]["channel"], "discord");
        assert_eq!(add_json["job"]["delivery"]["to"], "1234567890");

        let list_response = handle_api_cron_list(State(state), HeaderMap::new())
            .await
            .into_response();
        let list_json = response_json(list_response).await;
        let jobs = list_json["jobs"].as_array().expect("jobs array");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0]["delivery"]["mode"], "announce");
        assert_eq!(jobs[0]["delivery"]["channel"], "discord");
        assert_eq!(jobs[0]["delivery"]["to"], "1234567890");
    }

    #[tokio::test]
    async fn cron_api_accepts_agent_jobs() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config = crate::config::Config {
            workspace_dir: tmp.path().join("workspace"),
            config_path: tmp.path().join("config.toml"),
            ..crate::config::Config::default()
        };
        std::fs::create_dir_all(&config.workspace_dir).unwrap();
        let state = test_state(config);

        let response = handle_api_cron_add(
            State(state.clone()),
            HeaderMap::new(),
            Json(
                serde_json::from_value::<CronAddBody>(serde_json::json!({
                    "name": "agent-job",
                    "schedule": "*/5 * * * *",
                    "job_type": "agent",
                    "command": "ignored shell command",
                    "prompt": "summarize the latest logs"
                }))
                .expect("body should deserialize"),
            ),
        )
        .await
        .into_response();

        let json = response_json(response).await;
        assert_eq!(json["status"], "ok");

        let config = state.config.lock().clone();
        let jobs = crate::cron::list_jobs(&config).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].job_type, crate::cron::JobType::Agent);
        assert_eq!(jobs[0].prompt.as_deref(), Some("summarize the latest logs"));
    }

    #[tokio::test]
    async fn cron_api_rejects_announce_delivery_without_target() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config = crate::config::Config {
            workspace_dir: tmp.path().join("workspace"),
            config_path: tmp.path().join("config.toml"),
            ..crate::config::Config::default()
        };
        std::fs::create_dir_all(&config.workspace_dir).unwrap();
        let state = test_state(config);

        let response = handle_api_cron_add(
            State(state.clone()),
            HeaderMap::new(),
            Json(
                serde_json::from_value::<CronAddBody>(serde_json::json!({
                    "name": "invalid-delivery-job",
                    "schedule": "*/5 * * * *",
                    "command": "echo hello",
                    "delivery": {
                        "mode": "announce",
                        "channel": "discord"
                    }
                }))
                .expect("body should deserialize"),
            ),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = response_json(response).await;
        assert!(
            json["error"]
                .as_str()
                .unwrap_or_default()
                .contains("delivery.to is required")
        );

        let config = state.config.lock().clone();
        assert!(crate::cron::list_jobs(&config).unwrap().is_empty());
    }

    #[tokio::test]
    async fn cron_api_rejects_announce_delivery_with_unsupported_channel() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config = crate::config::Config {
            workspace_dir: tmp.path().join("workspace"),
            config_path: tmp.path().join("config.toml"),
            ..crate::config::Config::default()
        };
        std::fs::create_dir_all(&config.workspace_dir).unwrap();
        let state = test_state(config);

        let response = handle_api_cron_add(
            State(state.clone()),
            HeaderMap::new(),
            Json(
                serde_json::from_value::<CronAddBody>(serde_json::json!({
                    "name": "invalid-delivery-job",
                    "schedule": "*/5 * * * *",
                    "command": "echo hello",
                    "delivery": {
                        "mode": "announce",
                        "channel": "email",
                        "to": "alerts@example.com"
                    }
                }))
                .expect("body should deserialize"),
            ),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = response_json(response).await;
        assert!(
            json["error"]
                .as_str()
                .unwrap_or_default()
                .contains("unsupported delivery channel")
        );

        let config = state.config.lock().clone();
        assert!(crate::cron::list_jobs(&config).unwrap().is_empty());
    }
}
