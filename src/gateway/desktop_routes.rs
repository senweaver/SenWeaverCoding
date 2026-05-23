// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        sse::{Event as SseEvent, KeepAlive, Sse},
        IntoResponse, Json,
    },
};
use futures_util::stream::{Stream, StreamExt};
use serde::Deserialize;
use std::convert::Infallible;
use std::path::PathBuf;
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;

use super::api::require_auth;

#[derive(Debug, Deserialize, Default)]
pub struct ModelsListQuery {
    #[serde(rename = "providerId")]
    pub provider_id: Option<String>,
}

pub async fn handle_models_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ModelsListQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock().clone();

    let resolved_provider_id: Option<String> = q
        .provider_id
        .as_deref()
        .filter(|id| config.model_providers.contains_key(*id))
        .map(str::to_string)
        .or_else(|| {
            config
                .default_provider
                .as_deref()
                .filter(|id| config.model_providers.contains_key(*id))
                .map(str::to_string)
        })
        .or_else(|| config.model_providers.keys().next().cloned());

    let mut models: Vec<serde_json::Value> = Vec::new();
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let active_model = config.default_model.clone().unwrap_or_default();

    if let Some(provider_id) = resolved_provider_id.as_deref() {
        if let Some(profile) = config.model_providers.get(provider_id) {
            for mid in effective_model_names(profile) {
                if !seen.insert(mid.clone()) {
                    continue;
                }
                models.push(serde_json::json!({
                    "id": mid,
                    "name": mid,
                    "active": active_model == mid,
                }));
            }
        }
    }

    let provider_payload = match resolved_provider_id.as_deref() {
        Some(id) => serde_json::json!({
            "id": id,
            "name": pretty_provider_name(id),
        }),
        None => serde_json::Value::Null,
    };

    Json(serde_json::json!({
        "models": models,
        "provider": provider_payload,
    }))
    .into_response()
}

fn pretty_provider_name(id: &str) -> String {
    match id {
        "openrouter" => "OpenRouter".to_string(),
        "anthropic" => "Anthropic".to_string(),
        "openai" => "OpenAI".to_string(),
        "deepseek" => "DeepSeek".to_string(),
        "google" | "gemini" => "Google".to_string(),
        other => other.to_string(),
    }
}

pub async fn handle_models_available(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    let default_provider_id = config
        .default_provider
        .as_deref()
        .filter(|id| config.model_providers.contains_key(*id))
        .map(str::to_string);
    let mut provider_ids: Vec<&String> = config.model_providers.keys().collect();
    provider_ids.sort();
    let mut total: u32 = 0;
    let mut providers_with_models: u32 = 0;
    let mut entries: Vec<serde_json::Value> = Vec::new();
    let mut providers_payload: Vec<serde_json::Value> = Vec::new();
    for pid in provider_ids {
        let Some(profile) = config.model_providers.get(pid) else {
            continue;
        };
        let names = effective_model_names(profile);
        if !names.is_empty() {
            providers_with_models = providers_with_models.saturating_add(1);
        }
        let display_name = profile
            .name
            .clone()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| pretty_provider_name(pid));
        let is_default = default_provider_id.as_deref() == Some(pid.as_str());
        let mut models_for_provider: Vec<serde_json::Value> = Vec::new();
        for name in &names {
            total = total.saturating_add(1);
            entries.push(serde_json::json!({
                "id": name,
                "providerId": pid,
                "providerName": display_name,
                "isDefaultProvider": is_default,
            }));
            models_for_provider.push(serde_json::json!({
                "id": name,
                "name": name,
            }));
        }
        providers_payload.push(serde_json::json!({
            "id": pid,
            "name": display_name,
            "isDefault": is_default,
            "models": models_for_provider,
        }));
    }
    Json(serde_json::json!({
        "models": entries,
        "providers": providers_payload,
        "total": total,
        "providersConfigured": config.model_providers.len(),
        "providersWithModels": providers_with_models,
        "defaultProviderId": default_provider_id,
    }))
    .into_response()
}

pub async fn handle_models_current(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();

    let active_provider_id: Option<String> = config
        .default_provider
        .as_deref()
        .filter(|id| config.model_providers.contains_key(*id))
        .map(str::to_string)
        .or_else(|| config.model_providers.keys().next().cloned());

    let active_models: Vec<String> = active_provider_id
        .as_deref()
        .and_then(|pid| config.model_providers.get(pid))
        .map(effective_model_names)
        .unwrap_or_default();

    if active_models.is_empty() {
        return Json(serde_json::json!({ "model": null })).into_response();
    }

    let resolved = config
        .default_model
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .filter(|m| active_models.iter().any(|x| x == m))
        .map(str::to_string)
        .or_else(|| active_models.into_iter().next());

    match resolved {
        Some(id) => Json(serde_json::json!({
            "model": { "id": id.clone(), "name": id, "active": true },
        }))
        .into_response(),
        None => Json(serde_json::json!({ "model": null })).into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct SetCurrentModelBody {
    #[serde(rename = "modelId")]
    pub model_id: String,
}

pub async fn handle_models_set_current(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SetCurrentModelBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let snapshot = {
        let mut cfg = state.config.lock();
        cfg.default_model = Some(body.model_id.clone());
        cfg.clone()
    };
    if let Err(e) = snapshot.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "config_save_failed",
                "detail": format!("{e:#}"),
            })),
        )
            .into_response();
    }
    state.push_live_config(snapshot);
    state.rebuild_runtime_from_config();
    Json(serde_json::json!({ "ok": true, "model": body.model_id })).into_response()
}

pub async fn handle_effort_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    let level = config
        .runtime
        .reasoning_effort
        .clone()
        .unwrap_or_else(|| "medium".to_string());
    Json(serde_json::json!({
        "level": level,
        "available": ["low", "medium", "high"],
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
pub struct SetEffortBody {
    pub level: String,
}

pub async fn handle_effort_set(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SetEffortBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let snapshot = {
        let mut cfg = state.config.lock();
        cfg.runtime.reasoning_effort = Some(body.level.clone());
        cfg.clone()
    };
    if let Err(e) = snapshot.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "config_save_failed",
                "detail": format!("{e:#}"),
            })),
        )
            .into_response();
    }
    state.push_live_config(snapshot);
    state.rebuild_runtime_from_config();
    Json(serde_json::json!({ "ok": true, "level": body.level })).into_response()
}

fn provider_has_key(id: &str, config: &crate::config::Config) -> bool {
    if let Some(profile) = config.model_providers.get(id) {
        if profile.api_key.as_deref().map(str::trim).is_some_and(|s: &str| !s.is_empty()) {
            return true;
        }
        if profile.requires_openai_auth && std::env::var("OPENAI_API_KEY").is_ok() {
            return true;
        }
    }
    if config.api_key.is_some() {
        return true;
    }
    let env_var = match id.to_ascii_lowercase().as_str() {
        "openai" | "openai-codex" => "OPENAI_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        "deepseek" => "DEEPSEEK_API_KEY",
        "gemini" | "google" => "GEMINI_API_KEY",
        "groq" => "GROQ_API_KEY",
        "mistral" => "MISTRAL_API_KEY",
        "together" => "TOGETHER_API_KEY",
        "xai" | "grok" => "XAI_API_KEY",
        "moonshot" => "MOONSHOT_API_KEY",
        "zhipu" | "glm" => "ZHIPU_API_KEY",
        _ => return false,
    };
    std::env::var(env_var).is_ok()
}

fn resolve_provider_api_key(id: &str, config: &crate::config::Config) -> Option<String> {
    if let Some(profile) = config.model_providers.get(id) {
        if let Some(key) = profile
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(key.to_string());
        }
    }
    if let Some(key) = config
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(key.to_string());
    }
    let env_var = match id.to_ascii_lowercase().as_str() {
        "openai" | "openai-codex" => "OPENAI_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        "deepseek" => "DEEPSEEK_API_KEY",
        "gemini" | "google" => "GEMINI_API_KEY",
        "groq" => "GROQ_API_KEY",
        "mistral" => "MISTRAL_API_KEY",
        "together" => "TOGETHER_API_KEY",
        "xai" | "grok" => "XAI_API_KEY",
        "moonshot" => "MOONSHOT_API_KEY",
        "zhipu" | "glm" => "ZHIPU_API_KEY",
        _ => return None,
    };
    std::env::var(env_var).ok().filter(|s| !s.is_empty())
}

fn api_format_to_wire(format: &str) -> &'static str {
    match format {
        "anthropic" => "anthropic",
        "openai_responses" => "responses",

        _ => "chat_completions",
    }
}

fn wire_to_api_format(wire: Option<&str>) -> &'static str {
    match wire {
        Some("anthropic") => "anthropic",
        Some("responses") => "openai_responses",
        _ => "openai_chat",
    }
}

fn normalize_display_name_key(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut prev_space = false;
    for ch in value.trim().chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
            prev_space = false;
        }
    }
    out
}

pub(crate) fn find_provider_id_by_display_name(
    cfg: &crate::config::Config,
    name: &str,
    skip_id: Option<&str>,
) -> Option<String> {
    let needle = normalize_display_name_key(name);
    if needle.is_empty() {
        return None;
    }
    cfg.model_providers
        .iter()
        .filter(|(id, _)| skip_id.map(|skip| skip != id.as_str()).unwrap_or(true))
        .find_map(|(id, profile)| {
            let candidate = profile
                .name
                .as_deref()
                .map(normalize_display_name_key)
                .unwrap_or_else(|| normalize_display_name_key(id));
            (candidate == needle).then(|| id.clone())
        })
}

fn slugify_provider_id(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out = format!(
            "provider-{}",
            uuid::Uuid::new_v4().simple().to_string().get(..8).unwrap_or("custom")
        );
    }
    out
}

fn provider_to_saved_provider(
    id: &str,
    profile: &crate::config::ModelProviderConfig,
    config: &crate::config::Config,
) -> serde_json::Value {
    let api_key_present = profile
        .api_key
        .as_deref()
        .map(str::trim)
        .is_some_and(|s: &str| !s.is_empty());
    let env_present = !api_key_present && provider_has_key(id, config);
    let masked = if api_key_present {
        let raw = profile.api_key.clone().unwrap_or_default();
        mask_api_key(&raw)
    } else if env_present {
        "<env>".to_string()
    } else {
        String::new()
    };
    let models = effective_model_names(profile);
    let model_context_windows: serde_json::Map<String, serde_json::Value> = profile
        .model_context_windows
        .iter()
        .filter(|(_, value)| **value > 0)
        .map(|(model, value)| (model.clone(), serde_json::Value::from(*value)))
        .collect();
    let custom_headers: Vec<serde_json::Value> = profile
        .custom_headers
        .iter()
        .map(|header| {
            serde_json::json!({
                "name": header.name,
                "value": header.value,
                "enabled": header.enabled,
            })
        })
        .collect();
    serde_json::json!({
        "id": id,
        "presetId": profile.preset_id.clone().unwrap_or_else(|| id.to_string()),
        "name": profile.name.clone().unwrap_or_else(|| id.to_string()),
        "apiKey": masked,
        "baseUrl": profile.base_url.clone().unwrap_or_default(),
        "apiFormat": wire_to_api_format(profile.wire_api.as_deref()),
        "models": models,
        "modelContextWindows": serde_json::Value::Object(model_context_windows),
        "customHeaders": custom_headers,
        "notes": profile.notes.clone().unwrap_or_default(),
        "hasKey": api_key_present || env_present,
    })
}

fn effective_model_names(profile: &crate::config::ModelProviderConfig) -> Vec<String> {
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
    for (_, value) in profile.models.iter() {
        let trimmed = value.trim();
        if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
            out.push(trimmed.to_string());
        }
    }
    out
}

fn mask_api_key(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.len() <= 8 {
        return "*".repeat(trimmed.len());
    }
    let head = &trimmed[..4];
    let tail = &trimmed[trimmed.len() - 4..];
    format!("{head}…{tail}")
}

#[derive(Debug, Deserialize)]
pub struct CreateProviderBody {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub preset_id: Option<String>,
    #[serde(default, rename = "presetId")]
    pub preset_id_camel: Option<String>,
    pub name: String,
    #[serde(default, rename = "apiKey")]
    pub api_key: Option<String>,
    #[serde(default, rename = "baseUrl")]
    pub base_url: Option<String>,
    #[serde(default, rename = "apiFormat")]
    pub api_format: Option<String>,

    #[serde(default)]
    pub models: Option<serde_json::Value>,

    #[serde(default, rename = "modelContextWindows")]
    pub model_context_windows: Option<serde_json::Value>,
    #[serde(default, rename = "customHeaders")]
    pub custom_headers: Option<Vec<CustomHeaderInput>>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProviderBody {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, rename = "apiKey")]
    pub api_key: Option<String>,
    #[serde(default, rename = "baseUrl")]
    pub base_url: Option<String>,
    #[serde(default, rename = "apiFormat")]
    pub api_format: Option<String>,
    #[serde(default)]
    pub models: Option<serde_json::Value>,
    #[serde(default, rename = "modelContextWindows")]
    pub model_context_windows: Option<serde_json::Value>,
    #[serde(default, rename = "customHeaders")]
    pub custom_headers: Option<Vec<CustomHeaderInput>>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CustomHeaderInput {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub value: String,
    #[serde(default = "default_custom_header_input_enabled")]
    pub enabled: bool,
}

fn default_custom_header_input_enabled() -> bool {
    true
}

fn sanitize_custom_headers_input(
    input: &[CustomHeaderInput],
) -> Vec<crate::config::CustomHttpHeader> {
    let mut out: Vec<crate::config::CustomHttpHeader> = Vec::with_capacity(input.len());
    let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for entry in input {
        let trimmed_name = entry.name.trim().to_string();
        if trimmed_name.is_empty() {
            continue;
        }
        let dedupe_key = trimmed_name.to_ascii_lowercase();
        if !seen_names.insert(dedupe_key) {
            tracing::warn!(
                header_name = trimmed_name.as_str(),
                "custom_headers contains duplicate name; keeping the first entry only"
            );
            continue;
        }
        out.push(crate::config::CustomHttpHeader {
            name: trimmed_name,
            value: entry.value.clone(),
            enabled: entry.enabled,
        });
    }
    out
}

fn parse_model_names(value: &serde_json::Value) -> Vec<String> {
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut out: Vec<String> = Vec::new();
    let mut push = |raw: &str| {
        let trimmed = raw.trim();
        if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
            out.push(trimmed.to_string());
        }
    };
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                if let Some(s) = item.as_str() {
                    push(s);
                }
            }
        }
        serde_json::Value::Object(map) => {
            for slot in ["main", "haiku", "sonnet", "opus"] {
                if let Some(s) = map.get(slot).and_then(|v| v.as_str()) {
                    push(s);
                }
            }
            for (_, v) in map {
                if let Some(s) = v.as_str() {
                    push(s);
                }
            }
        }
        _ => {}
    }
    out
}

fn apply_models_to_profile(
    profile: &mut crate::config::ModelProviderConfig,
    models: &serde_json::Value,
) {
    profile.model_names = parse_model_names(models);
    profile.models.clear();
}

fn parse_model_context_windows(
    value: &serde_json::Value,
) -> std::collections::HashMap<String, u32> {
    let mut out = std::collections::HashMap::new();
    let serde_json::Value::Object(map) = value else {
        return out;
    };
    for (key, raw) in map {
        let trimmed = key.trim();
        if trimmed.is_empty() {
            continue;
        }
        let limit = match raw {
            serde_json::Value::Number(n) => n.as_u64(),
            serde_json::Value::String(s) => s.trim().parse::<u64>().ok(),
            _ => None,
        };
        let Some(limit) = limit else { continue };
        if limit == 0 || limit > u64::from(u32::MAX) {
            continue;
        }
        out.insert(trimmed.to_string(), limit as u32);
    }
    out
}

fn apply_model_context_windows_to_profile(
    profile: &mut crate::config::ModelProviderConfig,
    payload: &serde_json::Value,
) {
    let parsed = parse_model_context_windows(payload);
    profile.model_context_windows = parsed
        .into_iter()
        .filter(|(model, _)| profile.model_names.iter().any(|m| m == model))
        .collect();
}

pub async fn handle_providers_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();

    let active_id = config
        .default_provider
        .as_deref()
        .filter(|id| config.model_providers.contains_key(*id))
        .map(str::to_string);
    let providers: Vec<serde_json::Value> = config
        .model_providers
        .iter()
        .map(|(id, p)| provider_to_saved_provider(id, p, &config))
        .collect();
    Json(serde_json::json!({
        "providers": providers,
        "activeId": active_id,
    }))
    .into_response()
}

pub async fn handle_providers_presets(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    fn preset(
        id: &str,
        name: &str,
        base_url: &str,
        api_format: &str,
        models: &[&str],
        website: &str,
        needs_key: bool,
    ) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "name": name,
            "baseUrl": base_url,
            "apiFormat": api_format,
            "defaultModels": models,
            "needsApiKey": needs_key,
            "websiteUrl": website,
        })
    }

    let presets = serde_json::json!([
        preset("custom", "Custom (OpenAI-compatible)", "", "openai_chat",
            &[],
            "",
            true),
        preset("anthropic", "Anthropic", "https://api.anthropic.com", "anthropic",
            &[],
            "https://www.anthropic.com",
            true),
        preset("openai", "OpenAI", "https://api.openai.com/v1", "openai_chat",
            &[],
            "https://platform.openai.com",
            true),
        preset("openai-codex", "OpenAI Responses (Codex)", "https://api.openai.com/v1", "openai_responses",
            &[],
            "https://platform.openai.com",
            true),
        preset("openrouter", "OpenRouter", "https://openrouter.ai/api/v1", "openai_chat",
            &[],
            "https://openrouter.ai",
            true),
        preset("deepseek", "DeepSeek", "https://api.deepseek.com", "openai_chat",
            &[],
            "https://platform.deepseek.com",
            true),
        preset("gemini", "Google Gemini", "https://generativelanguage.googleapis.com/v1beta/openai", "openai_chat",
            &[],
            "https://aistudio.google.com",
            true),
        preset("groq", "Groq", "https://api.groq.com/openai/v1", "openai_chat",
            &[],
            "https://console.groq.com",
            true),
        preset("together", "Together AI", "https://api.together.xyz/v1", "openai_chat",
            &[],
            "https://www.together.ai",
            true),
        preset("mistral", "Mistral", "https://api.mistral.ai/v1", "openai_chat",
            &[],
            "https://console.mistral.ai",
            true),
        preset("moonshot", "Moonshot (月之暗面)", "https://api.moonshot.cn/v1", "openai_chat",
            &["kimi-k2.6"],
            "https://platform.moonshot.cn",
            true),
        preset("zhipu", "智谱 GLM", "https://open.bigmodel.cn/api/paas/v4", "openai_chat",
            &[],
            "https://open.bigmodel.cn",
            true),
    ]);
    Json(serde_json::json!({ "presets": presets })).into_response()
}

pub async fn handle_providers_auth_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    let has_global_key = config.api_key.is_some();
    let active_provider = config.default_provider.clone();
    let has_provider_key = active_provider
        .as_ref()
        .map(|id| provider_has_key(id, &config))
        .unwrap_or(false);

    let (has_auth, source) = if has_provider_key {
        (true, "sen-provider")
    } else if has_global_key {
        (true, "original-settings")
    } else if std::env::var("ANTHROPIC_API_KEY").is_ok()
        || std::env::var("OPENAI_API_KEY").is_ok()
        || std::env::var("OPENROUTER_API_KEY").is_ok()
    {
        (true, "env")
    } else {
        (false, "none")
    };

    Json(serde_json::json!({
        "hasAuth": has_auth,
        "source": source,
        "activeProvider": active_provider,
    }))
    .into_response()
}

pub async fn handle_providers_settings_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let path = desktop_user_settings_path(&state);
    let parsed = tokio::task::spawn_blocking(move || {
        let body = std::fs::read_to_string(&path).unwrap_or_else(|_| "{}".to_string());
        serde_json::from_str::<serde_json::Value>(&body).unwrap_or_else(|_| serde_json::json!({}))
    })
    .await
    .unwrap_or_else(|_| serde_json::json!({}));
    Json(parsed).into_response()
}

pub async fn handle_providers_settings_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let path = desktop_user_settings_path(&state);
    let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let serialized = serde_json::to_string_pretty(&body)
            .unwrap_or_else(|_| body.to_string());
        std::fs::write(&path, serialized).map_err(|e| format!("write settings: {e}"))
    })
    .await;
    match result {
        Ok(Ok(())) => Json(serde_json::json!({ "ok": true })).into_response(),
        Ok(Err(msg)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "blocking task join failed" })),
        )
            .into_response(),
    }
}

pub async fn handle_providers_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateProviderBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let trimmed_name = body.name.trim().to_string();
    if trimmed_name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "name is required"})),
        )
            .into_response();
    }

    let preset_id = body
        .preset_id_camel
        .clone()
        .or(body.preset_id.clone())
        .unwrap_or_else(|| slugify_provider_id(&trimmed_name));
    let mut id = body
        .id
        .clone()
        .filter(|raw| !raw.trim().is_empty())
        .unwrap_or_else(|| slugify_provider_id(&trimmed_name));
    {
        let cfg = state.config.lock();
        if let Some(existing_id) =
            find_provider_id_by_display_name(&cfg, &trimmed_name, None)
        {
            tracing::info!(
                requested_name = trimmed_name.as_str(),
                existing_id = existing_id.as_str(),
                "Provider create: display name reused; allowing per spec (name is for display only, id is the unique key)"
            );
        }
        if cfg.model_providers.contains_key(&id) {
            let mut suffix: u64 = 2;
            loop {
                let candidate = format!("{id}-{suffix}");
                if !cfg.model_providers.contains_key(&candidate) {
                    id = candidate;
                    break;
                }
                suffix = suffix.saturating_add(1);
                if suffix > 1_000_000 {

                    id = format!(
                        "{id}-{}",
                        uuid::Uuid::new_v4().simple().to_string().get(..12).unwrap_or("collision")
                    );
                    break;
                }
            }
        }
    }

    let api_format = body.api_format.as_deref().unwrap_or("openai_chat");
    let wire_api = api_format_to_wire(api_format).to_string();

    let mut profile = crate::config::ModelProviderConfig {
        name: Some(trimmed_name.clone()),
        base_url: body
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
        wire_api: Some(wire_api),
        preset_id: Some(preset_id.clone()),
        notes: body
            .notes
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
        api_key: body
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
        ..crate::config::ModelProviderConfig::default()
    };
    if preset_id == "openai-codex" {
        profile.requires_openai_auth = true;
    }
    if let Some(ref models) = body.models {
        apply_models_to_profile(&mut profile, models);
    }
    if let Some(ref overrides) = body.model_context_windows {
        apply_model_context_windows_to_profile(&mut profile, overrides);
    }
    if let Some(ref custom_headers) = body.custom_headers {
        profile.custom_headers = sanitize_custom_headers_input(custom_headers);
    }

    let snapshot;
    {
        let mut cfg = state.config.lock();
        cfg.model_providers.insert(id.clone(), profile.clone());
        snapshot = cfg.clone();
    }

    if let Err(e) = snapshot.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("save config: {e}")})),
        )
            .into_response();
    }
    state.push_live_config(snapshot);
    state.rebuild_runtime_from_config();

    let config_snapshot = state.config.lock().clone();
    let saved = provider_to_saved_provider(&id, &profile, &config_snapshot);
    Json(serde_json::json!({ "provider": saved })).into_response()
}

pub async fn handle_providers_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<UpdateProviderBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    if let Some(name) = body.name.as_deref().map(str::trim) {
        if !name.is_empty() {
            let cfg = state.config.lock();
            if let Some(existing_id) =
                find_provider_id_by_display_name(&cfg, name, Some(id.as_str()))
            {
                tracing::info!(
                    requested_name = name,
                    existing_id = existing_id.as_str(),
                    target_id = id.as_str(),
                    "Provider update: display name shared with existing provider; allowing per spec (name is display-only, id is unique key)"
                );
            }
        }
    }

    let snapshot = {
        let mut cfg = state.config.lock();
        let updated_profile = {
            let Some(profile) = cfg.model_providers.get_mut(&id) else {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": format!("provider not found: {id}")})),
                )
                    .into_response();
            };

            if let Some(name) = body.name.as_deref().map(str::trim) {
                if !name.is_empty() {
                    profile.name = Some(name.to_string());
                }
            }
            if let Some(base_url) = body.base_url.as_deref().map(str::trim) {
                profile.base_url = if base_url.is_empty() {
                    None
                } else {
                    Some(base_url.to_string())
                };
            }
            if let Some(api_format) = body.api_format.as_deref() {
                profile.wire_api = Some(api_format_to_wire(api_format).to_string());
                profile.requires_openai_auth = api_format == "openai_responses";
            }
            if let Some(notes) = body.notes.as_deref().map(str::trim) {
                profile.notes = if notes.is_empty() {
                    None
                } else {
                    Some(notes.to_string())
                };
            }
            if let Some(api_key) = body.api_key.as_deref().map(str::trim) {
                profile.api_key = if api_key.is_empty() {
                    None
                } else {
                    Some(api_key.to_string())
                };
            }
            if let Some(ref models) = body.models {
                apply_models_to_profile(profile, models);
            }
            if let Some(ref overrides) = body.model_context_windows {
                apply_model_context_windows_to_profile(profile, overrides);
            } else if body.models.is_some() {

                let kept: std::collections::HashSet<String> =
                    profile.model_names.iter().cloned().collect();
                profile
                    .model_context_windows
                    .retain(|model, _| kept.contains(model));
            }
            if let Some(ref custom_headers) = body.custom_headers {
                profile.custom_headers = sanitize_custom_headers_input(custom_headers);
            }
            profile.clone()
        };

        if cfg.default_provider.as_deref() == Some(id.as_str()) {
            apply_active_profile_to_top_level(&mut cfg, &id, &updated_profile);
        }
        cfg.clone()
    };

    if let Err(e) = snapshot.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("save config: {e}")})),
        )
            .into_response();
    }
    state.push_live_config(snapshot);
    state.rebuild_runtime_from_config();

    let config_snapshot = state.config.lock().clone();
    let profile = config_snapshot
        .model_providers
        .get(&id)
        .cloned()
        .unwrap_or_default();
    let saved = provider_to_saved_provider(&id, &profile, &config_snapshot);
    Json(serde_json::json!({ "provider": saved })).into_response()
}

pub async fn handle_providers_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let snapshot = {
        let mut cfg = state.config.lock();
        if cfg.model_providers.remove(&id).is_none() {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": format!("provider not found: {id}")})),
            )
                .into_response();
        }
        let was_active = cfg.default_provider.as_deref() == Some(id.as_str());
        if was_active {
            cfg.default_provider = None;
            cfg.default_model = None;
            cfg.api_key = None;
            cfg.api_url = None;
            cfg.api_path = None;
            cfg.provider_max_tokens = None;
            cfg.model_context_windows.clear();
        }

        sanitize_active_profile_in_place(&mut cfg);
        cfg.clone()
    };
    if let Err(e) = snapshot.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("save config: {e}")})),
        )
            .into_response();
    }
    state.push_live_config(snapshot);
    state.rebuild_runtime_from_config();
    Json(serde_json::json!({ "ok": true })).into_response()
}

pub async fn handle_providers_activate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let snapshot = {
        let mut cfg = state.config.lock();
        let Some(profile) = cfg.model_providers.get(&id).cloned() else {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": format!("provider not found: {id}")})),
            )
                .into_response();
        };
        apply_active_profile_to_top_level(&mut cfg, &id, &profile);
        cfg.clone()
    };
    if let Err(e) = snapshot.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("save config: {e}")})),
        )
            .into_response();
    }
    state.push_live_config(snapshot);
    state.rebuild_runtime_from_config();
    Json(serde_json::json!({ "ok": true })).into_response()
}

pub(crate) fn resolve_active_profile_id(cfg: &crate::config::Config) -> Option<String> {
    cfg.default_provider
        .as_deref()
        .filter(|id| cfg.model_providers.contains_key(*id))
        .map(str::to_string)
        .or_else(|| cfg.model_providers.keys().next().cloned())
}

pub(crate) fn sanitize_active_profile_in_place(cfg: &mut crate::config::Config) -> bool {
    let resolved = resolve_active_profile_id(cfg);
    let mut mutated = false;

    match resolved {
        Some(id) => {

            let was_ghost = cfg
                .default_provider
                .as_deref()
                .map(|persisted| persisted != id)
                .unwrap_or(true);
            if was_ghost {
                if let Some(profile) = cfg.model_providers.get(&id).cloned() {
                    apply_active_profile_to_top_level(cfg, &id, &profile);
                    mutated = true;
                }
            } else if cfg
                .default_model
                .as_deref()
                .map(|m| {
                    cfg.model_providers
                        .get(&id)
                        .map(effective_model_names)
                        .map(|models| !models.iter().any(|x| x == m))
                        .unwrap_or(false)
                })
                .unwrap_or(false)
            {

                if let Some(profile) = cfg.model_providers.get(&id).cloned() {
                    let models = effective_model_names(&profile);
                    if let Some(first) = models.into_iter().next() {
                        cfg.default_model = Some(first);
                        mutated = true;
                    }
                }
            }
        }
        None => {

            if cfg.default_provider.is_some() {
                cfg.default_provider = None;
                mutated = true;
            }
            if cfg.default_model.is_some() {
                cfg.default_model = None;
                mutated = true;
            }
        }
    }

    mutated
}

pub(crate) fn apply_active_profile_to_top_level(
    cfg: &mut crate::config::Config,
    id: &str,
    profile: &crate::config::ModelProviderConfig,
) {
    cfg.default_provider = Some(id.to_string());

    cfg.api_key = profile
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    cfg.api_url = profile
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    cfg.api_path = profile
        .api_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    cfg.provider_max_tokens = profile.max_tokens;

    cfg.model_context_windows = profile.model_context_windows.clone();

    let models = effective_model_names(profile);
    let current_model_belongs = cfg
        .default_model
        .as_deref()
        .map(|m| models.iter().any(|x| x == m))
        .unwrap_or(false);
    if !current_model_belongs {
        if let Some(first) = models.into_iter().next() {
            cfg.default_model = Some(first);
        } else {
            cfg.default_model = None;
        }
    }
}

pub async fn handle_providers_official(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    (
        StatusCode::GONE,
        Json(serde_json::json!({
            "ok": false,
            "error": "no built-in default provider; add one via the Add Provider dialog",
        })),
    )
        .into_response()
}

async fn probe_provider(
    base_url: &str,
    api_key: Option<&str>,
    api_format: &str,
) -> serde_json::Value {
    let started = std::time::Instant::now();
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return serde_json::json!({
            "success": false,
            "latencyMs": started.elapsed().as_millis() as u64,
            "error": "missing baseUrl",
        });
    }
    let url = match api_format {
        "anthropic" => format!("{trimmed}/v1/models"),

        _ => format!("{trimmed}/models"),
    };

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return serde_json::json!({
                "success": false,
                "latencyMs": started.elapsed().as_millis() as u64,
                "error": format!("client build failed: {e}"),
            });
        }
    };

    let mut req = client.get(&url);
    if let Some(key) = api_key.map(str::trim).filter(|s| !s.is_empty()) {
        match api_format {
            "anthropic" => {
                req = req.header("x-api-key", key);
                req = req.header("anthropic-version", "2023-06-01");
            }
            _ => {
                req = req.header("Authorization", format!("Bearer {key}"));
            }
        }
    }

    match req.send().await {
        Ok(resp) => {
            let status = resp.status();
            let latency_ms = started.elapsed().as_millis() as u64;
            let success = status.is_success();
            let body_snippet = resp.text().await.unwrap_or_default();
            let model_used = if success {
                serde_json::from_str::<serde_json::Value>(&body_snippet)
                    .ok()
                    .and_then(|v| {
                        v.get("data")
                            .and_then(|d| d.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|m| m.get("id"))
                            .and_then(|id| id.as_str())
                            .map(|s| s.to_string())
                    })
            } else {
                None
            };
            let error = if !success {
                let snippet: String = body_snippet.chars().take(200).collect();
                Some(format!("HTTP {status}: {snippet}"))
            } else {
                None
            };
            serde_json::json!({
                "success": success,
                "latencyMs": latency_ms,
                "httpStatus": status.as_u16(),
                "modelUsed": model_used,
                "error": error,
            })
        }
        Err(e) => serde_json::json!({
            "success": false,
            "latencyMs": started.elapsed().as_millis() as u64,
            "error": format!("network error: {e}"),
        }),
    }
}

pub async fn handle_providers_test(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    let Some(profile) = config.model_providers.get(&id).cloned() else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("provider not found: {id}")})),
        )
            .into_response();
    };

    let body: serde_json::Value = if body.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null)
    };

    let base_url = body
        .get("baseUrl")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or(profile.base_url.clone())
        .unwrap_or_default();
    let api_format = body
        .get("apiFormat")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| wire_to_api_format(profile.wire_api.as_deref()).to_string());
    let api_key = resolve_provider_api_key(&id, &config);

    let connectivity = probe_provider(&base_url, api_key.as_deref(), &api_format).await;

    let mut result = serde_json::json!({ "connectivity": connectivity });
    if api_format.starts_with("openai") {

        let proxy = probe_provider(&base_url, api_key.as_deref(), &api_format).await;
        if let Some(obj) = result.as_object_mut() {
            obj.insert("proxy".to_string(), proxy);
        } else {
            tracing::warn!(
                "providers/test: probe result is not a JSON object; skipping proxy field"
            );
        }
    }

    Json(serde_json::json!({ "result": result })).into_response()
}

pub async fn handle_providers_test_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let _ = &state;
    let base_url = body
        .get("baseUrl")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let api_key = body
        .get("apiKey")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let api_format = body
        .get("apiFormat")
        .and_then(|v| v.as_str())
        .unwrap_or("openai_chat")
        .to_string();

    let connectivity = probe_provider(&base_url, api_key.as_deref(), &api_format).await;
    let mut result = serde_json::json!({ "connectivity": connectivity });
    if api_format.starts_with("openai") {
        let proxy = probe_provider(&base_url, api_key.as_deref(), &api_format).await;
        if let Some(obj) = result.as_object_mut() {
            obj.insert("proxy".to_string(), proxy);
        } else {
            tracing::warn!(
                "providers/test-config: probe result is not a JSON object; skipping proxy field"
            );
        }
    }
    Json(serde_json::json!({ "result": result })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct SkillsListQuery {
    pub cwd: Option<String>,
}

pub async fn handle_skills_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<SkillsListQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    let cwd = q
        .cwd
        .map(PathBuf::from)
        .unwrap_or_else(|| config.workspace_dir.clone());
    let skills = crate::skills::discover_skills(&cwd, &config);
    let disabled: std::collections::HashSet<String> = config
        .skills
        .disabled_skills
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let workspace_skills_dir = cwd.join("skills");
    let user_skills_dir = user_senweaver_subdir("skills");
    let payload: Vec<serde_json::Value> = skills
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
                "source": path_str.clone(),
                "path": path_str,
                "enabled": enabled,
                "always_apply": s.always_apply,
                "tier": if s.always_apply { "always" } else { "on_demand" },
            })
        })
        .collect();
    let prompt_mode = match config.skills.prompt_injection_mode {
        crate::config::SkillsPromptInjectionMode::Full => "full",
        crate::config::SkillsPromptInjectionMode::Compact => "compact",
    };
    Json(serde_json::json!({
        "workspace_skills_dir": workspace_skills_dir.display().to_string(),
        "user_skills_dir": user_skills_dir.as_ref().map(|p| p.display().to_string()),
        "open_skills_enabled": config.skills.open_skills_enabled,
        "allow_scripts": config.skills.allow_scripts,
        "disabled_skills": config.skills.disabled_skills,
        "prompt_injection_mode": prompt_mode,
        "skills": payload,
    }))
    .into_response()
}

fn user_senweaver_subdir(child: &str) -> Option<PathBuf> {
    let raw = std::env::var("HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("USERPROFILE").ok().filter(|s| !s.is_empty()))?;
    Some(PathBuf::from(raw).join(".senweavercoding").join(child))
}

pub async fn handle_user_rules_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(rules_dir) = crate::user_rules::user_rules_dir() else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "could not resolve home directory (HOME / USERPROFILE not set)",
            })),
        )
            .into_response();
    };
    let exists = rules_dir.is_dir();
    let metas = crate::user_rules::list_user_rules();
    let files: Vec<serde_json::Value> = metas
        .into_iter()
        .map(|meta| {
            serde_json::json!({
                "name": meta.name,
                "path": meta.path.display().to_string(),
                "size": meta.size,
                "summary": meta.summary,
                "description": meta.description,
                "alwaysApply": meta.always_apply,
                "tier": if meta.always_apply { "always" } else { "on_demand" },
            })
        })
        .collect();
    Json(serde_json::json!({
        "directory": rules_dir.display().to_string(),
        "exists": exists,
        "files": files,
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
pub struct UserRuleNameQuery {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct UserRuleUpsertBody {
    pub name: String,
    pub content: String,
}

const USER_RULE_MAX_BODY_BYTES: usize = 256 * 1024;

fn sanitize_rule_filename(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("name must not be empty".to_string());
    }
    if trimmed.contains(['/', '\\', '\0']) || trimmed.contains("..") {
        return Err("name must not contain path separators or '..'".to_string());
    }
    if trimmed.starts_with('.') {
        return Err("name must not start with '.'".to_string());
    }
    let lower = trimmed.to_ascii_lowercase();
    let normalized =
        if lower.ends_with(".md") || lower.ends_with(".mdc") {
            trimmed.to_string()
        } else {
            format!("{trimmed}.md")
        };
    if normalized.len() > 200 {
        return Err("name is too long (>200 chars)".to_string());
    }
    Ok(normalized)
}

fn resolve_rule_path(name: &str) -> Result<(PathBuf, PathBuf), (StatusCode, String)> {
    let dir = crate::user_rules::user_rules_dir().ok_or((
        StatusCode::INTERNAL_SERVER_ERROR,
        "could not resolve home directory".to_string(),
    ))?;
    let filename =
        sanitize_rule_filename(name).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let path = dir.join(&filename);
    if !path.starts_with(&dir) {
        return Err((
            StatusCode::BAD_REQUEST,
            "resolved path escapes the rules directory".to_string(),
        ));
    }
    Ok((dir, path))
}

pub async fn handle_user_rule_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<UserRuleNameQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let (_, path) = match resolve_rule_path(&q.name) {
        Ok(v) => v,
        Err((code, msg)) => {
            return (code, Json(serde_json::json!({ "error": msg }))).into_response();
        }
    };
    if !path.is_file() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "rule file not found" })),
        )
            .into_response();
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => Json(serde_json::json!({
            "name": path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            "path": path.display().to_string(),
            "content": content,
        }))
        .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("read failed: {err}") })),
        )
            .into_response(),
    }
}

pub async fn handle_user_rule_upsert(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<UserRuleUpsertBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    if body.content.len() > USER_RULE_MAX_BODY_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(serde_json::json!({
                "error": format!(
                    "rule body exceeds {USER_RULE_MAX_BODY_BYTES} byte limit"
                ),
            })),
        )
            .into_response();
    }
    let (dir, path) = match resolve_rule_path(&body.name) {
        Ok(v) => v,
        Err((code, msg)) => {
            return (code, Json(serde_json::json!({ "error": msg }))).into_response();
        }
    };
    if let Err(err) = std::fs::create_dir_all(&dir) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("failed to create rules dir: {err}"),
            })),
        )
            .into_response();
    }
    if let Err(err) = std::fs::write(&path, body.content.as_bytes()) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("write failed: {err}") })),
        )
            .into_response();
    }
    Json(serde_json::json!({
        "status": "ok",
        "name": path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        "path": path.display().to_string(),
    }))
    .into_response()
}

pub async fn handle_user_rule_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<UserRuleNameQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let (_, path) = match resolve_rule_path(&q.name) {
        Ok(v) => v,
        Err((code, msg)) => {
            return (code, Json(serde_json::json!({ "error": msg }))).into_response();
        }
    };
    if !path.is_file() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "rule file not found" })),
        )
            .into_response();
    }
    match std::fs::remove_file(&path) {
        Ok(_) => Json(serde_json::json!({ "status": "ok" })).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("delete failed: {err}") })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct UserSkillNameQuery {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct UserSkillUpsertBody {
    pub name: String,
    pub content: String,
}

const USER_SKILL_MAX_BODY_BYTES: usize = 512 * 1024;

fn sanitize_skill_dirname(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("name must not be empty".to_string());
    }
    if trimmed.contains(['/', '\\', '\0', ' ']) || trimmed.contains("..") {
        return Err(
            "name must not contain path separators, spaces, or '..'".to_string(),
        );
    }
    if trimmed.starts_with('.') {
        return Err("name must not start with '.'".to_string());
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(
            "name may only contain ASCII letters, digits, '-' and '_'".to_string(),
        );
    }
    if trimmed.len() > 100 {
        return Err("name is too long (>100 chars)".to_string());
    }
    Ok(trimmed.to_string())
}

fn resolve_user_skill_paths(
    name: &str,
) -> Result<(PathBuf, PathBuf, PathBuf), (StatusCode, String)> {
    let raw = std::env::var("HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("USERPROFILE").ok().filter(|s| !s.is_empty()))
        .ok_or((
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not resolve home directory".to_string(),
        ))?;
    let root = PathBuf::from(raw).join(".senweavercoding").join("skills");
    let safe = sanitize_skill_dirname(name).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let dir = root.join(&safe);
    if !dir.starts_with(&root) {
        return Err((
            StatusCode::BAD_REQUEST,
            "resolved skill path escapes user skills root".to_string(),
        ));
    }
    let entry = dir.join("SKILL.md");
    Ok((root, dir, entry))
}

pub async fn handle_user_skill_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<UserSkillNameQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let (_, _, entry) = match resolve_user_skill_paths(&q.name) {
        Ok(v) => v,
        Err((code, msg)) => {
            return (code, Json(serde_json::json!({ "error": msg }))).into_response();
        }
    };
    if !entry.is_file() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "SKILL.md not found for this skill" })),
        )
            .into_response();
    }
    match std::fs::read_to_string(&entry) {
        Ok(content) => Json(serde_json::json!({
            "name": q.name,
            "path": entry.display().to_string(),
            "content": content,
        }))
        .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("read failed: {err}") })),
        )
            .into_response(),
    }
}

pub async fn handle_user_skill_upsert(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<UserSkillUpsertBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    if body.content.len() > USER_SKILL_MAX_BODY_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(serde_json::json!({
                "error": format!(
                    "skill body exceeds {USER_SKILL_MAX_BODY_BYTES} byte limit"
                ),
            })),
        )
            .into_response();
    }
    let (_, dir, entry) = match resolve_user_skill_paths(&body.name) {
        Ok(v) => v,
        Err((code, msg)) => {
            return (code, Json(serde_json::json!({ "error": msg }))).into_response();
        }
    };
    if let Err(err) = std::fs::create_dir_all(&dir) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("failed to create skill dir: {err}"),
            })),
        )
            .into_response();
    }
    if let Err(err) = std::fs::write(&entry, body.content.as_bytes()) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("write failed: {err}") })),
        )
            .into_response();
    }
    Json(serde_json::json!({
        "status": "ok",
        "name": body.name,
        "path": entry.display().to_string(),
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
pub struct UserSkillInstallBody {
    pub sources: Vec<String>,
    #[serde(default)]
    pub overwrite: bool,
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallMode {
    Abort,
    Overwrite,
    Rename,
}

impl InstallMode {
    fn from_body(mode: Option<&str>, overwrite_legacy: bool) -> Self {
        match mode {
            Some("overwrite") => InstallMode::Overwrite,
            Some("rename") => InstallMode::Rename,
            Some("abort") => InstallMode::Abort,
            _ if overwrite_legacy => InstallMode::Overwrite,
            _ => InstallMode::Abort,
        }
    }
}

fn derive_skill_dirname(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("name must not be empty".to_string());
    }
    let mut out = String::with_capacity(trimmed.len());
    let mut last_dash = false;
    for ch in trimmed.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else if ch == '-' || ch == '_' {
            ch
        } else {
            '-'
        };
        if mapped == '-' {
            if last_dash {
                continue;
            }
            last_dash = true;
        } else {
            last_dash = false;
        }
        out.push(mapped);
    }
    let cleaned = out.trim_matches(|c: char| c == '-' || c == '_').to_string();
    if cleaned.is_empty() {
        return Err(
            "name has no usable ASCII letters or digits; rename the source first".to_string(),
        );
    }
    let truncated = if cleaned.chars().count() > 80 {
        cleaned.chars().take(80).collect::<String>()
    } else {
        cleaned
    };
    let truncated = truncated
        .trim_matches(|c: char| c == '-' || c == '_')
        .to_string();
    if truncated.is_empty() {
        return Err("derived name is empty after sanitization".to_string());
    }
    Ok(truncated)
}


const USER_SKILL_INSTALL_MAX_BYTES: u64 = 16 * 1024 * 1024;
const USER_SKILL_INSTALL_MAX_FILES: usize = 2_000;

fn user_skills_root() -> Option<PathBuf> {
    let raw = std::env::var("HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("USERPROFILE").ok().filter(|s| !s.is_empty()))?;
    Some(PathBuf::from(raw).join(".senweavercoding").join("skills"))
}

fn parse_skill_name_from_md(content: &str) -> Option<String> {
    let normalized = content.replace("\r\n", "\n");
    let rest = normalized.strip_prefix("---\n")?;
    let end_idx = rest.find("\n---")?;
    let frontmatter = &rest[..end_idx];
    for line in frontmatter.lines() {
        let Some((key, val)) = line.split_once(':') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("name") {
            let v = val.trim().trim_matches('"').trim_matches('\'');
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn find_skill_md_in_dir(dir: &std::path::Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.eq_ignore_ascii_case("SKILL.md") {
                return Some(path);
            }
        }
    }
    None
}

fn measure_dir(dir: &std::path::Path) -> Result<(u64, usize), String> {
    let mut total_bytes: u64 = 0;
    let mut total_files: usize = 0;
    let mut stack: Vec<PathBuf> = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current)
            .map_err(|e| format!("read_dir {} failed: {e}", current.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_symlink() {
                return Err(format!("symlinks are not allowed: {}", path.display()));
            }
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                let meta = std::fs::metadata(&path)
                    .map_err(|e| format!("metadata {} failed: {e}", path.display()))?;
                total_bytes = total_bytes.saturating_add(meta.len());
                total_files = total_files.saturating_add(1);
                if total_bytes > USER_SKILL_INSTALL_MAX_BYTES {
                    return Err(format!(
                        "skill exceeds {}MB size limit",
                        USER_SKILL_INSTALL_MAX_BYTES / (1024 * 1024)
                    ));
                }
                if total_files > USER_SKILL_INSTALL_MAX_FILES {
                    return Err(format!(
                        "skill exceeds {} files limit",
                        USER_SKILL_INSTALL_MAX_FILES
                    ));
                }
            }
        }
    }
    Ok((total_bytes, total_files))
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let name = entry.file_name();
        let to = dst.join(&name);
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("symlink not allowed: {}", from.display()),
            ));
        }
        if ft.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if ft.is_file() {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

fn unique_suffix() -> String {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{pid}.{nanos}")
}

fn install_skill_atomic(
    src: &std::path::Path,
    final_dir: &std::path::Path,
    is_dir_source: bool,
    overwriting: bool,
) -> Result<(), String> {
    let final_name = final_dir
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "invalid final directory name".to_string())?;
    let parent = final_dir
        .parent()
        .ok_or_else(|| "final directory has no parent".to_string())?;

    let suffix = unique_suffix();
    let staging = parent.join(format!(".sw_install_{final_name}.{suffix}"));

    if staging.exists() {
        let _ = std::fs::remove_dir_all(&staging);
    }

    if is_dir_source {
        copy_dir_recursive(src, &staging).map_err(|err| {
            let _ = std::fs::remove_dir_all(&staging);
            format!("stage copy failed: {err}")
        })?;
    } else {
        if let Err(err) = std::fs::create_dir_all(&staging) {
            return Err(format!("stage mkdir failed: {err}"));
        }
        if let Err(err) = std::fs::copy(src, staging.join("SKILL.md")) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(format!("stage copy SKILL.md failed: {err}"));
        }
    }

    if overwriting {
        let backup = parent.join(format!(".sw_backup_{final_name}.{suffix}"));
        if let Err(err) = std::fs::rename(final_dir, &backup) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(format!("backup existing failed: {err}"));
        }
        if let Err(err) = std::fs::rename(&staging, final_dir) {
            let restore = std::fs::rename(&backup, final_dir);
            let _ = std::fs::remove_dir_all(&staging);
            return Err(if restore.is_ok() {
                format!("activate failed: {err} (existing skill restored)")
            } else {
                format!(
                    "activate failed: {err}; restore also failed; backup left at {}",
                    backup.display()
                )
            });
        }
        let _ = std::fs::remove_dir_all(&backup);
    } else if let Err(err) = std::fs::rename(&staging, final_dir) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!("activate failed: {err}"));
    }

    Ok(())
}

#[derive(Debug, serde::Serialize)]
struct InstallEntryReport {
    source: String,
    name: Option<String>,
    target: Option<String>,
    status: &'static str,
    error: Option<String>,
}

struct InstallPlan {
    raw_source: String,
    base_name: String,
    src_path: PathBuf,
    is_dir: bool,
}

pub async fn handle_user_skill_install(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<UserSkillInstallBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(root) = user_skills_root() else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "could not resolve home directory",
            })),
        )
            .into_response();
    };
    if let Err(err) = std::fs::create_dir_all(&root) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("failed to create skills root: {err}"),
            })),
        )
            .into_response();
    }
    let canonical_root = std::fs::canonicalize(&root).unwrap_or(root.clone());
    let mode = InstallMode::from_body(body.mode.as_deref(), body.overwrite);

    let mut reports: Vec<InstallEntryReport> = Vec::with_capacity(body.sources.len());
    let mut plans: Vec<InstallPlan> = Vec::with_capacity(body.sources.len());

    for raw_source in &body.sources {
        let source = PathBuf::from(raw_source);
        let canonical_src = match std::fs::canonicalize(&source) {
            Ok(p) => p,
            Err(err) => {
                reports.push(InstallEntryReport {
                    source: raw_source.clone(),
                    name: None,
                    target: None,
                    status: "error",
                    error: Some(format!("path not accessible: {err}")),
                });
                continue;
            }
        };
        if canonical_src.starts_with(&canonical_root) {
            reports.push(InstallEntryReport {
                source: raw_source.clone(),
                name: None,
                target: None,
                status: "error",
                error: Some("source is inside the user skills directory".to_string()),
            });
            continue;
        }

        let metadata = match std::fs::metadata(&canonical_src) {
            Ok(m) => m,
            Err(err) => {
                reports.push(InstallEntryReport {
                    source: raw_source.clone(),
                    name: None,
                    target: None,
                    status: "error",
                    error: Some(format!("metadata read failed: {err}")),
                });
                continue;
            }
        };

        let derived: Result<(String, PathBuf, bool), String> = if metadata.is_dir() {
            let skill_md = find_skill_md_in_dir(&canonical_src);
            if skill_md.is_none() {
                Err("directory has no SKILL.md at the top level".to_string())
            } else {
                let inferred_name = canonical_src
                    .file_name()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                let frontmatter_name = skill_md
                    .as_ref()
                    .and_then(|p| std::fs::read_to_string(p).ok())
                    .and_then(|c| parse_skill_name_from_md(&c));
                let chosen = frontmatter_name.unwrap_or(inferred_name);
                derive_skill_dirname(&chosen).map(|safe| (safe, canonical_src.clone(), true))
            }
        } else if metadata.is_file() {
            let ext_ok = canonical_src
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("md") || s.eq_ignore_ascii_case("mdc"))
                .unwrap_or(false);
            if !ext_ok {
                Err(
                    "only directories with SKILL.md or single .md/.mdc files are supported"
                        .to_string(),
                )
            } else {
                let raw_content = std::fs::read_to_string(&canonical_src).unwrap_or_default();
                let frontmatter_name = parse_skill_name_from_md(&raw_content);
                let stem_name = canonical_src
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                let chosen = frontmatter_name.unwrap_or(stem_name);
                derive_skill_dirname(&chosen).map(|safe| (safe, canonical_src.clone(), false))
            }
        } else {
            Err("source is neither a regular file nor a directory".to_string())
        };

        match derived {
            Ok((base_name, src_path, is_dir)) => {
                plans.push(InstallPlan {
                    raw_source: raw_source.clone(),
                    base_name,
                    src_path,
                    is_dir,
                });
            }
            Err(err) => {
                reports.push(InstallEntryReport {
                    source: raw_source.clone(),
                    name: None,
                    target: None,
                    status: "error",
                    error: Some(err),
                });
            }
        }
    }

    let mut name_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for plan in &plans {
        *name_counts.entry(plan.base_name.clone()).or_insert(0) += 1;
    }
    let mut planned_names: std::collections::HashSet<String> = std::collections::HashSet::new();

    for plan in plans {
        let InstallPlan {
            raw_source,
            base_name,
            src_path,
            is_dir,
        } = plan;

        let dup_in_batch = name_counts.get(&base_name).copied().unwrap_or(0) > 1;

        let final_name = if dup_in_batch && mode != InstallMode::Rename {
            reports.push(InstallEntryReport {
                source: raw_source,
                name: Some(base_name.clone()),
                target: None,
                status: "duplicate",
                error: Some(format!(
                    "multiple dropped items resolve to the same name '{base_name}'; choose 'Keep both' to auto-rename"
                )),
            });
            continue;
        } else {
            let target_existed = root.join(&base_name).exists()
                || planned_names.contains(&base_name);
            if target_existed {
                match mode {
                    InstallMode::Abort => {
                        reports.push(InstallEntryReport {
                            source: raw_source,
                            name: Some(base_name.clone()),
                            target: Some(root.join(&base_name).display().to_string()),
                            status: "exists",
                            error: None,
                        });
                        continue;
                    }
                    InstallMode::Overwrite => base_name.clone(),
                    InstallMode::Rename => {
                        let mut idx = 2u32;
                        let mut candidate = format!("{base_name}-{idx}");
                        while root.join(&candidate).exists() || planned_names.contains(&candidate)
                        {
                            idx += 1;
                            if idx > 999 {
                                reports.push(InstallEntryReport {
                                    source: raw_source.clone(),
                                    name: Some(base_name.clone()),
                                    target: None,
                                    status: "error",
                                    error: Some(format!(
                                        "no available name for '{base_name}' after 999 attempts"
                                    )),
                                });
                                break;
                            }
                            candidate = format!("{base_name}-{idx}");
                        }
                        if idx > 999 {
                            continue;
                        }
                        candidate
                    }
                }
            } else {
                base_name.clone()
            }
        };

        let target_dir = root.join(&final_name);
        let target_existed = target_dir.exists();
        let did_overwrite = target_existed && matches!(mode, InstallMode::Overwrite);

        if is_dir {
            if let Err(err) = measure_dir(&src_path) {
                reports.push(InstallEntryReport {
                    source: raw_source,
                    name: Some(final_name.clone()),
                    target: Some(target_dir.display().to_string()),
                    status: "error",
                    error: Some(err),
                });
                continue;
            }
        } else if let Ok(meta) = std::fs::metadata(&src_path) {
            if meta.len() > USER_SKILL_INSTALL_MAX_BYTES {
                reports.push(InstallEntryReport {
                    source: raw_source,
                    name: Some(final_name.clone()),
                    target: Some(target_dir.display().to_string()),
                    status: "error",
                    error: Some(format!(
                        "SKILL.md exceeds {}MB size limit",
                        USER_SKILL_INSTALL_MAX_BYTES / (1024 * 1024)
                    )),
                });
                continue;
            }
        }

        if let Err(err) = install_skill_atomic(&src_path, &target_dir, is_dir, did_overwrite) {
            reports.push(InstallEntryReport {
                source: raw_source,
                name: Some(final_name.clone()),
                target: Some(target_dir.display().to_string()),
                status: "error",
                error: Some(err),
            });
            continue;
        }

        planned_names.insert(final_name.clone());
        let renamed_from_base = final_name != base_name;
        reports.push(InstallEntryReport {
            source: raw_source,
            name: Some(final_name),
            target: Some(target_dir.display().to_string()),
            status: if did_overwrite {
                "overwritten"
            } else if renamed_from_base {
                "renamed"
            } else {
                "installed"
            },
            error: None,
        });
    }

    Json(serde_json::json!({ "results": reports })).into_response()
}

pub async fn handle_user_skill_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<UserSkillNameQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let (_, dir, _) = match resolve_user_skill_paths(&q.name) {
        Ok(v) => v,
        Err((code, msg)) => {
            return (code, Json(serde_json::json!({ "error": msg }))).into_response();
        }
    };
    if !dir.is_dir() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "user skill folder not found" })),
        )
            .into_response();
    }
    match std::fs::remove_dir_all(&dir) {
        Ok(_) => Json(serde_json::json!({ "status": "ok" })).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("delete failed: {err}") })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct SkillsDetailQuery {
    pub name: String,
    pub source: Option<String>,
    pub cwd: Option<String>,
}

pub async fn handle_skills_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<SkillsDetailQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    let cwd = q
        .cwd
        .map(PathBuf::from)
        .unwrap_or_else(|| config.workspace_dir.clone());
    let skill = crate::skills::discover_skills(&cwd, &config)
        .into_iter()
        .find(|s| s.name == q.name);
    let Some(skill) = skill else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "skill not found"})),
        )
            .into_response();
    };
    let readme = if let Some(loc) = skill.location.clone() {
        tokio::task::spawn_blocking(move || {
            for fname in ["SKILL.md", "README.md", "skill.md"] {
                let path = loc.join(fname);
                if let Ok(body) = std::fs::read_to_string(&path) {
                    return body;
                }
            }
            String::new()
        })
        .await
        .unwrap_or_default()
    } else {
        String::new()
    };
    Json(serde_json::json!({
        "detail": {
            "name": skill.name,
            "description": skill.description,
            "version": skill.version,
            "author": skill.author,
            "tags": skill.tags,
            "tools": skill.tools,
            "prompts": skill.prompts,
            "readme": readme,
            "location": skill.location.as_ref().map(|p| p.to_string_lossy().to_string()),
        }
    }))
    .into_response()
}

fn mcp_enabled_map() -> &'static parking_lot::RwLock<std::collections::HashMap<String, bool>> {
    static MAP: std::sync::OnceLock<parking_lot::RwLock<std::collections::HashMap<String, bool>>> =
        std::sync::OnceLock::new();
    MAP.get_or_init(|| parking_lot::RwLock::new(std::collections::HashMap::new()))
}

fn mcp_server_enabled(name: &str, default_enabled: bool) -> bool {
    mcp_enabled_map()
        .read()
        .get(name)
        .copied()
        .unwrap_or(default_enabled)
}

fn mcp_set_server_enabled(name: &str, enabled: bool) {
    mcp_enabled_map()
        .write()
        .insert(name.to_string(), enabled);
}

fn mcp_transport_str(t: &crate::config::McpTransport) -> &'static str {
    match t {
        crate::config::McpTransport::Stdio => "stdio",
        crate::config::McpTransport::Http => "http",
        crate::config::McpTransport::Sse => "sse",
    }
}

fn parse_mcp_transport(s: &str) -> crate::config::McpTransport {
    match s.to_ascii_lowercase().as_str() {
        "http" | "streamable" => crate::config::McpTransport::Http,
        "sse" => crate::config::McpTransport::Sse,
        _ => crate::config::McpTransport::Stdio,
    }
}

fn build_mcp_summary(server: &crate::config::McpServerConfig) -> String {
    match server.transport {
        crate::config::McpTransport::Stdio => {
            if server.args.is_empty() {
                server.command.clone()
            } else {
                format!("{} {}", server.command, server.args.join(" "))
            }
        }
        crate::config::McpTransport::Http | crate::config::McpTransport::Sse => server
            .url
            .clone()
            .unwrap_or_else(|| "<no url>".to_string()),
    }
}

async fn mcp_server_to_record(
    server: &crate::config::McpServerConfig,
    mcp_globally_enabled: bool,
    config_path: &str,
) -> serde_json::Value {
    let enabled = mcp_globally_enabled && mcp_server_enabled(&server.name, true);
    let (status, status_label, status_detail): (&str, &str, Option<String>) =
        match crate::services::try_get_services() {
            Some(svc) => match svc.mcp.get_server(&server.name).await {
                Some(conn) => match conn.status {
                    crate::services::mcp_manager::McpServerStatus::Connected => {
                        ("connected", "Connected", None)
                    }
                    crate::services::mcp_manager::McpServerStatus::Connecting => {
                        ("checking", "Connecting…", None)
                    }
                    crate::services::mcp_manager::McpServerStatus::Disabled => {
                        ("disabled", "Disabled", None)
                    }
                    crate::services::mcp_manager::McpServerStatus::Disconnected => {
                        ("failed", "Disconnected", conn.error.clone())
                    }
                    crate::services::mcp_manager::McpServerStatus::Error => {
                        ("failed", "Error", conn.error.clone())
                    }
                },
                None => {
                    if !enabled {
                        ("disabled", "Disabled", None)
                    } else {
                        ("checking", "Pending", None)
                    }
                }
            },
            None => {
                if !enabled {
                    ("disabled", "Disabled", None)
                } else {
                    ("checking", "Pending", None)
                }
            }
        };

    let transport_str = mcp_transport_str(&server.transport);
    let config_value = match server.transport {
        crate::config::McpTransport::Stdio => serde_json::json!({
            "type": "stdio",
            "command": server.command,
            "args": server.args,
            "env": server.env,
        }),
        crate::config::McpTransport::Http => serde_json::json!({
            "type": "http",
            "url": server.url.clone().unwrap_or_default(),
            "headers": server.headers,
        }),
        crate::config::McpTransport::Sse => serde_json::json!({
            "type": "sse",
            "url": server.url.clone().unwrap_or_default(),
            "headers": server.headers,
        }),
    };

    serde_json::json!({
        "name": server.name,
        "scope": "user",
        "transport": transport_str,
        "enabled": enabled,
        "status": status,
        "statusLabel": status_label,
        "statusDetail": status_detail,
        "configLocation": config_path,
        "summary": build_mcp_summary(server),
        "canEdit": true,
        "canRemove": true,
        "canReconnect": true,
        "canToggle": true,
        "config": config_value,
    })
}

#[derive(Debug, Deserialize)]
pub struct McpConfigBody {
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Option<Vec<String>>,
    #[serde(default)]
    pub env: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
pub struct McpUpsertBody {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    pub config: McpConfigBody,
    #[serde(default)]
    pub cwd: Option<String>,
}

fn body_to_server(name: &str, body: &McpUpsertBody) -> crate::config::McpServerConfig {
    let cfg = &body.config;
    let transport = parse_mcp_transport(cfg.kind.as_deref().unwrap_or("stdio"));
    crate::config::McpServerConfig {
        name: name.to_string(),
        transport,
        url: cfg.url.clone(),
        command: cfg.command.clone().unwrap_or_default(),
        args: cfg.args.clone().unwrap_or_default(),
        env: cfg.env.clone().unwrap_or_default(),
        headers: cfg.headers.clone().unwrap_or_default(),
        tool_timeout_secs: None,

        enabled: true,
    }
}

fn config_path_string(state: &AppState) -> String {
    state
        .config
        .lock()
        .config_path
        .display()
        .to_string()
}

pub async fn handle_mcp_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    let mcp_enabled = config.mcp.enabled;
    let location = config_path_string(&state);
    let mut servers: Vec<serde_json::Value> = Vec::new();
    for server in &config.mcp.servers {
        servers.push(mcp_server_to_record(server, mcp_enabled, &location).await);
    }
    Json(serde_json::json!({ "servers": servers })).into_response()
}

pub async fn handle_mcp_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    let location = config_path_string(&state);
    let Some(server) = config.mcp.servers.iter().find(|s| s.name == name).cloned() else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "server not found"})),
        )
            .into_response();
    };
    let record = mcp_server_to_record(&server, config.mcp.enabled, &location).await;
    Json(serde_json::json!({ "server": record })).into_response()
}

pub async fn handle_mcp_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<McpUpsertBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(name) = body
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "name is required"})),
        )
            .into_response();
    };

    let snapshot = {
        let mut cfg = state.config.lock();
        if cfg.mcp.servers.iter().any(|s| s.name == name) {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": format!("server already exists: {name}")})),
            )
                .into_response();
        }
        cfg.mcp.enabled = true;
        cfg.mcp.servers.push(body_to_server(&name, &body));
        cfg.clone()
    };
    if let Err(e) = snapshot.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("save config: {e}")})),
        )
            .into_response();
    }

    *state.config.lock() = snapshot.clone();
    state.push_live_config(snapshot.clone());
    mcp_set_server_enabled(&name, true);

    let location = config_path_string(&state);
    let server = snapshot
        .mcp
        .servers
        .iter()
        .find(|s| s.name == name)
        .cloned()
        .unwrap_or_default();
    let record = mcp_server_to_record(&server, snapshot.mcp.enabled, &location).await;
    Json(serde_json::json!({ "server": record })).into_response()
}

pub async fn handle_mcp_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(body): Json<McpUpsertBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let snapshot = {
        let mut cfg = state.config.lock();
        let Some(idx) = cfg.mcp.servers.iter().position(|s| s.name == name) else {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": format!("server not found: {name}")})),
            )
                .into_response();
        };

        let preserved_enabled = cfg.mcp.servers[idx].enabled;
        let mut new_server = body_to_server(&name, &body);
        new_server.enabled = preserved_enabled;
        cfg.mcp.servers[idx] = new_server;
        cfg.clone()
    };
    if let Err(e) = snapshot.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("save config: {e}")})),
        )
            .into_response();
    }
    *state.config.lock() = snapshot.clone();
    state.push_live_config(snapshot.clone());
    let location = config_path_string(&state);
    let server = snapshot
        .mcp
        .servers
        .iter()
        .find(|s| s.name == name)
        .cloned()
        .unwrap_or_default();
    let record = mcp_server_to_record(&server, snapshot.mcp.enabled, &location).await;
    Json(serde_json::json!({ "server": record })).into_response()
}

pub async fn handle_mcp_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let snapshot = {
        let mut cfg = state.config.lock();
        let Some(idx) = cfg.mcp.servers.iter().position(|s| s.name == name) else {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": format!("server not found: {name}")})),
            )
                .into_response();
        };
        cfg.mcp.servers.remove(idx);
        cfg.clone()
    };
    if let Err(e) = snapshot.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("save config: {e}")})),
        )
            .into_response();
    }
    *state.config.lock() = snapshot.clone();
    state.push_live_config(snapshot);
    if let Some(svc) = crate::services::try_get_services() {
        let _ = svc.mcp.remove_server(&name).await;
    }
    mcp_enabled_map().write().remove(&name);
    Json(serde_json::json!({ "ok": true })).into_response()
}

pub async fn handle_mcp_toggle(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let snapshot = {
        let mut cfg = state.config.lock();
        let Some(idx) = cfg.mcp.servers.iter().position(|s| s.name == name) else {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": format!("server not found: {name}")})),
            )
                .into_response();
        };
        cfg.mcp.servers[idx].enabled = !cfg.mcp.servers[idx].enabled;
        cfg.clone()
    };

    if let Err(e) = snapshot.save().await {
        tracing::error!("Failed to save config (mcp toggle): {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to save configuration"})),
        )
            .into_response();
    }
    *state.config.lock() = snapshot.clone();
    state.push_live_config(snapshot.clone());

    let server = snapshot
        .mcp
        .servers
        .iter()
        .find(|s| s.name == name)
        .cloned()
        .unwrap_or_default();
    let new_state = server.enabled;

    mcp_set_server_enabled(&name, new_state);
    if let Some(svc) = crate::services::try_get_services() {
        if new_state {
            svc.mcp
                .set_server_status(
                    &name,
                    crate::services::mcp_manager::McpServerStatus::Disconnected,
                    None,
                )
                .await;
        } else {
            svc.mcp
                .set_server_status(
                    &name,
                    crate::services::mcp_manager::McpServerStatus::Disabled,
                    None,
                )
                .await;
        }
    }
    let location = config_path_string(&state);
    let record = mcp_server_to_record(&server, snapshot.mcp.enabled, &location).await;
    Json(serde_json::json!({ "server": record })).into_response()
}

pub async fn handle_mcp_reconnect(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    let Some(server) = config.mcp.servers.iter().find(|s| s.name == name).cloned() else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("server not found: {name}")})),
        )
            .into_response();
    };
    if let Some(svc) = crate::services::try_get_services() {
        svc.mcp
            .set_server_status(
                &name,
                crate::services::mcp_manager::McpServerStatus::Connecting,
                None,
            )
            .await;
    }
    let location = config_path_string(&state);
    let record = mcp_server_to_record(&server, config.mcp.enabled, &location).await;
    Json(serde_json::json!({ "server": record })).into_response()
}

fn plugins_enabled_map() -> &'static parking_lot::RwLock<std::collections::HashMap<String, bool>> {
    static MAP: std::sync::OnceLock<parking_lot::RwLock<std::collections::HashMap<String, bool>>> =
        std::sync::OnceLock::new();
    MAP.get_or_init(|| parking_lot::RwLock::new(std::collections::HashMap::new()))
}

fn plugin_enabled(name: &str, default_enabled: bool) -> bool {
    plugins_enabled_map()
        .read()
        .get(name)
        .copied()
        .unwrap_or(default_enabled)
}

fn plugin_install_path() -> String {
    std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("plugins")
        .display()
        .to_string()
}

#[cfg(feature = "plugins-wasm")]
fn plugin_capability_to_key(cap: &crate::plugins::PluginCapability) -> &'static str {
    match cap {
        crate::plugins::PluginCapability::Tool => "commands",
        crate::plugins::PluginCapability::Channel => "hooks",
        crate::plugins::PluginCapability::Memory => "skills",
        crate::plugins::PluginCapability::Observer => "lspServers",
    }
}

#[cfg(feature = "plugins-wasm")]
fn plugin_to_summary(
    info: &crate::plugins::PluginInfo,
    globally_enabled: bool,
    install_path: &str,
) -> serde_json::Value {
    let enabled = globally_enabled && plugin_enabled(&info.name, true);
    let mut counts = serde_json::Map::new();
    for key in [
        "commands",
        "agents",
        "skills",
        "hooks",
        "mcpServers",
        "lspServers",
    ] {
        counts.insert(key.to_string(), serde_json::Value::Number(0.into()));
    }
    for cap in &info.capabilities {
        let key = plugin_capability_to_key(cap);
        if let Some(slot) = counts.get_mut(key) {
            let n = slot.as_u64().unwrap_or(0) + 1;
            *slot = serde_json::Value::Number(n.into());
        }
    }
    serde_json::json!({
        "id": info.name,
        "name": info.name,
        "marketplace": "local",
        "scope": "user",
        "enabled": enabled,
        "hasErrors": false,
        "isBuiltin": false,
        "version": info.version,
        "description": info.description,
        "authorName": null,
        "installPath": install_path,
        "componentCounts": serde_json::Value::Object(counts),
        "errors": serde_json::Value::Array(Vec::new()),
    })
}

#[cfg(feature = "plugins-wasm")]
fn collect_plugins() -> Vec<crate::plugins::PluginInfo> {
    use crate::plugins::host::PluginHost;
    let workspace = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    match PluginHost::new(&workspace) {
        Ok(host) => host.list_plugins(),
        Err(_) => Vec::new(),
    }
}

#[cfg(not(feature = "plugins-wasm"))]
fn empty_plugin_summary() -> Vec<serde_json::Value> {
    Vec::new()
}

pub async fn handle_plugins_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    #[cfg(feature = "plugins-wasm")]
    {
        let globally_enabled = state.config.lock().plugins.enabled;
        let install_path = plugin_install_path();
        let plugins = collect_plugins();
        let plugins_json: Vec<serde_json::Value> = plugins
            .iter()
            .map(|info| plugin_to_summary(info, globally_enabled, &install_path))
            .collect();
        let total = plugins_json.len() as u64;
        let enabled_count = plugins_json
            .iter()
            .filter(|p: &&serde_json::Value| {
                p.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false)
            })
            .count() as u64;
        return Json(serde_json::json!({
            "plugins": plugins_json,
            "marketplaces": [],
            "summary": {
                "total": total,
                "enabled": enabled_count,
                "errorCount": 0,
                "marketplaceCount": 0,
            },
        }))
        .into_response();
    }
    #[cfg(not(feature = "plugins-wasm"))]
    {
        let _ = &state;
        let _ = plugin_install_path();
        let _ = plugin_enabled("__noop__", true);
        Json(serde_json::json!({
            "plugins": empty_plugin_summary(),
            "marketplaces": [],
            "summary": { "total": 0, "enabled": 0, "errorCount": 0, "marketplaceCount": 0 },
        }))
        .into_response()
    }
}

#[derive(Debug, Deserialize)]
pub struct PluginDetailQuery {
    pub id: String,
    #[serde(default)]
    pub cwd: Option<String>,
}

pub async fn handle_plugins_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<PluginDetailQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    #[cfg(feature = "plugins-wasm")]
    {
        let globally_enabled = state.config.lock().plugins.enabled;
        let install_path = plugin_install_path();
        let plugins = collect_plugins();
        let Some(info) = plugins.into_iter().find(|p| p.name == q.id) else {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": format!("plugin not found: {}", q.id)})),
            )
                .into_response();
        };
        let mut summary = plugin_to_summary(&info, globally_enabled, &install_path);
        if let serde_json::Value::Object(ref mut map) = summary {
            map.insert(
                "capabilities".to_string(),
                serde_json::json!({
                    "commands": [], "agents": [], "skills": [],
                    "hooks": [], "mcpServers": [], "lspServers": [],
                }),
            );
            map.insert("commandEntries".to_string(), serde_json::json!([]));
            map.insert("agentEntries".to_string(), serde_json::json!([]));
            map.insert("hookEntries".to_string(), serde_json::json!([]));
            map.insert("skillEntries".to_string(), serde_json::json!([]));
            map.insert("mcpServerEntries".to_string(), serde_json::json!([]));
        }
        return Json(serde_json::json!({ "detail": summary })).into_response();
    }
    #[cfg(not(feature = "plugins-wasm"))]
    {
        let _ = &state;
        let _ = q.id;
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "plugin runtime disabled in this build"})),
        )
            .into_response()
    }
}

#[derive(Debug, Deserialize)]
pub struct PluginActionBody {
    pub id: String,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default, rename = "keepData")]
    pub keep_data: Option<bool>,
}

pub async fn handle_plugins_enable(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PluginActionBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    plugins_enabled_map().write().insert(body.id.clone(), true);
    let _ = &state;
    Json(serde_json::json!({ "ok": true, "message": format!("plugin enabled: {}", body.id) })).into_response()
}

pub async fn handle_plugins_disable(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PluginActionBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    plugins_enabled_map().write().insert(body.id.clone(), false);
    let _ = &state;
    Json(serde_json::json!({ "ok": true, "message": format!("plugin disabled: {}", body.id) })).into_response()
}

pub async fn handle_plugins_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PluginActionBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let _ = &state;
    Json(serde_json::json!({
        "ok": true,
        "message": format!("plugin update is not yet supported in the embedded gateway: {}", body.id),
    }))
    .into_response()
}

pub async fn handle_plugins_uninstall(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PluginActionBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    #[cfg(feature = "plugins-wasm")]
    {
        use crate::plugins::host::PluginHost;
        let workspace = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        match PluginHost::new(&workspace) {
            Ok(mut host) => match host.uninstall(&body.id) {
                Ok(_) => {
                    plugins_enabled_map().write().remove(&body.id);
                    let _ = &state;
                    Json(serde_json::json!({
                        "ok": true,
                        "message": format!("plugin uninstalled: {}", body.id),
                    }))
                    .into_response()
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": format!("uninstall failed: {e}")})),
                )
                    .into_response(),
            },
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("plugin host: {e}")})),
            )
                .into_response(),
        }
    }
    #[cfg(not(feature = "plugins-wasm"))]
    {
        let _ = &state;
        plugins_enabled_map().write().remove(&body.id);
        Json(serde_json::json!({
            "ok": true,
            "message": "plugin runtime disabled in this build",
        }))
        .into_response()
    }
}

pub async fn handle_plugins_reload(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    #[cfg(feature = "plugins-wasm")]
    {
        let globally_enabled = state.config.lock().plugins.enabled;
        let plugins = collect_plugins();
        let enabled_count = plugins
            .iter()
            .filter(|p: &&crate::plugins::PluginInfo| {
                globally_enabled && plugin_enabled(&p.name, true)
            })
            .count() as u64;
        let disabled_count = plugins.len() as u64 - enabled_count;
        return Json(serde_json::json!({
            "ok": true,
            "summary": {
                "enabled": enabled_count,
                "disabled": disabled_count,
                "skills": 0, "agents": 0, "hooks": 0,
                "mcpServers": 0, "lspServers": 0, "errors": 0,
            },
        }))
        .into_response();
    }
    #[cfg(not(feature = "plugins-wasm"))]
    {
        let _ = &state;
        Json(serde_json::json!({
            "ok": true,
            "summary": {
                "enabled": 0, "disabled": 0,
                "skills": 0, "agents": 0, "hooks": 0,
                "mcpServers": 0, "lspServers": 0, "errors": 0,
            },
        }))
        .into_response()
    }
}

pub async fn handle_teams_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    Json(serde_json::json!({ "teams": [] })).into_response()
}

pub async fn handle_teams_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    Json(serde_json::json!({
        "name": name,
        "description": "",
        "members": [],
    }))
    .into_response()
}

pub async fn handle_teams_member_transcript(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((_team, _member)): Path<(String, String)>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    Json(serde_json::json!({ "messages": [] })).into_response()
}

pub async fn handle_teams_member_send(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((_team, _member)): Path<(String, String)>,
    Json(_body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    Json(serde_json::json!({ "ok": true })).into_response()
}

pub async fn handle_teams_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(_name): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    Json(serde_json::json!({ "ok": true })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct AgentsListQuery {
    pub cwd: Option<String>,
}

pub async fn handle_agents_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(_q): Query<AgentsListQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();

    let mut all_agents: Vec<serde_json::Value> = config
        .agents
        .iter()
        .map(|(name, cfg)| {
            serde_json::json!({
                "agentType": name,
                "description": cfg.system_prompt.clone().unwrap_or_default(),
                "model": cfg.model,
                "modelDisplay": format!("{}/{}", cfg.provider, cfg.model),
                "tools": cfg.allowed_tools,
                "systemPrompt": cfg.system_prompt,
                "color": null,
                "source": "userSettings",
                "baseDir": null,
                "overriddenBy": null,
                "isActive": true,
            })
        })
        .collect();
    all_agents.sort_by(|a, b| {
        a.get("agentType")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .cmp(b.get("agentType").and_then(|v| v.as_str()).unwrap_or(""))
    });

    let active_agents = all_agents.clone();

    Json(serde_json::json!({
        "activeAgents": active_agents,
        "allAgents": all_agents,
    }))
    .into_response()
}

fn snake_to_camel(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut upper_next = false;
    for ch in input.chars() {
        if ch == '_' {
            upper_next = true;
        } else if upper_next {
            out.extend(ch.to_uppercase());
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

fn camel_to_snake(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 4);
    for (i, ch) in input.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

fn rekey_value(value: serde_json::Value, mapper: &dyn Fn(&str) -> String) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut next = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                next.insert(mapper(&k), rekey_value(v, mapper));
            }
            serde_json::Value::Object(next)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(|v| rekey_value(v, mapper)).collect())
        }
        other => other,
    }
}

fn to_camel_case_keys(value: serde_json::Value) -> serde_json::Value {
    rekey_value(value, &|k| snake_to_camel(k))
}

fn to_snake_case_keys(value: serde_json::Value) -> serde_json::Value {
    rekey_value(value, &|k| camel_to_snake(k))
}

fn channel_to_camel_json<T: serde::Serialize>(opt: &Option<T>) -> serde_json::Value {
    match opt {
        Some(value) => match serde_json::to_value(value) {
            Ok(v) => to_camel_case_keys(v),
            Err(_) => serde_json::Value::Null,
        },
        None => serde_json::Value::Null,
    }
}

fn deep_merge(target: &mut serde_json::Value, source: serde_json::Value) {
    match (target, source) {
        (serde_json::Value::Object(t), serde_json::Value::Object(s)) => {
            for (k, v) in s {
                let entry = t.entry(k).or_insert(serde_json::Value::Null);
                deep_merge(entry, v);
            }
        }
        (slot, other) => *slot = other,
    }
}

fn build_adapters_payload(config: &crate::config::Config) -> serde_json::Value {
    let cc = &config.channels_config;

    let mut telegram_value = channel_to_camel_json(&cc.telegram);
    if let serde_json::Value::Object(map) = &mut telegram_value {

        map.entry("pairedUsers")
            .or_insert(serde_json::Value::Array(Vec::new()));
        map.entry("defaultWorkDir")
            .or_insert(serde_json::Value::Null);
    }

    let mut feishu_value = channel_to_camel_json(&cc.feishu);
    if let serde_json::Value::Object(map) = &mut feishu_value {
        map.entry("pairedUsers")
            .or_insert(serde_json::Value::Array(Vec::new()));
        map.entry("defaultWorkDir")
            .or_insert(serde_json::Value::Null);
        map.entry("streamingCard")
            .or_insert(serde_json::Value::Bool(false));
    }

    let nostr_value = {
        #[cfg(feature = "channel-nostr")]
        {
            channel_to_camel_json(&cc.nostr)
        }
        #[cfg(not(feature = "channel-nostr"))]
        {
            serde_json::Value::Null
        }
    };
    let voice_wake_value = {
        #[cfg(feature = "voice-wake")]
        {
            channel_to_camel_json(&cc.voice_wake)
        }
        #[cfg(not(feature = "voice-wake"))]
        {
            serde_json::Value::Null
        }
    };

    let features = serde_json::json!({
        "channelNostr": cfg!(feature = "channel-nostr"),
        "voiceWake": cfg!(feature = "voice-wake"),
        "channelMatrix": cfg!(feature = "channel-matrix"),
    });

    serde_json::json!({
        "serverUrl": format!("http://{}:{}", config.gateway.host, config.gateway.port),
        "defaultProjectDir": super::api::display_path(&config.workspace_dir.display().to_string()),
        "pairing": {
            "code": serde_json::Value::Null,
            "expiresAt": serde_json::Value::Null,
            "createdAt": serde_json::Value::Null,
        },
        "global": {
            "cli": cc.cli,
            "messageTimeoutSecs": cc.message_timeout_secs,
            "ackReactions": cc.ack_reactions,
            "showToolCalls": cc.show_tool_calls,
            "sessionPersistence": cc.session_persistence,
            "sessionBackend": cc.session_backend,
            "sessionTtlHours": cc.session_ttl_hours,
            "debounceMs": cc.debounce_ms,
        },
        "features": features,
        "telegram": telegram_value,
        "discord": channel_to_camel_json(&cc.discord),
        "discordHistory": channel_to_camel_json(&cc.discord_history),
        "slack": channel_to_camel_json(&cc.slack),
        "mattermost": channel_to_camel_json(&cc.mattermost),
        "webhook": channel_to_camel_json(&cc.webhook),
        "imessage": channel_to_camel_json(&cc.imessage),
        "matrix": channel_to_camel_json(&cc.matrix),
        "signal": channel_to_camel_json(&cc.signal),
        "whatsapp": channel_to_camel_json(&cc.whatsapp),
        "linq": channel_to_camel_json(&cc.linq),
        "wati": channel_to_camel_json(&cc.wati),
        "nextcloudTalk": channel_to_camel_json(&cc.nextcloud_talk),
        "email": channel_to_camel_json(&cc.email),
        "gmailPush": channel_to_camel_json(&cc.gmail_push),
        "irc": channel_to_camel_json(&cc.irc),
        "lark": channel_to_camel_json(&cc.lark),
        "feishu": feishu_value,
        "dingtalk": channel_to_camel_json(&cc.dingtalk),
        "wecom": channel_to_camel_json(&cc.wecom),
        "qq": channel_to_camel_json(&cc.qq),
        "twitter": channel_to_camel_json(&cc.twitter),
        "mochat": channel_to_camel_json(&cc.mochat),
        "nostr": nostr_value,
        "clawdtalk": channel_to_camel_json(&cc.clawdtalk),
        "reddit": channel_to_camel_json(&cc.reddit),
        "bluesky": channel_to_camel_json(&cc.bluesky),
        "voiceCall": channel_to_camel_json(&cc.voice_call),
        "voiceWake": voice_wake_value,
    })
}

pub async fn handle_adapters_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    Json(build_adapters_payload(&config)).into_response()
}

fn apply_channel_patch<T>(slot: &mut Option<T>, patch_value: &serde_json::Value, channel_id: &str)
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    if patch_value.is_null() {
        *slot = None;
        return;
    }
    let Some(_obj) = patch_value.as_object() else {
        return;
    };

    let mut current_camel = match slot {
        Some(existing) => to_camel_case_keys(
            serde_json::to_value(existing).unwrap_or(serde_json::Value::Object(Default::default())),
        ),
        None => serde_json::Value::Object(serde_json::Map::new()),
    };
    deep_merge(&mut current_camel, patch_value.clone());
    let snake = to_snake_case_keys(current_camel);
    match serde_json::from_value::<T>(snake) {
        Ok(parsed) => *slot = Some(parsed),
        Err(err) => {
            tracing::warn!(error = %err, channel = channel_id, "ignoring invalid adapters patch");
        }
    }
}

pub async fn handle_adapters_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let snapshot = {
        let mut cfg = state.config.lock();

        if let Some(dir) = body
            .get("defaultProjectDir")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            cfg.workspace_dir = std::path::PathBuf::from(dir);
        }

        if let Some(global) = body.get("global").and_then(|v| v.as_object()) {
            if let Some(b) = global.get("cli").and_then(|v| v.as_bool()) {
                cfg.channels_config.cli = b;
            }
            if let Some(n) = global.get("messageTimeoutSecs").and_then(|v| v.as_u64()) {
                cfg.channels_config.message_timeout_secs = n;
            }
            if let Some(b) = global.get("ackReactions").and_then(|v| v.as_bool()) {
                cfg.channels_config.ack_reactions = b;
            }
            if let Some(b) = global.get("showToolCalls").and_then(|v| v.as_bool()) {
                cfg.channels_config.show_tool_calls = b;
            }
            if let Some(b) = global.get("sessionPersistence").and_then(|v| v.as_bool()) {
                cfg.channels_config.session_persistence = b;
            }
            if let Some(s) = global
                .get("sessionBackend")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                cfg.channels_config.session_backend = s.to_string();
            }
            if let Some(n) = global.get("sessionTtlHours").and_then(|v| v.as_u64()) {
                cfg.channels_config.session_ttl_hours = u32::try_from(n).unwrap_or(u32::MAX);
            }
            if let Some(n) = global.get("debounceMs").and_then(|v| v.as_u64()) {
                cfg.channels_config.debounce_ms = n;
            }
        }

        macro_rules! patch_channel {
            ($key:literal, $field:ident) => {
                if let Some(value) = body.get($key) {
                    apply_channel_patch(&mut cfg.channels_config.$field, value, $key);
                }
            };
        }

        patch_channel!("telegram", telegram);
        patch_channel!("discord", discord);
        patch_channel!("discordHistory", discord_history);
        patch_channel!("slack", slack);
        patch_channel!("mattermost", mattermost);
        patch_channel!("webhook", webhook);
        patch_channel!("imessage", imessage);
        patch_channel!("matrix", matrix);
        patch_channel!("signal", signal);
        patch_channel!("whatsapp", whatsapp);
        patch_channel!("linq", linq);
        patch_channel!("wati", wati);
        patch_channel!("nextcloudTalk", nextcloud_talk);
        patch_channel!("email", email);
        patch_channel!("gmailPush", gmail_push);
        patch_channel!("irc", irc);
        patch_channel!("lark", lark);
        patch_channel!("feishu", feishu);
        patch_channel!("dingtalk", dingtalk);
        patch_channel!("wecom", wecom);
        patch_channel!("qq", qq);
        patch_channel!("twitter", twitter);
        patch_channel!("mochat", mochat);
        patch_channel!("clawdtalk", clawdtalk);
        patch_channel!("reddit", reddit);
        patch_channel!("bluesky", bluesky);
        patch_channel!("voiceCall", voice_call);

        #[cfg(feature = "channel-nostr")]
        patch_channel!("nostr", nostr);

        #[cfg(feature = "voice-wake")]
        patch_channel!("voiceWake", voice_wake);

        cfg.clone()
    };

    if let Err(e) = snapshot.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("save config: {e}")})),
        )
            .into_response();
    }
    *state.config.lock() = snapshot.clone();
    state.push_live_config(snapshot.clone());

    Json(build_adapters_payload(&snapshot)).into_response()
}

fn detect_python() -> Option<(String, String)> {

    let candidates: &[&[&str]] = if cfg!(target_os = "windows") {
        &[&["py", "-3"], &["python"], &["python3"]]
    } else {
        &[&["python3"], &["python"]]
    };
    for cmd in candidates {
        let mut command = crate::util::hidden_sync_command(cmd[0]);
        for arg in &cmd[1..] {
            command.arg(arg);
        }
        command.arg("--version");
        if let Ok(out) = command.output() {
            if out.status.success() {
                let v = String::from_utf8_lossy(&out.stdout)
                    .trim()
                    .to_string()
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();
                let v = if v.is_empty() {
                    String::from_utf8_lossy(&out.stderr)
                        .trim()
                        .to_string()
                } else {
                    v
                };
                let path = which::which(cmd[0])
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| cmd[0].to_string());
                return Some((v, path));
            }
        }
    }
    None
}

fn computer_use_venv_dir() -> std::path::PathBuf {
    let base = directories::ProjectDirs::from("io", "senweaver", "senweavercoding")
        .map(|p| p.data_local_dir().to_path_buf())
        .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from))
        .or_else(|| std::env::var_os("USERPROFILE").map(std::path::PathBuf::from))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    base.join("computer_use_venv")
}

pub async fn handle_computer_use_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let supported = cfg!(target_os = "macos") || cfg!(target_os = "windows");
    let (py_installed, py_version, py_path) = match tokio::task::spawn_blocking(detect_python).await
    {
        Ok(Some((v, p))) => (true, Some(v), Some(p)),
        _ => (false, None, None),
    };
    let venv_dir = computer_use_venv_dir();
    let venv_created = venv_dir.exists();
    Json(serde_json::json!({
        "platform": std::env::consts::OS,
        "supported": supported,
        "python": {
            "installed": py_installed,
            "version": py_version,
            "path": py_path,
        },
        "venv": {
            "created": venv_created,
            "path": venv_dir.to_string_lossy(),
        },
        "dependencies": {
            "installed": venv_created,
            "requirementsFound": venv_created,
        },
        "permissions": {
            "accessibility": serde_json::Value::Null,
            "screenRecording": serde_json::Value::Null,
        },
    }))
    .into_response()
}

pub async fn handle_computer_use_setup(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let mut steps: Vec<serde_json::Value> = Vec::new();
    let mut success = true;

    let py = match tokio::task::spawn_blocking(detect_python).await.ok().flatten() {
        Some(p) => p,
        None => {
            return Json(serde_json::json!({
                "success": false,
                "steps": [{
                    "name": "python_check",
                    "ok": false,
                    "message": "Python 3 not found on PATH; install it from python.org and retry.",
                }],
            }))
            .into_response();
        }
    };
    steps.push(serde_json::json!({
        "name": "python_check",
        "ok": true,
        "message": format!("Found {}", py.0),
    }));

    let venv_dir = computer_use_venv_dir();
    if let Some(parent) = venv_dir.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    if !venv_dir.exists() {
        let py_path = py.1.clone();
        let venv_path = venv_dir.clone();
        let result = tokio::task::spawn_blocking(move || {
            crate::util::hidden_sync_command(py_path)
                .args(["-m", "venv"])
                .arg(&venv_path)
                .output()
        })
        .await;
        match result {
            Ok(Ok(out)) if out.status.success() => steps.push(serde_json::json!({
                "name": "venv_create",
                "ok": true,
                "message": format!("Created venv at {}", venv_dir.to_string_lossy()),
            })),
            Ok(Ok(out)) => {
                success = false;
                steps.push(serde_json::json!({
                    "name": "venv_create",
                    "ok": false,
                    "message": String::from_utf8_lossy(&out.stderr).into_owned(),
                }));
            }
            _ => {
                success = false;
                steps.push(serde_json::json!({
                    "name": "venv_create",
                    "ok": false,
                    "message": "Failed to spawn python -m venv",
                }));
            }
        }
    } else {
        steps.push(serde_json::json!({
            "name": "venv_create",
            "ok": true,
            "message": "Venv already exists",
        }));
    }

    if success {
        let pip = if cfg!(target_os = "windows") {
            venv_dir.join("Scripts").join("pip.exe")
        } else {
            venv_dir.join("bin").join("pip")
        };
        if pip.exists() {
            let pip_path = pip.clone();
            let pkgs = vec!["pyautogui", "pillow", "pynput"];
            let result = tokio::task::spawn_blocking(move || {
                let mut cmd = crate::util::hidden_sync_command(pip_path);
                cmd.arg("install").arg("--upgrade");
                for pkg in pkgs {
                    cmd.arg(pkg);
                }
                cmd.output()
            })
            .await;
            match result {
                Ok(Ok(out)) if out.status.success() => steps.push(serde_json::json!({
                    "name": "pip_install",
                    "ok": true,
                    "message": "Installed pyautogui, pillow, pynput",
                })),
                Ok(Ok(out)) => {
                    success = false;
                    steps.push(serde_json::json!({
                        "name": "pip_install",
                        "ok": false,
                        "message": String::from_utf8_lossy(&out.stderr).into_owned(),
                    }));
                }
                _ => {
                    success = false;
                    steps.push(serde_json::json!({
                        "name": "pip_install",
                        "ok": false,
                        "message": "Failed to spawn pip install",
                    }));
                }
            }
        } else {
            success = false;
            steps.push(serde_json::json!({
                "name": "pip_install",
                "ok": false,
                "message": format!("pip not found at {}", pip.to_string_lossy()),
            }));
        }
    }

    Json(serde_json::json!({ "success": success, "steps": steps })).into_response()
}

pub async fn handle_computer_use_apps(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    Json(serde_json::json!({ "apps": [] })).into_response()
}

pub async fn handle_computer_use_authorized_apps_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    Json(serde_json::json!({
        "authorizedApps": [],
        "grantFlags": {
            "clipboardRead": false,
            "clipboardWrite": false,
            "systemKeyCombos": false,
        },
    }))
    .into_response()
}

pub async fn handle_computer_use_authorized_apps_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(_body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    Json(serde_json::json!({ "ok": true })).into_response()
}

pub async fn handle_computer_use_open_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let pane = body
        .get("pane")
        .and_then(|v| v.as_str())
        .unwrap_or("Privacy_Accessibility");
    #[cfg(target_os = "macos")]
    {
        let url = match pane {
            "Privacy_ScreenCapture" => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
            }
            _ => "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
        };
        let _ = crate::util::hidden_sync_command("open").arg(url).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = pane;
        let _ = crate::util::hidden_sync_command("cmd")
            .args(["/C", "start", "ms-settings:privacy"])
            .spawn();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = pane;
    }
    Json(serde_json::json!({ "ok": true })).into_response()
}

pub async fn handle_haha_oauth_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    Json(serde_json::json!({
        "loggedIn": false,
        "enabled": false,
        "supported": false,
    }))
    .into_response()
}

pub async fn handle_haha_oauth_start(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(_body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({
            "error": "Haha OAuth is not configured for this build.",
        })),
    )
        .into_response()
}

pub async fn handle_haha_oauth_logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    Json(serde_json::json!({ "ok": true })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct FilesystemBrowseQuery {
    pub path: Option<String>,
    #[serde(rename = "includeFiles")]
    pub include_files: Option<bool>,
    pub search: Option<String>,
    #[serde(rename = "maxResults")]
    pub max_results: Option<usize>,
}

pub async fn handle_filesystem_browse(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<FilesystemBrowseQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    let raw_path = q
        .path
        .clone()
        .unwrap_or_else(|| config.workspace_dir.to_string_lossy().to_string());
    let current_path = PathBuf::from(&raw_path);
    let parent_path = current_path
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let include_files = q.include_files.unwrap_or(false) || q.search.is_some();

    let mut entries: Vec<serde_json::Value> = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(&current_path) {
        for entry in read_dir.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let is_dir = file_type.is_dir();
            if !is_dir && !include_files {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            if let Some(needle) = q.search.as_ref()
                && !name.to_ascii_lowercase().contains(&needle.to_ascii_lowercase())
            {
                continue;
            }
            entries.push(serde_json::json!({
                "name": name,
                "path": entry.path().to_string_lossy().to_string(),
                "isDirectory": is_dir,
            }));
        }
    }
    entries.sort_by(|a, b| {
        let a_dir = a.get("isDirectory").and_then(|v| v.as_bool()).unwrap_or(false);
        let b_dir = b.get("isDirectory").and_then(|v| v.as_bool()).unwrap_or(false);
        let a_name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let b_name = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
        b_dir.cmp(&a_dir).then(a_name.cmp(b_name))
    });
    if let Some(max) = q.max_results
        && entries.len() > max
    {
        entries.truncate(max);
    }
    Json(serde_json::json!({
        "currentPath": raw_path,
        "parentPath": parent_path,
        "entries": entries,
        "query": q.search,
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
pub struct SearchBody {
    pub query: String,
    pub cwd: Option<String>,
    #[serde(rename = "maxResults")]
    pub max_results: Option<usize>,
}

pub async fn handle_search_files(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SearchBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    let cwd = body
        .cwd
        .map(PathBuf::from)
        .unwrap_or_else(|| config.workspace_dir.clone());
    let max_results = body.max_results.unwrap_or(200);
    let needle = body.query.to_ascii_lowercase();

    if needle.is_empty() {
        return Json(serde_json::json!({ "results": [], "total": 0 })).into_response();
    }

    let results = tokio::task::spawn_blocking(move || {
        let mut results: Vec<serde_json::Value> = Vec::new();
        walk_for_search(&cwd, &needle, &mut results, max_results);
        results
    })
    .await
    .unwrap_or_default();

    let total = results.len();
    Json(serde_json::json!({ "results": results, "total": total })).into_response()
}

fn walk_for_search(
    root: &std::path::Path,
    needle: &str,
    out: &mut Vec<serde_json::Value>,
    cap: usize,
) {
    if out.len() >= cap {
        return;
    }
    let Ok(read_dir) = std::fs::read_dir(root) else {
        return;
    };
    for entry in read_dir.flatten() {
        if out.len() >= cap {
            return;
        }
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            walk_for_search(&path, needle, out, cap);
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if metadata.len() > 256 * 1024 {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (line_idx, line) in content.lines().enumerate() {
            if out.len() >= cap {
                return;
            }
            if line.to_ascii_lowercase().contains(needle) {
                out.push(serde_json::json!({
                    "file": path.to_string_lossy().to_string(),
                    "line": (line_idx + 1) as u64,
                    "text": line,
                }));
            }
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SessionsSearchBody {
    pub query: String,
}

pub async fn handle_search_sessions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SessionsSearchBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(ref backend) = state.session_backend else {
        return Json(serde_json::json!({ "results": [] })).into_response();
    };
    let needle = body.query.to_ascii_lowercase();
    let backend_cloned = backend.clone();
    let results = tokio::task::spawn_blocking(move || {
        let mut results: Vec<serde_json::Value> = Vec::new();
        for meta in backend_cloned.list_sessions_with_metadata() {
            let Some(session_id) = meta.key.strip_prefix("gw_") else {
                continue;
            };
            let messages = backend_cloned.load(&meta.key);
            let mut matches: Vec<serde_json::Value> = Vec::new();
            for (i, msg) in messages.iter().enumerate() {
                if msg.content.to_ascii_lowercase().contains(&needle) {
                    matches.push(serde_json::json!({
                        "line": (i + 1) as u64,
                        "text": msg.content.lines().next().unwrap_or("").to_string(),
                    }));
                }
            }
            if matches.is_empty() {
                continue;
            }
            let title = meta.name.clone().unwrap_or_default();
            results.push(serde_json::json!({
                "sessionId": session_id,
                "title": title,
                "matchCount": matches.len(),
                "matches": matches,
            }));
        }
        results
    })
    .await
    .unwrap_or_default();
    Json(serde_json::json!({ "results": results })).into_response()
}

fn desktop_user_settings_path(state: &AppState) -> PathBuf {
    let config_path = state.config.lock().config_path.clone();
    config_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
        .join("desktop_user.json")
}

pub async fn handle_settings_user_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let path = desktop_user_settings_path(&state);
    let parsed = tokio::task::spawn_blocking(move || {
        let body = std::fs::read_to_string(&path).unwrap_or_else(|_| "{}".to_string());
        serde_json::from_str::<serde_json::Value>(&body).unwrap_or_else(|_| serde_json::json!({}))
    })
    .await
    .unwrap_or_else(|_| serde_json::json!({}));
    Json(parsed).into_response()
}

pub async fn handle_settings_user_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let path = desktop_user_settings_path(&state);
    let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let mut existing: serde_json::Value = match std::fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| serde_json::json!({})),
            Err(_) => serde_json::json!({}),
        };
        if !existing.is_object() {
            existing = serde_json::json!({});
        }
        let mut body = body;
        if let Some(patch) = body.as_object() {
            if let Some(obj) = existing.as_object_mut() {
                for (k, v) in patch {
                    obj.insert(k.clone(), v.clone());
                }
            }
        } else if body.is_object() || body.is_null() {

        } else {
            std::mem::swap(&mut existing, &mut body);
        }
        let serialized = serde_json::to_string_pretty(&existing)
            .unwrap_or_else(|_| existing.to_string());
        std::fs::write(&path, serialized).map_err(|e| format!("write settings: {e}"))
    })
    .await;
    match result {
        Ok(Ok(())) => Json(serde_json::json!({ "ok": true })).into_response(),
        Ok(Err(msg)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "blocking task join failed" })),
        )
            .into_response(),
    }
}

pub async fn handle_permissions_mode_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let mode = super::ws_desktop::desktop_runtime_state().permission_mode();
    Json(serde_json::json!({ "mode": mode })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct SetPermissionModeBody {
    pub mode: String,
}

pub async fn handle_permissions_mode_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SetPermissionModeBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    super::ws_desktop::desktop_runtime_state().set_permission_mode(&body.mode);
    Json(serde_json::json!({ "ok": true, "mode": body.mode })).into_response()
}

fn autonomy_view_json(cfg: &crate::config::schema::Config) -> serde_json::Value {
    serde_json::json!({
        "autoApprove": cfg.autonomy.auto_approve.clone(),
        "alwaysAsk": cfg.autonomy.always_ask.clone(),
        "protectBrowserTools": cfg.autonomy.protect_browser_tools,
        "protectMcpTools": cfg.autonomy.protect_mcp_tools,
        "autoApproveModeTransitions": cfg.autonomy.auto_approve_mode_transitions.clone(),
        "enableCommandPolicy": cfg.autonomy.enable_command_policy,
    })
}

pub async fn handle_permissions_autonomy_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let view = match crate::services::try_get_services() {
        Some(svc) => autonomy_view_json(&svc.config()),
        None => serde_json::json!({
            "autoApprove": Vec::<String>::new(),
            "alwaysAsk": Vec::<String>::new(),
            "protectBrowserTools": true,
            "protectMcpTools": true,
            "autoApproveModeTransitions": Vec::<String>::new(),
            "enableCommandPolicy": false,
        }),
    };
    Json(view).into_response()
}

#[derive(Debug, Deserialize)]
pub struct SetAutonomyBody {
    #[serde(default, rename = "autoApprove")]
    pub auto_approve: Option<Vec<String>>,
    #[serde(default, rename = "alwaysAsk")]
    pub always_ask: Option<Vec<String>>,
    #[serde(default, rename = "protectBrowserTools")]
    pub protect_browser_tools: Option<bool>,
    #[serde(default, rename = "protectMcpTools")]
    pub protect_mcp_tools: Option<bool>,
    #[serde(default, rename = "autoApproveModeTransitions")]
    pub auto_approve_mode_transitions: Option<Vec<String>>,
    #[serde(default, rename = "enableCommandPolicy")]
    pub enable_command_policy: Option<bool>,
}

pub async fn handle_permissions_autonomy_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SetAutonomyBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(svc) = crate::services::try_get_services() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "service container not initialized" })),
        )
            .into_response();
    };

    let mut next_cfg: crate::config::schema::Config = (*svc.config()).clone();
    if let Some(v) = body.auto_approve {
        next_cfg.autonomy.auto_approve = v;
    }
    if let Some(v) = body.always_ask {
        next_cfg.autonomy.always_ask = v;
    }
    if let Some(v) = body.protect_browser_tools {
        next_cfg.autonomy.protect_browser_tools = v;
    }
    if let Some(v) = body.protect_mcp_tools {
        next_cfg.autonomy.protect_mcp_tools = v;
    }
    if let Some(v) = body.auto_approve_mode_transitions {
        next_cfg.autonomy.auto_approve_mode_transitions = v;
    }
    if let Some(v) = body.enable_command_policy {
        next_cfg.autonomy.enable_command_policy = v;
    }

    if let Err(e) = next_cfg.save().await {
        tracing::error!(
            target: "gateway.permissions",
            error = %e,
            "autonomy config save failed"
        );
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "config_save_failed",
                "detail": format!("{e:#}"),
            })),
        )
            .into_response();
    }
    svc.update_config(next_cfg.clone());
    *state.config.lock() = next_cfg.clone();
    state.push_live_config(next_cfg.clone());

    Json(autonomy_view_json(&next_cfg)).into_response()
}

pub async fn handle_coding_modes_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let modes: Vec<serde_json::Value> = crate::agent::coding_mode::CodingMode::visible()
        .iter()
        .map(|m| {
            let allowed = build_allowed_tools_for_mode(*m);
            let profile = m.resource_profile();
            serde_json::json!({
                "id": m.display_name(),
                "label": m.label(),
                "description": m.description(),
                "icon": m.icon(),
                "permissionMode": derive_permission_from_coding(m),
                "allowedTools": allowed,
                "resourceProfile": {
                    "browser": profile.browser,
                    "shell": profile.shell,
                    "mayWrite": profile.may_write,
                },
            })
        })
        .collect();
    Json(serde_json::json!({ "modes": modes })).into_response()
}

pub async fn handle_coding_mode_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let mode = match crate::services::try_get_services() {
        Some(svc) => *svc.coding_mode.read(),
        None => crate::agent::coding_mode::CodingMode::default(),
    };
    let profile = mode.resource_profile();
    Json(serde_json::json!({
        "mode": mode.display_name(),
        "label": mode.label(),
        "description": mode.description(),
        "icon": mode.icon(),
        "permissionMode": derive_permission_from_coding(&mode),
        "allowedTools": build_allowed_tools_for_mode(mode),
        "resourceProfile": {
            "browser": profile.browser,
            "shell": profile.shell,
            "mayWrite": profile.may_write,
        },
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
pub struct SetCodingModeBody {
    pub mode: String,
    #[serde(default)]
    pub confirmed: bool,
}

pub async fn handle_coding_mode_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SetCodingModeBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(parsed) = crate::agent::coding_mode::CodingMode::from_str_loose(&body.mode) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("unknown coding mode: {}", body.mode),
            })),
        )
            .into_response();
    };
    let svc_opt = crate::services::try_get_services();
    let previous_mode = svc_opt
        .map(|svc| *svc.coding_mode.read())
        .unwrap_or_default();
    let cfg = svc_opt
        .map(|svc| svc.config())
        .unwrap_or_else(|| std::sync::Arc::new(state.config.lock().clone()));
    let whitelist: &[String] = cfg.autonomy.auto_approve_mode_transitions.as_slice();
    let auto_approved = crate::agent::mode_transition::is_auto_approved(
        whitelist,
        previous_mode,
        parsed,
    );
    let needs_confirm = !body.confirmed && previous_mode != parsed && !auto_approved;
    if needs_confirm {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "confirmation_required",
                "confirmationRequired": true,
                "from": previous_mode.display_name(),
                "to": parsed.display_name(),
                "whitelist": whitelist,
                "message": format!(
                    "Switching coding mode {} -> {} is not in the autonomy auto-approve list; resubmit with `confirmed: true` to apply",
                    previous_mode.display_name(),
                    parsed.display_name(),
                ),
            })),
        )
            .into_response();
    }
    if let Some(svc) = svc_opt {
        *svc.coding_mode.write() = parsed;
    }
    let permission = derive_permission_from_coding(&parsed);
    super::ws_desktop::desktop_runtime_state().set_permission_mode(permission);
    Json(serde_json::json!({
        "ok": true,
        "mode": parsed.display_name(),
        "from": previous_mode.display_name(),
        "permissionMode": permission,
        "allowedTools": build_allowed_tools_for_mode(parsed),
        "autoApproved": previous_mode == parsed || auto_approved,
    }))
    .into_response()
}

pub fn derive_permission_from_coding(mode: &crate::agent::coding_mode::CodingMode) -> &'static str {
    use crate::agent::coding_mode::CodingMode;
    match mode {
        CodingMode::Plan | CodingMode::Ask => "plan",
        CodingMode::Agent | CodingMode::Harness => "bypassPermissions",
        _ => "acceptEdits",
    }
}

fn build_allowed_tools_for_mode(
    mode: crate::agent::coding_mode::CodingMode,
) -> Vec<String> {
    if let Some(allow) = mode.allowed_tools() {
        let mut list: Vec<String> = allow.iter().map(|s| (*s).to_string()).collect();
        list.sort();
        list.dedup();
        list
    } else {
        Vec::new()
    }
}

pub async fn handle_settings_cli_launcher(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let installed = which::which("sen").is_ok();
    let launcher_path = which::which("sen")
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    Json(serde_json::json!({
        "supported": true,
        "command": "sen",
        "installed": installed,
        "launcherPath": launcher_path,
        "binDir": String::new(),
        "pathConfigured": installed,
        "pathInCurrentShell": installed,
        "availableInNewTerminals": installed,
        "needsTerminalRestart": false,
        "configTarget": serde_json::Value::Null,
        "lastError": serde_json::Value::Null,
    }))
    .into_response()
}

pub async fn handle_scheduled_tasks_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    let jobs = match crate::cron::list_jobs(&config) {
        Ok(j) => j,
        Err(_) => return Json(serde_json::json!({ "tasks": [] })).into_response(),
    };
    let tasks: Vec<serde_json::Value> = jobs.into_iter().map(cron_job_to_payload).collect();
    Json(serde_json::json!({ "tasks": tasks })).into_response()
}

fn cron_job_to_payload(j: crate::cron::CronJob) -> serde_json::Value {
    serde_json::json!({
        "id": j.id,
        "name": j.name.clone().unwrap_or_else(|| j.id.clone()),
        "description": j.task_description.clone().unwrap_or_default(),
        "cron": cron_schedule_string(&j.schedule),
        "schedule": cron_schedule_string(&j.schedule),
        "type": if j.prompt.is_some() { "agent" } else { "shell" },
        "command": j.command,
        "prompt": j.prompt.clone().unwrap_or_default(),
        "enabled": j.enabled,
        "recurring": matches!(j.schedule, crate::cron::Schedule::Cron { .. } | crate::cron::Schedule::Every { .. }),
        "permanent": !j.delete_after_run,
        "createdAt": j.created_at.timestamp_millis(),
        "lastRunAt": j.last_run.map(|t| t.timestamp_millis()),
        "lastFiredAt": j.last_run.map(|t| t.to_rfc3339()),
        "nextRunAt": j.next_run.timestamp_millis(),
        "lastResult": j.last_status,
        "nextRun": j.next_run.to_rfc3339(),
        "model": j.model,
        "deleteAfterRun": j.delete_after_run,
        "permissionMode": j.permission_mode.clone(),
        "codingMode": j.coding_mode.clone(),
        "folderPath": j.folder_path.clone(),
        "useWorktree": j.use_worktree.unwrap_or(false),
        "notification": j.notification.clone(),
    })
}

fn cron_schedule_string(schedule: &crate::cron::Schedule) -> String {
    match schedule {
        crate::cron::Schedule::Cron { expr, .. } => expr.clone(),
        crate::cron::Schedule::At { at } => at.to_rfc3339(),
        other => format!("{other:?}"),
    }
}

fn map_cron_run_status(raw: &str) -> &'static str {
    match raw {
        "ok" | "success" | "completed" => "completed",
        "running" => "running",
        "timeout" => "timeout",
        _ => "failed",
    }
}

fn cron_run_to_payload(r: crate::cron::CronRun) -> serde_json::Value {
    let status = map_cron_run_status(&r.status);
    let is_running = status == "running";
    let completed_at = if is_running {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(r.finished_at.to_rfc3339())
    };
    let ended_at = completed_at.clone();
    serde_json::json!({
        "id": r.id,
        "taskId": r.job_id,
        "startedAt": r.started_at.to_rfc3339(),
        "endedAt": ended_at,
        "completedAt": completed_at,
        "status": status,
        "ok": status == "completed",
        "summary": r.output.clone(),
        "output": r.output,
        "durationMs": if is_running { None } else { r.duration_ms },
    })
}

pub async fn handle_scheduled_tasks_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let schedule_expr = body
        .get("cron")
        .and_then(|v| v.as_str())
        .or_else(|| body.get("schedule").and_then(|v| v.as_str()))
        .unwrap_or("0 0 * * *")
        .to_string();
    let schedule = crate::cron::Schedule::Cron {
        expr: schedule_expr,
        tz: None,
    };
    let prompt = body
        .get("prompt")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    let command = body
        .get("command")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let delete_after_run = body
        .get("permanent")
        .and_then(|v| v.as_bool())
        .map(|p| !p)
        .or_else(|| body.get("deleteAfterRun").and_then(|v| v.as_bool()))
        .unwrap_or(false);

    let task_description = body
        .get("description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let permission_mode = body
        .get("permissionMode")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let coding_mode = body
        .get("codingMode")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let folder_path = body
        .get("folderPath")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let use_worktree = body.get("useWorktree").and_then(|v| v.as_bool());

    let notification = body.get("notification").cloned();

    let job = if let Some(prompt) = prompt {
        crate::cron::add_agent_job(
            &config,
            name,
            schedule,
            &prompt,
            crate::cron::AgentJobOptions {
                session_target: crate::cron::SessionTarget::default(),
                model,
                delivery: None,
                delete_after_run,
                allowed_tools: None,
                permission_mode,
                coding_mode,
                folder_path,
                use_worktree,
                notification,
                task_description,
            },
        )
    } else if let Some(command) = command {
        crate::cron::add_shell_job_with_approval(
            &config,
            name,
            schedule,
            &command,
            None,
            delete_after_run,
        )
    } else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "missing prompt or command"})),
        )
            .into_response();
    };

    match job {
        Ok(j) => Json(serde_json::json!({ "task": cron_job_to_payload(j) })).into_response(),
        Err(e) => {
            tracing::error!("scheduled-tasks create failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "failed to create task"})),
            )
                .into_response()
        }
    }
}

pub async fn handle_scheduled_tasks_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    let mut patch = crate::cron::CronJobPatch::default();
    if let Some(expr) = body
        .get("cron")
        .and_then(|v| v.as_str())
        .or_else(|| body.get("schedule").and_then(|v| v.as_str()))
    {
        patch.schedule = Some(crate::cron::Schedule::Cron {
            expr: expr.to_string(),
            tz: None,
        });
    }
    if let Some(name) = body.get("name").and_then(|v| v.as_str()) {
        patch.name = Some(name.to_string());
    }
    if let Some(prompt) = body.get("prompt").and_then(|v| v.as_str()) {
        patch.prompt = Some(prompt.to_string());
    }
    if let Some(command) = body.get("command").and_then(|v| v.as_str()) {
        patch.command = Some(command.to_string());
    }
    if let Some(enabled) = body.get("enabled").and_then(|v| v.as_bool()) {
        patch.enabled = Some(enabled);
    }
    if let Some(model) = body.get("model").and_then(|v| v.as_str()) {
        patch.model = Some(model.to_string());
    }
    if let Some(permanent) = body.get("permanent").and_then(|v| v.as_bool()) {
        patch.delete_after_run = Some(!permanent);
    } else if let Some(delete_after) = body.get("deleteAfterRun").and_then(|v| v.as_bool()) {
        patch.delete_after_run = Some(delete_after);
    }

    if let Some(description) = body.get("description").and_then(|v| v.as_str()) {
        patch.task_description = Some(description.to_string());
    }
    if let Some(v) = body.get("permissionMode").and_then(|x| x.as_str()) {
        patch.permission_mode = Some(v.to_string());
    }
    if let Some(v) = body.get("codingMode").and_then(|x| x.as_str()) {
        patch.coding_mode = Some(v.to_string());
    }
    if let Some(v) = body.get("folderPath").and_then(|x| x.as_str()) {
        patch.folder_path = Some(v.to_string());
    }
    if let Some(v) = body.get("useWorktree").and_then(|x| x.as_bool()) {
        patch.use_worktree = Some(v);
    }
    if let Some(v) = body.get("notification") {
        patch.notification = Some(v.clone());
    }

    match crate::cron::update_job(&config, &id, patch) {
        Ok(j) => Json(serde_json::json!({ "task": cron_job_to_payload(j) })).into_response(),
        Err(e) => {
            tracing::error!("scheduled-tasks update failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("update failed: {e}") })),
            )
                .into_response()
        }
    }
}

pub async fn handle_scheduled_tasks_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    match crate::cron::remove_job(&config, &id) {
        Ok(_) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => {
            tracing::error!("scheduled-tasks delete failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "failed to delete task"})),
            )
                .into_response()
        }
    }
}

pub async fn handle_scheduled_tasks_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    let jobs = match crate::cron::list_jobs(&config) {
        Ok(j) => j,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("list jobs failed: {e}") })),
            )
                .into_response();
        }
    };
    let Some(job) = jobs.into_iter().find(|j| j.id == id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "task not found"})),
        )
            .into_response();
    };

    let cfg_clone = config.clone();
    crate::runtime::task_manager::spawn_supervised(
        "scheduled_tasks.manual_run",
        async move {
            let (success, output) =
                crate::cron::scheduler::execute_job_now_and_record(&cfg_clone, &job).await;
            tracing::info!(
                "scheduled task {} run completed: success={} bytes={}",
                job.id,
                success,
                output.len()
            );
        },
    );
    Json(serde_json::json!({ "ok": true, "started": true })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct ScheduledRunsQuery {
    pub limit: Option<usize>,
}

pub async fn handle_scheduled_tasks_runs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ScheduledRunsQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    let limit = q.limit.unwrap_or(50);
    let mut all_runs: Vec<crate::cron::CronRun> = Vec::new();
    if let Ok(jobs) = crate::cron::list_jobs(&config) {
        for job in jobs {
            if let Ok(mut runs) = crate::cron::list_runs(&config, &job.id, limit) {
                all_runs.append(&mut runs);
            }
        }
    }
    all_runs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    all_runs.truncate(limit);
    let runs_json: Vec<serde_json::Value> = all_runs
        .into_iter()
        .map(cron_run_to_payload)
        .collect();
    Json(serde_json::json!({ "runs": runs_json })).into_response()
}

pub async fn handle_scheduled_tasks_task_runs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    let runs = crate::cron::list_runs(&config, &id, 50).unwrap_or_default();
    let runs_json: Vec<serde_json::Value> = runs
        .into_iter()
        .map(cron_run_to_payload)
        .collect();
    Json(serde_json::json!({ "runs": runs_json })).into_response()
}

fn snapshot_session_todos(list_id: &str) -> Vec<crate::tools::todo_write::TodoItem> {
    if let Some(svc) = crate::services::try_get_services() {
        crate::tools::todo_write::session_todos(&svc.todo_store, list_id)
    } else {
        Vec::new()
    }
}

fn snapshot_all_sessions(
) -> std::collections::HashMap<String, Vec<crate::tools::todo_write::TodoItem>> {
    if let Some(svc) = crate::services::try_get_services() {
        crate::tools::todo_write::snapshot_all(&svc.todo_store)
    } else {
        std::collections::HashMap::new()
    }
}

fn todo_status_to_task_status(status: &crate::tools::todo_write::TodoStatus) -> &'static str {
    use crate::tools::todo_write::TodoStatus;
    match status {
        TodoStatus::Pending => "pending",
        TodoStatus::InProgress => "in_progress",
        TodoStatus::Completed | TodoStatus::Cancelled => "completed",
    }
}

fn render_cli_tasks_from(
    list_id: &str,
    todos: &[crate::tools::todo_write::TodoItem],
) -> Vec<serde_json::Value> {
    todos
        .iter()
        .enumerate()
        .map(|(idx, todo)| {
            serde_json::json!({
                "id": if todo.id.trim().is_empty() {
                    (idx + 1).to_string()
                } else {
                    todo.id.clone()
                },
                "subject": todo.content,
                "description": "",
                "status": todo_status_to_task_status(&todo.status),
                "activeForm": serde_json::Value::Null,
                "owner": serde_json::Value::Null,
                "blocks": serde_json::Value::Array(Vec::new()),
                "blockedBy": serde_json::Value::Array(Vec::new()),
                "taskListId": list_id,
            })
        })
        .collect()
}

fn render_cli_tasks_for(list_id: &str) -> Vec<serde_json::Value> {
    let todos = snapshot_session_todos(list_id);
    render_cli_tasks_from(list_id, &todos)
}

fn summarize_todo_list(
    list_id: &str,
    todos: &[crate::tools::todo_write::TodoItem],
) -> serde_json::Value {
    use crate::tools::todo_write::TodoStatus;
    let total = todos.len();
    let completed = todos
        .iter()
        .filter(|t| matches!(t.status, TodoStatus::Completed | TodoStatus::Cancelled))
        .count();
    let in_progress = todos
        .iter()
        .filter(|t| matches!(t.status, TodoStatus::InProgress))
        .count();
    let pending = total.saturating_sub(completed).saturating_sub(in_progress);
    serde_json::json!({
        "id": list_id,
        "taskCount": total,
        "completedCount": completed,
        "inProgressCount": in_progress,
        "pendingCount": pending,
    })
}

pub async fn handle_cli_task_lists(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let by_session = snapshot_all_sessions();
    let mut lists: Vec<serde_json::Value> = Vec::with_capacity(by_session.len());
    for (list_id, todos) in by_session.iter() {
        if todos.is_empty() {
            continue;
        }
        lists.push(summarize_todo_list(list_id, todos));
    }
    Json(serde_json::json!({ "lists": lists })).into_response()
}

pub async fn handle_cli_tasks_for_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(list_id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let tasks = render_cli_tasks_for(&list_id);
    Json(serde_json::json!({ "tasks": tasks })).into_response()
}

pub async fn handle_cli_task_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(_p): Path<(String, String)>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "task not found" })),
    )
        .into_response()
}

pub async fn handle_cli_tasks_reset(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(list_id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    if let Some(svc) = crate::services::try_get_services() {
        crate::tools::todo_write::clear_session(&svc.todo_store, &list_id);
    }
    Json(serde_json::json!({ "ok": true })).into_response()
}

pub async fn handle_cli_tasks_list_all(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let by_session = snapshot_all_sessions();
    let mut tasks: Vec<serde_json::Value> = Vec::new();
    for (list_id, todos) in by_session.iter() {
        tasks.extend(render_cli_tasks_from(list_id, todos));
    }
    Json(serde_json::json!({ "tasks": tasks })).into_response()
}

pub async fn handle_conversations_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    Json(serde_json::json!({ "conversations": [] })).into_response()
}

pub async fn handle_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "provider": config.default_provider.clone(),
        "model": config.default_model.clone(),
        "workspaceDir": config.workspace_dir.to_string_lossy().to_string(),
    }))
    .into_response()
}

pub async fn handle_runtime_snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock().clone();
    let started_at = crate::runtime::task_manager::process_started_at();
    let uptime_secs = crate::runtime::task_manager::process_uptime_secs();

    let now = std::time::Instant::now();
    let mut buckets: std::collections::BTreeMap<String, (u64, Option<u64>)> =
        std::collections::BTreeMap::new();
    for info in crate::runtime::task_manager::snapshot() {
        let elapsed_ms = now.saturating_duration_since(info.spawned_at).as_millis() as u64;
        let entry = buckets.entry(info.name.clone()).or_insert((0, None));
        entry.0 += 1;
        entry.1 = Some(entry.1.map_or(elapsed_ms, |existing| existing.max(elapsed_ms)));
    }
    let task_groups: Vec<serde_json::Value> = buckets
        .into_iter()
        .map(|(name, (count, oldest_ms))| {
            serde_json::json!({
                "name": name,
                "count": count,
                "oldestAgeMs": oldest_ms.unwrap_or(0),
            })
        })
        .collect();

    let listen_host = config.gateway.host.clone();
    let listen_port = config.gateway.port;
    let public_url = if listen_host.is_empty() {
        format!("http://127.0.0.1:{listen_port}")
    } else {
        format!("http://{listen_host}:{listen_port}")
    };

    let process_id = std::process::id();
    let cpu_count = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(0);

    Json(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "buildProfile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "pid": process_id,
        "cpuCount": cpu_count,
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "startedAt": started_at.to_rfc3339(),
        "uptimeSecs": uptime_secs,
        "workspaceDir": config.workspace_dir.to_string_lossy().to_string(),
        "defaultProvider": config.default_provider.clone(),
        "defaultModel": config.default_model.clone(),
        "gateway": {
            "host": listen_host,
            "port": listen_port,
            "url": public_url,
            "pathPrefix": state.path_prefix.clone(),
        },
        "tasks": {
            "liveCount": crate::runtime::task_manager::live_count() as u64,
            "groups": task_groups,
        },
    }))
    .into_response()
}

fn build_hooks_payload(config: &crate::config::Config) -> serde_json::Value {
    let snake = serde_json::to_value(&config.hooks).unwrap_or_else(|_| serde_json::json!({}));
    let mut value = to_camel_case_keys(snake);
    let candidates = [
        config.workspace_dir.join(".cursor").join("hooks.json"),
        config.workspace_dir.join(".sen").join("hooks.json"),
    ];
    let mut script_paths: Vec<String> = candidates
        .into_iter()
        .map(|p| p.display().to_string())
        .collect();
    if let Some(home) = directories::UserDirs::new().map(|u| u.home_dir().to_path_buf()) {
        script_paths.push(home.join(".cursor").join("hooks.json").display().to_string());
        script_paths.push(home.join(".sen").join("hooks.json").display().to_string());
    }
    if let serde_json::Value::Object(ref mut map) = value {
        map.insert(
            "scriptHookPaths".to_string(),
            serde_json::Value::Array(
                script_paths
                    .into_iter()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );
    }
    value
}

pub async fn handle_hooks_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    Json(build_hooks_payload(&config)).into_response()
}

pub async fn handle_hooks_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let snapshot = {
        let mut cfg = state.config.lock();
        let current = serde_json::to_value(&cfg.hooks)
            .unwrap_or_else(|_| serde_json::json!({}));
        let mut merged_camel = to_camel_case_keys(current);
        if let Some(obj) = body.as_object() {
            let mut filtered = serde_json::Map::with_capacity(obj.len());
            for (k, v) in obj {
                if k == "scriptHookPaths" {
                    continue;
                }
                filtered.insert(k.clone(), v.clone());
            }
            deep_merge(
                &mut merged_camel,
                serde_json::Value::Object(filtered),
            );
        }
        let snake = to_snake_case_keys(merged_camel);
        match serde_json::from_value::<crate::config::HooksConfig>(snake) {
            Ok(parsed) => cfg.hooks = parsed,
            Err(err) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": format!("invalid hooks payload: {err}")
                    })),
                )
                    .into_response();
            }
        }
        cfg.clone()
    };

    if let Err(e) = snapshot.save().await {
        tracing::error!("Failed to save config (hooks): {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to save configuration"})),
        )
            .into_response();
    }
    *state.config.lock() = snapshot.clone();

    state.push_live_config(snapshot.clone());
    let workspace_anchor = if snapshot.workspace_dir.as_os_str().is_empty() {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    } else {
        snapshot.workspace_dir.clone()
    };
    state.hooks.rebuild(&snapshot, &workspace_anchor);
    Json(build_hooks_payload(&snapshot)).into_response()
}

pub async fn handle_guardrails_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let snapshot = {
        let mut cfg = state.config.lock();
        let current = serde_json::to_value(&cfg.guardrails)
            .unwrap_or_else(|_| serde_json::json!({}));
        let mut merged_camel = to_camel_case_keys(current);
        deep_merge(&mut merged_camel, body);
        let snake = to_snake_case_keys(merged_camel);
        match serde_json::from_value::<crate::guardrails::GuardrailsConfig>(snake) {
            Ok(parsed) => cfg.guardrails = parsed,
            Err(err) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": format!("invalid guardrails payload: {err}")
                    })),
                )
                    .into_response();
            }
        }
        cfg.clone()
    };

    if let Err(e) = snapshot.save().await {
        tracing::error!("Failed to save config (guardrails): {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to save configuration"})),
        )
            .into_response();
    }
    *state.config.lock() = snapshot.clone();

    state.push_live_config(snapshot.clone());
    crate::guardrails::ensure_global_guardrails(snapshot.guardrails.clone());

    Json(serde_json::json!({"status": "ok"})).into_response()
}

fn agent_to_camel_json(name: &str, cfg: &crate::config::DelegateAgentConfig) -> serde_json::Value {
    let snake = serde_json::to_value(cfg).unwrap_or_else(|_| serde_json::json!({}));
    let mut camel = to_camel_case_keys(snake);
    if let serde_json::Value::Object(ref mut map) = camel {
        map.insert(
            "name".to_string(),
            serde_json::Value::String(name.to_string()),
        );
    }
    camel
}

pub async fn handle_agent_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    match config.agents.get(name.trim()) {
        Some(cfg) => Json(serde_json::json!({
            "agent": agent_to_camel_json(name.trim(), cfg)
        }))
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "agent not found" })),
        )
            .into_response(),
    }
}

pub async fn handle_agent_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let Some(name) = body
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "agent name is required"})),
        )
            .into_response();
    };

    let snapshot = {
        let mut cfg = state.config.lock();
        if cfg.agents.contains_key(&name) {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": "agent already exists"})),
            )
                .into_response();
        }
        let mut payload_obj = body.as_object().cloned().unwrap_or_default();
        payload_obj.remove("name");
        let snake = to_snake_case_keys(serde_json::Value::Object(payload_obj));
        let parsed: crate::config::DelegateAgentConfig = match serde_json::from_value(snake) {
            Ok(v) => v,
            Err(err) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": format!("invalid agent payload: {err}")
                    })),
                )
                    .into_response();
            }
        };
        let errors = parsed.validate();
        if !errors.is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": errors.join("; ")})),
            )
                .into_response();
        }
        cfg.agents.insert(name.clone(), parsed);
        cfg.clone()
    };

    if let Err(e) = snapshot.save().await {
        tracing::error!("Failed to save config (agents.create): {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to save configuration"})),
        )
            .into_response();
    }

    *state.config.lock() = snapshot.clone();
    state.push_live_config(snapshot.clone());

    let payload = match snapshot.agents.get(&name) {
        Some(cfg) => agent_to_camel_json(&name, cfg),
        None => serde_json::Value::Null,
    };
    (StatusCode::CREATED, Json(serde_json::json!({"agent": payload}))).into_response()
}

pub async fn handle_agent_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let trimmed = name.trim().to_string();
    let snapshot = {
        let mut cfg = state.config.lock();
        let Some(existing) = cfg.agents.get(&trimmed).cloned() else {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "agent not found"})),
            )
                .into_response();
        };
        let current = serde_json::to_value(&existing).unwrap_or_else(|_| serde_json::json!({}));
        let mut merged_camel = to_camel_case_keys(current);
        let mut patch_map = body.as_object().cloned().unwrap_or_default();
        patch_map.remove("name");
        deep_merge(&mut merged_camel, serde_json::Value::Object(patch_map));
        let snake = to_snake_case_keys(merged_camel);
        let parsed: crate::config::DelegateAgentConfig = match serde_json::from_value(snake) {
            Ok(v) => v,
            Err(err) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": format!("invalid agent payload: {err}")
                    })),
                )
                    .into_response();
            }
        };
        let errors = parsed.validate();
        if !errors.is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": errors.join("; ")})),
            )
                .into_response();
        }
        cfg.agents.insert(trimmed.clone(), parsed);
        cfg.clone()
    };

    if let Err(e) = snapshot.save().await {
        tracing::error!("Failed to save config (agents.update): {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to save configuration"})),
        )
            .into_response();
    }

    *state.config.lock() = snapshot.clone();
    state.push_live_config(snapshot.clone());
    let payload = match snapshot.agents.get(&trimmed) {
        Some(cfg) => agent_to_camel_json(&trimmed, cfg),
        None => serde_json::Value::Null,
    };
    Json(serde_json::json!({"agent": payload})).into_response()
}

pub async fn handle_agent_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let trimmed = name.trim().to_string();
    let snapshot = {
        let mut cfg = state.config.lock();
        if cfg.agents.remove(&trimmed).is_none() {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "agent not found"})),
            )
                .into_response();
        }
        cfg.clone()
    };

    if let Err(e) = snapshot.save().await {
        tracing::error!("Failed to save config (agents.delete): {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to save configuration"})),
        )
            .into_response();
    }

    *state.config.lock() = snapshot.clone();
    state.push_live_config(snapshot);
    Json(serde_json::json!({"ok": true})).into_response()
}

fn custom_tool_to_camel_json(def: &crate::config::CustomToolDef) -> serde_json::Value {
    let snake = serde_json::to_value(def).unwrap_or_else(|_| serde_json::json!({}));
    to_camel_case_keys(snake)
}

pub async fn handle_custom_tools_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    let tools: Vec<serde_json::Value> = config
        .custom_tools
        .tools
        .iter()
        .map(custom_tool_to_camel_json)
        .collect();
    Json(serde_json::json!({"tools": tools})).into_response()
}

pub async fn handle_custom_tools_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let snake = to_snake_case_keys(body.clone());
    let parsed: crate::config::CustomToolDef = match serde_json::from_value(snake) {
        Ok(v) => v,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("invalid custom tool payload: {err}")
                })),
            )
                .into_response();
        }
    };
    let errors = parsed.validate();
    if !errors.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": errors.join("; ")})),
        )
            .into_response();
    }

    let snapshot = {
        let mut cfg = state.config.lock();
        if cfg
            .custom_tools
            .tools
            .iter()
            .any(|t| t.name.trim() == parsed.name.trim())
        {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": "custom tool already exists"})),
            )
                .into_response();
        }
        cfg.custom_tools.tools.push(parsed.clone());
        cfg.clone()
    };

    if let Err(e) = snapshot.save().await {
        tracing::error!("Failed to save config (custom_tools.create): {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to save configuration"})),
        )
            .into_response();
    }
    *state.config.lock() = snapshot.clone();
    state.push_live_config(snapshot);

    (
        StatusCode::CREATED,
        Json(serde_json::json!({"tool": custom_tool_to_camel_json(&parsed)})),
    )
        .into_response()
}

pub async fn handle_custom_tools_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let trimmed = name.trim().to_string();
    let (snapshot, updated_idx) = {
        let mut cfg = state.config.lock();
        let Some(idx) = cfg
            .custom_tools
            .tools
            .iter()
            .position(|t| t.name.trim() == trimmed)
        else {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "custom tool not found"})),
            )
                .into_response();
        };
        let existing = cfg.custom_tools.tools[idx].clone();
        let current = serde_json::to_value(&existing).unwrap_or_else(|_| serde_json::json!({}));
        let mut merged_camel = to_camel_case_keys(current);
        deep_merge(&mut merged_camel, body);
        let snake = to_snake_case_keys(merged_camel);
        let parsed: crate::config::CustomToolDef = match serde_json::from_value(snake) {
            Ok(v) => v,
            Err(err) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": format!("invalid custom tool payload: {err}")
                    })),
                )
                    .into_response();
            }
        };
        let errors = parsed.validate();
        if !errors.is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": errors.join("; ")})),
            )
                .into_response();
        }
        if parsed.name.trim() != trimmed
            && cfg
                .custom_tools
                .tools
                .iter()
                .enumerate()
                .any(|(i, t)| i != idx && t.name.trim() == parsed.name.trim())
        {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": "another custom tool with that name already exists"})),
            )
                .into_response();
        }
        cfg.custom_tools.tools[idx] = parsed;
        (cfg.clone(), idx)
    };

    if let Err(e) = snapshot.save().await {
        tracing::error!("Failed to save config (custom_tools.update): {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to save configuration"})),
        )
            .into_response();
    }
    *state.config.lock() = snapshot.clone();
    state.push_live_config(snapshot.clone());

    let updated = snapshot
        .custom_tools
        .tools
        .get(updated_idx)
        .map(custom_tool_to_camel_json)
        .unwrap_or(serde_json::Value::Null);
    Json(serde_json::json!({"tool": updated})).into_response()
}

pub async fn handle_custom_tools_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let trimmed = name.trim().to_string();
    let snapshot = {
        let mut cfg = state.config.lock();
        let before = cfg.custom_tools.tools.len();
        cfg.custom_tools
            .tools
            .retain(|t| t.name.trim() != trimmed);
        if cfg.custom_tools.tools.len() == before {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "custom tool not found"})),
            )
                .into_response();
        }
        cfg.clone()
    };

    if let Err(e) = snapshot.save().await {
        tracing::error!("Failed to save config (custom_tools.delete): {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to save configuration"})),
        )
            .into_response();
    }
    *state.config.lock() = snapshot.clone();
    state.push_live_config(snapshot);
    Json(serde_json::json!({"ok": true})).into_response()
}

#[derive(Debug, Deserialize, Default)]
pub struct UsageQuery {
    pub period: Option<String>,
}

pub async fn handle_usage_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<UsageQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let summary_value = match state.cost_tracker {
        Some(ref tracker) => match tracker.get_summary() {
            Ok(summary) => match serde_json::to_value(&summary) {
                Ok(v) => v,
                Err(_) => serde_json::json!({}),
            },
            Err(e) => {
                tracing::error!("usage: cost summary failed: {e}");
                serde_json::json!({})
            }
        },
        None => serde_json::json!({
            "session_cost_usd": 0.0,
            "daily_cost_usd": 0.0,
            "monthly_cost_usd": 0.0,
            "total_tokens": 0,
            "request_count": 0,
            "by_model": {},
        }),
    };

    let include_lifetime = q
        .period
        .as_deref()
        .map(|s| s.eq_ignore_ascii_case("all") || s.eq_ignore_ascii_case("lifetime"))
        .unwrap_or(true);

    let aggregates = if include_lifetime {
        let config = state.config.lock().clone();
        compute_lifetime_by_model_and_session(&config.workspace_dir)
    } else {
        UsageAggregates::empty()
    };

    let mut summary_camel = to_camel_case_keys(summary_value);
    if let serde_json::Value::Object(ref mut map) = summary_camel {
        map.insert("byModelLifetime".to_string(), aggregates.by_model);
        map.insert("bySession".to_string(), aggregates.by_session);
        map.insert("byProvider".to_string(), aggregates.by_provider);
        map.insert("byWorkspace".to_string(), aggregates.by_workspace);
        map.insert("byCodingMode".to_string(), aggregates.by_coding_mode);
        map.insert(
            "tokenRatePerMin".to_string(),
            serde_json::json!(aggregates.token_rate_per_min),
        );
        map.insert(
            "last24hTokens".to_string(),
            serde_json::json!(aggregates.last_24h_tokens),
        );
        map.insert(
            "last24hCostUsd".to_string(),
            serde_json::json!(aggregates.last_24h_cost_usd),
        );
        map.insert(
            "last24hRequests".to_string(),
            serde_json::json!(aggregates.last_24h_requests),
        );
        map.insert(
            "last7dTokens".to_string(),
            serde_json::json!(aggregates.last_7d_tokens),
        );
        map.insert(
            "last7dCostUsd".to_string(),
            serde_json::json!(aggregates.last_7d_cost_usd),
        );
        map.insert(
            "last7dRequests".to_string(),
            serde_json::json!(aggregates.last_7d_requests),
        );
    }
    Json(serde_json::json!({"cost": summary_camel})).into_response()
}

struct UsageAggregates {
    by_model: serde_json::Value,
    by_session: serde_json::Value,
    by_provider: serde_json::Value,
    by_workspace: serde_json::Value,
    by_coding_mode: serde_json::Value,
    token_rate_per_min: f64,
    last_24h_tokens: u64,
    last_24h_cost_usd: f64,
    last_24h_requests: u64,
    last_7d_tokens: u64,
    last_7d_cost_usd: f64,
    last_7d_requests: u64,
}

impl UsageAggregates {
    fn empty() -> Self {
        let empty = || serde_json::Value::Object(serde_json::Map::new());
        Self {
            by_model: empty(),
            by_session: empty(),
            by_provider: empty(),
            by_workspace: empty(),
            by_coding_mode: empty(),
            token_rate_per_min: 0.0,
            last_24h_tokens: 0,
            last_24h_cost_usd: 0.0,
            last_24h_requests: 0,
            last_7d_tokens: 0,
            last_7d_cost_usd: 0.0,
            last_7d_requests: 0,
        }
    }
}

fn provider_from_model(model: &str) -> String {
    let lower = model.to_ascii_lowercase();
    if let Some((prefix, _)) = lower.split_once('/') {
        return prefix.to_string();
    }
    if lower.starts_with("gpt-") || lower.starts_with("o1") || lower.starts_with("o3")
        || lower.starts_with("o4") || lower.starts_with("chatgpt-")
    {
        return "openai".to_string();
    }
    if lower.starts_with("claude") {
        return "anthropic".to_string();
    }
    if lower.starts_with("kimi") || lower.starts_with("moonshot") {
        return "moonshot".to_string();
    }
    if lower.starts_with("deepseek") {
        return "deepseek".to_string();
    }
    if lower.starts_with("qwen") {
        return "qwen".to_string();
    }
    if lower.starts_with("glm") {
        return "zhipu".to_string();
    }
    if lower.starts_with("gemini") {
        return "google".to_string();
    }
    if lower.starts_with("grok") {
        return "xai".to_string();
    }
    if lower.starts_with("mistral") || lower.starts_with("mixtral") {
        return "mistral".to_string();
    }
    if lower.starts_with("llama") {
        return "meta".to_string();
    }
    "other".to_string()
}

fn compute_lifetime_by_model_and_session(
    workspace_dir: &std::path::Path,
) -> UsageAggregates {
    use std::io::{BufRead, BufReader};

    let path = workspace_dir.join("state").join("costs.jsonl");
    if !path.exists() {
        return UsageAggregates::empty();
    }

    #[derive(Default)]
    struct ModelAgg {
        cost_usd: f64,
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: u64,
        request_count: u64,
        first_used: Option<chrono::DateTime<chrono::Utc>>,
        last_used: Option<chrono::DateTime<chrono::Utc>>,
    }

    #[derive(Default)]
    struct SessionAgg {
        cost_usd: f64,
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: u64,
        request_count: u64,
        first_used: Option<chrono::DateTime<chrono::Utc>>,
        last_used: Option<chrono::DateTime<chrono::Utc>>,
        by_model: std::collections::BTreeMap<String, ModelAgg>,
    }

    #[derive(Default)]
    struct ProviderAgg {
        cost_usd: f64,
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: u64,
        request_count: u64,
        models: std::collections::BTreeSet<String>,
        first_used: Option<chrono::DateTime<chrono::Utc>>,
        last_used: Option<chrono::DateTime<chrono::Utc>>,
    }

    #[derive(Default)]
    struct CodingModeAgg {
        cost_usd: f64,
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: u64,
        request_count: u64,
        sessions: std::collections::BTreeSet<String>,
        models: std::collections::BTreeSet<String>,
        first_used: Option<chrono::DateTime<chrono::Utc>>,
        last_used: Option<chrono::DateTime<chrono::Utc>>,
    }

    let mut by_model: std::collections::BTreeMap<String, ModelAgg> =
        std::collections::BTreeMap::new();
    let mut by_session: std::collections::BTreeMap<String, SessionAgg> =
        std::collections::BTreeMap::new();
    let mut by_provider: std::collections::BTreeMap<String, ProviderAgg> =
        std::collections::BTreeMap::new();
    let mut by_coding_mode: std::collections::BTreeMap<String, CodingModeAgg> =
        std::collections::BTreeMap::new();

    let now = chrono::Utc::now();
    let one_minute_ago = now - chrono::Duration::minutes(1);
    let one_hour_ago = now - chrono::Duration::hours(1);
    let twenty_four_hours_ago = now - chrono::Duration::hours(24);
    let seven_days_ago = now - chrono::Duration::days(7);

    let mut last_minute_tokens: u64 = 0;
    let mut last_hour_tokens: u64 = 0;
    let mut last_24h_tokens: u64 = 0;
    let mut last_24h_cost_usd: f64 = 0.0;
    let mut last_24h_requests: u64 = 0;
    let mut last_7d_tokens: u64 = 0;
    let mut last_7d_cost_usd: f64 = 0.0;
    let mut last_7d_requests: u64 = 0;

    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(err) => {
            tracing::warn!(error = %err, "usage: failed to open costs.jsonl");
            return UsageAggregates::empty();
        }
    };
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<crate::cost::types::CostRecord>(trimmed) else {
            continue;
        };
        let ts = record.usage.timestamp;
        let model = record.usage.model.clone();
        let provider = provider_from_model(&model);
        let total = record.usage.total_tokens;
        let cost = record.usage.cost_usd;

        if ts >= one_minute_ago {
            last_minute_tokens += total;
        }
        if ts >= one_hour_ago {
            last_hour_tokens += total;
        }
        if ts >= twenty_four_hours_ago {
            last_24h_tokens += total;
            last_24h_cost_usd += cost;
            last_24h_requests += 1;
        }
        if ts >= seven_days_ago {
            last_7d_tokens += total;
            last_7d_cost_usd += cost;
            last_7d_requests += 1;
        }

        let entry = by_model.entry(model.clone()).or_default();
        entry.cost_usd += cost;
        entry.input_tokens += record.usage.input_tokens;
        entry.output_tokens += record.usage.output_tokens;
        entry.total_tokens += total;
        entry.request_count += 1;
        entry.first_used = Some(entry.first_used.map_or(ts, |existing| existing.min(ts)));
        entry.last_used = Some(entry.last_used.map_or(ts, |existing| existing.max(ts)));

        let provider_entry = by_provider.entry(provider).or_default();
        provider_entry.cost_usd += cost;
        provider_entry.input_tokens += record.usage.input_tokens;
        provider_entry.output_tokens += record.usage.output_tokens;
        provider_entry.total_tokens += total;
        provider_entry.request_count += 1;
        provider_entry.models.insert(model.clone());
        provider_entry.first_used = Some(
            provider_entry
                .first_used
                .map_or(ts, |existing| existing.min(ts)),
        );
        provider_entry.last_used = Some(
            provider_entry
                .last_used
                .map_or(ts, |existing| existing.max(ts)),
        );

        if let Some(coding_mode) = record.coding_mode.as_deref() {
            let trimmed = coding_mode.trim();
            if !trimmed.is_empty() {
                let key = trimmed.to_ascii_lowercase();
                let mode_entry = by_coding_mode.entry(key).or_default();
                mode_entry.cost_usd += cost;
                mode_entry.input_tokens += record.usage.input_tokens;
                mode_entry.output_tokens += record.usage.output_tokens;
                mode_entry.total_tokens += total;
                mode_entry.request_count += 1;
                mode_entry.models.insert(model.clone());
                if let Some(chat_session_id) = record.chat_session_id.as_deref() {
                    mode_entry.sessions.insert(chat_session_id.to_string());
                }
                mode_entry.first_used = Some(
                    mode_entry
                        .first_used
                        .map_or(ts, |existing| existing.min(ts)),
                );
                mode_entry.last_used = Some(
                    mode_entry
                        .last_used
                        .map_or(ts, |existing| existing.max(ts)),
                );
            }
        }

        if let Some(chat_session_id) = record.chat_session_id.as_deref() {
            let session_entry = by_session
                .entry(chat_session_id.to_string())
                .or_default();
            session_entry.cost_usd += cost;
            session_entry.input_tokens += record.usage.input_tokens;
            session_entry.output_tokens += record.usage.output_tokens;
            session_entry.total_tokens += total;
            session_entry.request_count += 1;
            session_entry.first_used =
                Some(session_entry.first_used.map_or(ts, |existing| existing.min(ts)));
            session_entry.last_used =
                Some(session_entry.last_used.map_or(ts, |existing| existing.max(ts)));

            let per_model = session_entry
                .by_model
                .entry(model.clone())
                .or_default();
            per_model.cost_usd += cost;
            per_model.input_tokens += record.usage.input_tokens;
            per_model.output_tokens += record.usage.output_tokens;
            per_model.total_tokens += total;
            per_model.request_count += 1;
            per_model.first_used = Some(
                per_model
                    .first_used
                    .map_or(ts, |existing| existing.min(ts)),
            );
            per_model.last_used = Some(
                per_model
                    .last_used
                    .map_or(ts, |existing| existing.max(ts)),
            );
        }
    }

    let mut model_out = serde_json::Map::with_capacity(by_model.len());
    for (model, agg) in by_model {
        model_out.insert(
            model.clone(),
            serde_json::json!({
                "model": model,
                "costUsd": agg.cost_usd,
                "inputTokens": agg.input_tokens,
                "outputTokens": agg.output_tokens,
                "totalTokens": agg.total_tokens,
                "requestCount": agg.request_count,
                "firstUsed": agg.first_used.map(|t| t.to_rfc3339()),
                "lastUsed": agg.last_used.map(|t| t.to_rfc3339()),
            }),
        );
    }

    let mut session_out = serde_json::Map::with_capacity(by_session.len());
    for (session_id, agg) in by_session {
        let mut per_model_map = serde_json::Map::with_capacity(agg.by_model.len());
        for (model, stats) in agg.by_model {
            per_model_map.insert(
                model.clone(),
                serde_json::json!({
                    "model": model,
                    "costUsd": stats.cost_usd,
                    "inputTokens": stats.input_tokens,
                    "outputTokens": stats.output_tokens,
                    "totalTokens": stats.total_tokens,
                    "requestCount": stats.request_count,
                    "firstUsed": stats.first_used.map(|t| t.to_rfc3339()),
                    "lastUsed": stats.last_used.map(|t| t.to_rfc3339()),
                }),
            );
        }
        session_out.insert(
            session_id.clone(),
            serde_json::json!({
                "sessionId": session_id,
                "costUsd": agg.cost_usd,
                "inputTokens": agg.input_tokens,
                "outputTokens": agg.output_tokens,
                "totalTokens": agg.total_tokens,
                "requestCount": agg.request_count,
                "firstUsed": agg.first_used.map(|t| t.to_rfc3339()),
                "lastUsed": agg.last_used.map(|t| t.to_rfc3339()),
                "byModel": serde_json::Value::Object(per_model_map),
            }),
        );
    }

    let mut provider_out = serde_json::Map::with_capacity(by_provider.len());
    for (provider, agg) in by_provider {
        provider_out.insert(
            provider.clone(),
            serde_json::json!({
                "provider": provider,
                "costUsd": agg.cost_usd,
                "inputTokens": agg.input_tokens,
                "outputTokens": agg.output_tokens,
                "totalTokens": agg.total_tokens,
                "requestCount": agg.request_count,
                "modelCount": agg.models.len() as u64,
                "models": agg.models.into_iter().collect::<Vec<_>>(),
                "firstUsed": agg.first_used.map(|t| t.to_rfc3339()),
                "lastUsed": agg.last_used.map(|t| t.to_rfc3339()),
            }),
        );
    }

    let mut coding_mode_out = serde_json::Map::with_capacity(by_coding_mode.len());
    for (mode, agg) in by_coding_mode {
        coding_mode_out.insert(
            mode.clone(),
            serde_json::json!({
                "mode": mode,
                "costUsd": agg.cost_usd,
                "inputTokens": agg.input_tokens,
                "outputTokens": agg.output_tokens,
                "totalTokens": agg.total_tokens,
                "requestCount": agg.request_count,
                "sessionCount": agg.sessions.len() as u64,
                "modelCount": agg.models.len() as u64,
                "firstUsed": agg.first_used.map(|t| t.to_rfc3339()),
                "lastUsed": agg.last_used.map(|t| t.to_rfc3339()),
            }),
        );
    }

    let token_rate_per_min = if last_minute_tokens > 0 {
        last_minute_tokens as f64
    } else {
        last_hour_tokens as f64 / 60.0
    };

    UsageAggregates {
        by_model: serde_json::Value::Object(model_out),
        by_session: serde_json::Value::Object(session_out),
        by_provider: serde_json::Value::Object(provider_out),
        by_workspace: serde_json::Value::Object(serde_json::Map::new()),
        by_coding_mode: serde_json::Value::Object(coding_mode_out),
        token_rate_per_min,
        last_24h_tokens,
        last_24h_cost_usd,
        last_24h_requests,
        last_7d_tokens,
        last_7d_cost_usd,
        last_7d_requests,
    }
}

fn build_agent_config_payload(config: &crate::config::Config) -> serde_json::Value {
    let snake = serde_json::to_value(&config.agent).unwrap_or_else(|_| serde_json::json!({}));
    to_camel_case_keys(snake)
}

pub async fn handle_agent_config_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    Json(build_agent_config_payload(&config)).into_response()
}

pub async fn handle_agent_config_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let snapshot = {
        let mut cfg = state.config.lock();
        let current = serde_json::to_value(&cfg.agent).unwrap_or_else(|_| serde_json::json!({}));
        let mut merged_camel = to_camel_case_keys(current);
        deep_merge(&mut merged_camel, body);
        let snake = to_snake_case_keys(merged_camel);
        match serde_json::from_value::<crate::config::AgentConfig>(snake) {
            Ok(parsed) => cfg.agent = parsed,
            Err(err) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": format!("invalid agent config payload: {err}")
                    })),
                )
                    .into_response();
            }
        }
        cfg.clone()
    };

    if let Err(e) = snapshot.save().await {
        tracing::error!("Failed to save config (agent): {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to save configuration"})),
        )
            .into_response();
    }
    *state.config.lock() = snapshot.clone();
    state.push_live_config(snapshot.clone());
    Json(build_agent_config_payload(&snapshot)).into_response()
}

fn build_agent_runtime_payload(config: &crate::config::Config) -> serde_json::Value {
    let snake =
        serde_json::to_value(&config.agent_runtime).unwrap_or_else(|_| serde_json::json!({}));
    to_camel_case_keys(snake)
}

pub async fn handle_agent_runtime_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    Json(build_agent_runtime_payload(&config)).into_response()
}

pub async fn handle_agent_runtime_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let snapshot = {
        let mut cfg = state.config.lock();
        let current =
            serde_json::to_value(&cfg.agent_runtime).unwrap_or_else(|_| serde_json::json!({}));
        let mut merged_camel = to_camel_case_keys(current);
        deep_merge(&mut merged_camel, body);
        let snake = to_snake_case_keys(merged_camel);
        match serde_json::from_value::<crate::config::domain::AgentRuntimeExtras>(snake) {
            Ok(parsed) => cfg.agent_runtime = parsed,
            Err(err) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": format!("invalid agent runtime payload: {err}")
                    })),
                )
                    .into_response();
            }
        }
        cfg.clone()
    };

    if let Err(e) = snapshot.save().await {
        tracing::error!("Failed to save config (agent_runtime): {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to save configuration"})),
        )
            .into_response();
    }
    *state.config.lock() = snapshot.clone();
    state.push_live_config(snapshot.clone());
    Json(build_agent_runtime_payload(&snapshot)).into_response()
}

fn build_web_search_payload(config: &crate::config::Config) -> serde_json::Value {
    let snake = serde_json::to_value(&config.web_search).unwrap_or_else(|_| serde_json::json!({}));
    to_camel_case_keys(snake)
}

pub async fn handle_web_search_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    Json(build_web_search_payload(&config)).into_response()
}

pub async fn handle_web_search_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let snapshot = {
        let mut cfg = state.config.lock();
        let current =
            serde_json::to_value(&cfg.web_search).unwrap_or_else(|_| serde_json::json!({}));
        let mut merged_camel = to_camel_case_keys(current);
        deep_merge(&mut merged_camel, body);
        let snake = to_snake_case_keys(merged_camel);
        match serde_json::from_value::<crate::config::WebSearchConfig>(snake) {
            Ok(parsed) => cfg.web_search = parsed,
            Err(err) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": format!("invalid web_search payload: {err}")
                    })),
                )
                    .into_response();
            }
        }
        cfg.clone()
    };

    if let Err(e) = snapshot.save().await {
        tracing::error!("Failed to save config (web_search): {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to save configuration"})),
        )
            .into_response();
    }
    *state.config.lock() = snapshot.clone();
    state.push_live_config(snapshot.clone());
    Json(build_web_search_payload(&snapshot)).into_response()
}

fn build_web_fetch_payload(config: &crate::config::Config) -> serde_json::Value {
    let snake = serde_json::to_value(&config.web_fetch).unwrap_or_else(|_| serde_json::json!({}));
    to_camel_case_keys(snake)
}

pub async fn handle_web_fetch_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.config.lock().clone();
    Json(build_web_fetch_payload(&config)).into_response()
}

pub async fn handle_web_fetch_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let snapshot = {
        let mut cfg = state.config.lock();
        let current =
            serde_json::to_value(&cfg.web_fetch).unwrap_or_else(|_| serde_json::json!({}));
        let mut merged_camel = to_camel_case_keys(current);
        deep_merge(&mut merged_camel, body);
        let snake = to_snake_case_keys(merged_camel);
        match serde_json::from_value::<crate::config::WebFetchConfig>(snake) {
            Ok(parsed) => cfg.web_fetch = parsed,
            Err(err) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": format!("invalid web_fetch payload: {err}")
                    })),
                )
                    .into_response();
            }
        }
        cfg.clone()
    };

    if let Err(e) = snapshot.save().await {
        tracing::error!("Failed to save config (web_fetch): {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to save configuration"})),
        )
            .into_response();
    }
    *state.config.lock() = snapshot.clone();
    state.push_live_config(snapshot.clone());
    Json(build_web_fetch_payload(&snapshot)).into_response()
}

use crate::config::schema::{LspInstallState, LspServerEntry};

#[derive(Debug, Deserialize)]
pub struct LspUpsertBody {
    pub id: String,
    #[serde(rename = "languageId", default)]
    pub language_id: String,
    #[serde(rename = "displayName", default)]
    pub display_name: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub managed: bool,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    #[serde(default, rename = "fileExtensions")]
    pub file_extensions: Vec<String>,
    #[serde(default, rename = "initializationOptions")]
    pub initialization_options: Option<serde_json::Value>,
}

fn lsp_entry_from_body(body: LspUpsertBody) -> LspServerEntry {
    LspServerEntry {
        id: body.id.trim().to_string(),
        language_id: body.language_id,
        display_name: body.display_name,
        enabled: body.enabled,
        managed: body.managed,
        command: body.command.filter(|s| !s.trim().is_empty()),
        args: body.args,
        env: body.env,
        file_extensions: body.file_extensions,
        initialization_options: body.initialization_options,
        install_state: LspInstallState::default(),
    }
}

fn lsp_entry_to_record(entry: &LspServerEntry) -> serde_json::Value {
    serde_json::json!({
        "id": entry.id,
        "languageId": entry.language_id,
        "displayName": entry.display_name,
        "enabled": entry.enabled,
        "managed": entry.managed,
        "command": entry.command,
        "args": entry.args,
        "env": entry.env,
        "fileExtensions": entry.file_extensions,
        "initializationOptions": entry.initialization_options,
        "installState": entry.install_state,
    })
}

pub async fn handle_lsp_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let cfg = state.config.lock().clone();
    let live_servers = state.lsp.service().list_servers().await;
    let live_languages: std::collections::HashSet<String> = live_servers
        .into_iter()
        .map(|info| info.language_id)
        .collect();
    let servers: Vec<serde_json::Value> = cfg
        .lsp
        .servers
        .iter()
        .map(|entry| {
            let mut record = lsp_entry_to_record(entry);
            let lifecycle = if live_languages.contains(&entry.language_id) {
                "ready"
            } else {
                "stopped"
            };
            if let Some(obj) = record.as_object_mut() {
                obj.insert(
                    "lifecycleStatus".to_string(),
                    serde_json::Value::String(lifecycle.into()),
                );
            }
            record
        })
        .collect();
    Json(serde_json::json!({
        "enabled": cfg.lsp.enabled,
        "servers": servers,
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
pub struct LspGlobalEnableBody {
    pub enabled: bool,
}

pub async fn handle_lsp_global_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LspGlobalEnableBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let snapshot = {
        let mut cfg = state.config.lock();
        cfg.lsp.enabled = body.enabled;
        cfg.clone()
    };
    if let Err(e) = snapshot.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("save config: {e}")})),
        )
            .into_response();
    }
    *state.config.lock() = snapshot.clone();
    state.push_live_config(snapshot.clone());
    state.lsp.reconcile(&snapshot).await;
    Json(serde_json::json!({"ok": true, "enabled": snapshot.lsp.enabled})).into_response()
}

pub async fn handle_lsp_preferences_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let cfg = state.config.lock().clone();
    Json(serde_json::json!({
        "inlayHintsEnabled": cfg.lsp.inlay_hints_enabled,
        "formatOnSave": cfg.lsp.format_on_save,
        "hoverDelayMs": cfg.lsp.hover_delay_ms,
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspPreferencesBody {
    #[serde(default)]
    pub inlay_hints_enabled: Option<bool>,
    #[serde(default)]
    pub format_on_save: Option<bool>,
    #[serde(default)]
    pub hover_delay_ms: Option<u32>,
}

pub async fn handle_lsp_preferences_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LspPreferencesBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let snapshot = {
        let mut cfg = state.config.lock();
        if let Some(v) = body.inlay_hints_enabled {
            cfg.lsp.inlay_hints_enabled = v;
        }
        if let Some(v) = body.format_on_save {
            cfg.lsp.format_on_save = v;
        }
        if let Some(v) = body.hover_delay_ms {
            cfg.lsp.hover_delay_ms = v.clamp(0, 5_000);
        }
        cfg.clone()
    };
    if let Err(e) = snapshot.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("save config: {e}")})),
        )
            .into_response();
    }
    *state.config.lock() = snapshot.clone();
    state.push_live_config(snapshot.clone());
    Json(serde_json::json!({
        "ok": true,
        "inlayHintsEnabled": snapshot.lsp.inlay_hints_enabled,
        "formatOnSave": snapshot.lsp.format_on_save,
        "hoverDelayMs": snapshot.lsp.hover_delay_ms,
    }))
    .into_response()
}

pub async fn handle_lsp_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LspUpsertBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let new_entry = lsp_entry_from_body(body);
    if new_entry.id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "id is required"})),
        )
            .into_response();
    }
    let snapshot = {
        let mut cfg = state.config.lock();
        if cfg.lsp.servers.iter().any(|s| s.id == new_entry.id) {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": format!("server already exists: {}", new_entry.id)
                })),
            )
                .into_response();
        }
        cfg.lsp.servers.push(new_entry.clone());
        cfg.clone()
    };
    if let Err(e) = snapshot.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("save config: {e}")})),
        )
            .into_response();
    }
    *state.config.lock() = snapshot.clone();
    state.push_live_config(snapshot.clone());
    state.lsp.reconcile(&snapshot).await;
    let record = snapshot
        .lsp
        .servers
        .iter()
        .find(|s| s.id == new_entry.id)
        .map(lsp_entry_to_record)
        .unwrap_or(serde_json::Value::Null);
    Json(serde_json::json!({ "server": record })).into_response()
}

pub async fn handle_lsp_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<LspUpsertBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let snapshot = {
        let mut cfg = state.config.lock();
        let Some(idx) = cfg.lsp.servers.iter().position(|s| s.id == id) else {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": format!("server not found: {id}")
                })),
            )
                .into_response();
        };

        let preserved_state = cfg.lsp.servers[idx].install_state.clone();
        let mut next = lsp_entry_from_body(body);
        next.id = id.clone();
        if next.managed {
            next.install_state = preserved_state;

            if next.command.is_none() {
                if let LspInstallState::Installed { ref path, .. } = next.install_state {
                    next.command = Some(path.clone());
                }
            }
        } else {
            next.install_state = LspInstallState::NotInstalled;
        }
        cfg.lsp.servers[idx] = next;
        cfg.clone()
    };
    if let Err(e) = snapshot.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("save config: {e}")})),
        )
            .into_response();
    }
    *state.config.lock() = snapshot.clone();
    state.push_live_config(snapshot.clone());
    state.lsp.reconcile(&snapshot).await;
    let record = snapshot
        .lsp
        .servers
        .iter()
        .find(|s| s.id == id)
        .map(lsp_entry_to_record)
        .unwrap_or(serde_json::Value::Null);
    Json(serde_json::json!({ "server": record })).into_response()
}

pub async fn handle_lsp_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let snapshot = {
        let mut cfg = state.config.lock();
        let Some(idx) = cfg.lsp.servers.iter().position(|s| s.id == id) else {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": format!("server not found: {id}")
                })),
            )
                .into_response();
        };
        cfg.lsp.servers.remove(idx);
        cfg.clone()
    };
    if let Err(e) = snapshot.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("save config: {e}")})),
        )
            .into_response();
    }
    *state.config.lock() = snapshot.clone();
    state.push_live_config(snapshot.clone());
    state.lsp.reconcile(&snapshot).await;
    Json(serde_json::json!({"ok": true})).into_response()
}

pub async fn handle_lsp_toggle(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let snapshot = {
        let mut cfg = state.config.lock();
        let Some(idx) = cfg.lsp.servers.iter().position(|s| s.id == id) else {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": format!("server not found: {id}")
                })),
            )
                .into_response();
        };
        cfg.lsp.servers[idx].enabled = !cfg.lsp.servers[idx].enabled;
        cfg.clone()
    };
    if let Err(e) = snapshot.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("save config: {e}")})),
        )
            .into_response();
    }
    *state.config.lock() = snapshot.clone();
    state.push_live_config(snapshot.clone());
    state.lsp.reconcile(&snapshot).await;
    let record = snapshot
        .lsp
        .servers
        .iter()
        .find(|s| s.id == id)
        .map(lsp_entry_to_record)
        .unwrap_or(serde_json::Value::Null);
    Json(serde_json::json!({ "server": record })).into_response()
}

pub async fn handle_lsp_install(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    {
        let cfg = state.config.lock();
        if !cfg.lsp.servers.iter().any(|s| s.id == id) {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": format!("server not found: {id}")
                })),
            )
                .into_response();
        }
    }
    let result = state
        .lsp
        .clone()
        .install(state.config.clone(), state.live_config.clone(), id.clone())
        .await;
    match result {
        Ok(report) => Json(serde_json::json!({
            "ok": true,
            "version": report.version,
            "path": report.binary_path.to_string_lossy(),
        }))
        .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{err:#}")})),
        )
            .into_response(),
    }
}

pub async fn handle_lsp_restart(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let snapshot = state.config.lock().clone();
    let Some(entry) = snapshot.lsp.servers.iter().find(|s| s.id == id).cloned() else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": format!("server not found: {id}")
            })),
        )
            .into_response();
    };
    state
        .lsp
        .service()
        .shutdown_server(&entry.language_id, &snapshot.workspace_dir)
        .await;
    state.lsp.reconcile(&snapshot).await;
    Json(serde_json::json!({"ok": true})).into_response()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspNotifyBody {

    pub method: String,

    pub uri: String,
    #[serde(default)]
    pub language_id: Option<String>,

    #[serde(default)]
    pub text: Option<String>,

    #[serde(default)]
    pub version: Option<i64>,
}

fn uri_to_path(uri: &str) -> Option<PathBuf> {
    if let Some(rest) = uri.strip_prefix("file://") {

        #[cfg(windows)]
        {
            let trimmed = rest.trim_start_matches('/');
            return Some(PathBuf::from(trimmed.replace('/', "\\")));
        }
        #[cfg(not(windows))]
        {
            return Some(PathBuf::from(rest));
        }
    }
    Some(PathBuf::from(uri))
}

fn resolve_lsp_server_language(
    snapshot: &crate::config::schema::Config,
    language: &str,
    path: Option<&std::path::Path>,
) -> Option<String> {
    if !snapshot.lsp.enabled {
        return None;
    }
    let ext = path
        .and_then(|p| p.extension())
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase());

    for s in &snapshot.lsp.servers {
        if !s.enabled || s.resolved_command().is_none() {
            continue;
        }
        if s.language_id == language {
            return Some(s.language_id.clone());
        }
    }
    if let Some(ref ext_lower) = ext {
        for s in &snapshot.lsp.servers {
            if !s.enabled || s.resolved_command().is_none() {
                continue;
            }
            if s
                .file_extensions
                .iter()
                .any(|x| x.eq_ignore_ascii_case(ext_lower))
            {
                return Some(s.language_id.clone());
            }
        }
    }
    None
}

pub async fn handle_lsp_notify(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LspNotifyBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let path = match uri_to_path(&body.uri) {
        Some(p) => p,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid uri"})),
            )
                .into_response();
        }
    };
    let hint_language = body
        .language_id
        .clone()
        .or_else(|| crate::services::lsp::detect_language(&path).map(str::to_string));
    let Some(hint) = hint_language else {
        return Json(serde_json::json!({"ok": false, "skipped": "unknown language"})).into_response();
    };

    let svc = state.lsp.service();

    let snapshot = state.config.lock().clone();
    let routed = resolve_lsp_server_language(&snapshot, &hint, Some(&path));
    let Some(language) = routed else {
        if body.method == "didClose" {
            return Json(serde_json::json!({"ok": true, "skipped": "no server"}))
                .into_response();
        }
        return Json(serde_json::json!({
            "ok": false,
            "skipped": "no enabled server for this language"
        }))
        .into_response();
    };

    let version = body.version.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(1)
    });

    let result: anyhow::Result<()> = match body.method.as_str() {
        "didOpen" => {
            let text = body.text.clone().unwrap_or_default();
            svc.open_text_document(&path, &language, &text, version)
                .await
        }
        "didChange" => {
            let text = body.text.clone().unwrap_or_default();
            svc.change_text_document(&path, &language, &text, version)
                .await
        }
        "didSave" => svc
            .save_text_document(&path, &language, body.text.as_deref())
            .await,
        "didClose" => svc.close_text_document(&path, &language).await,
        other => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("unsupported method: {other}")})),
            )
                .into_response();
        }
    };

    match result {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "ok": false,
                "error": format!("{err:#}")
            })),
        )
            .into_response(),
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
pub struct LspPositionBody {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
pub struct LspRangeBody {
    pub start: LspPositionBody,
    pub end: LspPositionBody,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LspFormattingOptionsBody {
    #[serde(default = "default_tab_size")]
    pub tab_size: u32,
    #[serde(default = "default_insert_spaces")]
    pub insert_spaces: bool,
}

fn default_tab_size() -> u32 {
    4
}

fn default_insert_spaces() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspRequestBody {

    pub method: String,
    #[serde(default)]
    pub uri: String,
    #[serde(default)]
    pub language_id: Option<String>,
    #[serde(default)]
    pub line: u32,
    #[serde(default)]
    pub character: u32,

    #[serde(default)]
    pub text: Option<String>,

    #[serde(default)]
    pub range: Option<LspRangeBody>,

    #[serde(default)]
    pub options: Option<LspFormattingOptionsBody>,

    #[serde(default)]
    pub trigger_character: Option<String>,

    #[serde(default)]
    pub trigger_kind: Option<i32>,

    #[serde(default)]
    pub diagnostics: Option<Vec<serde_json::Value>>,

    #[serde(default)]
    pub only: Option<Vec<String>>,

    #[serde(default)]
    pub command: Option<String>,

    #[serde(default)]
    pub arguments: Option<Vec<serde_json::Value>>,

    #[serde(default)]
    pub new_name: Option<String>,

    #[serde(default)]
    pub item: Option<serde_json::Value>,

    #[serde(default)]
    pub positions: Option<Vec<LspPositionBody>>,

    #[serde(default)]
    pub previous_result_id: Option<String>,

    #[serde(default)]
    pub character_typed: Option<String>,

    #[serde(default)]
    pub query: Option<String>,
}

pub async fn handle_lsp_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LspRequestBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    if body.method == "executeCommand" {
        return handle_lsp_execute_command(&state, &body).await;
    }

    if matches!(
        body.method.as_str(),
        "completionItem/resolve" | "completionResolve" | "workspaceSymbol" | "workspace/symbol"
    ) {
        return handle_lsp_pathless_request(&state, &body).await;
    }

    let path = match uri_to_path(&body.uri) {
        Some(p) => p,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid uri"})),
            )
                .into_response();
        }
    };
    let hint_language = body
        .language_id
        .clone()
        .or_else(|| crate::services::lsp::detect_language(&path).map(str::to_string));
    let Some(hint) = hint_language else {
        return Json(serde_json::json!({"result": null})).into_response();
    };

    let svc = state.lsp.service();
    let snapshot = state.config.lock().clone();
    let workspace = snapshot.workspace_dir.clone();
    let Some(language) = resolve_lsp_server_language(&snapshot, &hint, Some(&path)) else {
        return Json(serde_json::json!({"result": null})).into_response();
    };
    drop(snapshot);
    if let Some(text) = body.text.as_deref() {

        let version = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(1);
        let _ = svc
            .change_text_document(&path, &language, text, version)
            .await;
    }

    let lsp_method = match body.method.as_str() {
        "hover" => "textDocument/hover",
        "completion" => "textDocument/completion",
        "definition" => "textDocument/definition",
        "typeDefinition" => "textDocument/typeDefinition",
        "implementation" => "textDocument/implementation",
        "declaration" => "textDocument/declaration",
        "references" => "textDocument/references",
        "documentHighlight" => "textDocument/documentHighlight",
        "inlayHint" => "textDocument/inlayHint",
        "signatureHelp" => "textDocument/signatureHelp",
        "documentSymbol" => "textDocument/documentSymbol",
        "formatting" => "textDocument/formatting",
        "rangeFormatting" => "textDocument/rangeFormatting",
        "onTypeFormatting" => "textDocument/onTypeFormatting",
        "codeAction" => "textDocument/codeAction",
        "prepareRename" => "textDocument/prepareRename",
        "rename" => "textDocument/rename",
        "foldingRange" => "textDocument/foldingRange",
        "selectionRange" => "textDocument/selectionRange",
        "documentLink" => "textDocument/documentLink",
        "semanticTokens/full" | "semanticTokensFull" => "textDocument/semanticTokens/full",
        "semanticTokens/full/delta" | "semanticTokensFullDelta" => {
            "textDocument/semanticTokens/full/delta"
        }
        "semanticTokens/range" | "semanticTokensRange" => "textDocument/semanticTokens/range",
        other => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("unsupported method: {other}")})),
            )
                .into_response();
        }
    };
    let uri = crate::services::lsp::path_to_uri(&path);
    let params = match body.method.as_str() {
        "hover"
        | "definition"
        | "typeDefinition"
        | "implementation"
        | "declaration"
        | "references"
        | "documentHighlight"
        | "signatureHelp"
        | "prepareRename" => {
            let mut p = serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": body.line, "character": body.character },
            });
            if body.method == "references" {
                p["context"] = serde_json::json!({ "includeDeclaration": true });
            }
            if body.method == "signatureHelp" {
                if let Some(tc) = body.trigger_character.as_deref() {
                    p["context"] = serde_json::json!({
                        "triggerKind": body.trigger_kind.unwrap_or(2),
                        "triggerCharacter": tc,
                        "isRetrigger": false,
                    });
                }
            }
            p
        }
        "completion" => {
            let mut p = serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": body.line, "character": body.character },
            });
            let trigger_kind = body.trigger_kind.unwrap_or(1);
            let mut ctx = serde_json::json!({ "triggerKind": trigger_kind });
            if let Some(tc) = body.trigger_character.as_deref() {
                ctx["triggerCharacter"] = serde_json::json!(tc);
            }
            p["context"] = ctx;
            p
        }
        "rename" => {
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": body.line, "character": body.character },
                "newName": body.new_name.clone().unwrap_or_default(),
            })
        }
        "documentSymbol" | "foldingRange" | "documentLink" | "semanticTokens/full"
        | "semanticTokensFull" => serde_json::json!({
            "textDocument": { "uri": uri },
        }),
        "semanticTokens/full/delta" | "semanticTokensFullDelta" => serde_json::json!({
            "textDocument": { "uri": uri },
            "previousResultId": body.previous_result_id.clone().unwrap_or_default(),
        }),
        "semanticTokens/range" | "semanticTokensRange" => {
            let range = body.range.as_ref();
            let start_line = range.map(|r| r.start.line).unwrap_or(0);
            let start_char = range.map(|r| r.start.character).unwrap_or(0);
            let end_line = range.map(|r| r.end.line).unwrap_or(u32::MAX / 2);
            let end_char = range.map(|r| r.end.character).unwrap_or(0);
            serde_json::json!({
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": start_line, "character": start_char },
                    "end": { "line": end_line, "character": end_char },
                },
            })
        }
        "selectionRange" => {
            let positions: Vec<serde_json::Value> = body
                .positions
                .clone()
                .unwrap_or_else(|| {
                    vec![LspPositionBody {
                        line: body.line,
                        character: body.character,
                    }]
                })
                .into_iter()
                .map(|p| serde_json::json!({ "line": p.line, "character": p.character }))
                .collect();
            serde_json::json!({
                "textDocument": { "uri": uri },
                "positions": positions,
            })
        }
        "inlayHint" => {
            let range = body.range.as_ref();
            let start_line = range.map(|r| r.start.line).unwrap_or(0);
            let start_char = range.map(|r| r.start.character).unwrap_or(0);
            let end_line = range.map(|r| r.end.line).unwrap_or(u32::MAX / 2);
            let end_char = range.map(|r| r.end.character).unwrap_or(0);
            serde_json::json!({
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": start_line, "character": start_char },
                    "end": { "line": end_line, "character": end_char },
                },
            })
        }
        "formatting" => {
            let opts = body.options.as_ref();
            let tab_size = opts.map(|o| o.tab_size).unwrap_or(4);
            let insert_spaces = opts.map(|o| o.insert_spaces).unwrap_or(true);
            serde_json::json!({
                "textDocument": { "uri": uri },
                "options": {
                    "tabSize": tab_size,
                    "insertSpaces": insert_spaces,
                },
            })
        }
        "rangeFormatting" => {
            let opts = body.options.as_ref();
            let tab_size = opts.map(|o| o.tab_size).unwrap_or(4);
            let insert_spaces = opts.map(|o| o.insert_spaces).unwrap_or(true);
            let range = body.range.as_ref();
            let start_line = range.map(|r| r.start.line).unwrap_or(0);
            let start_char = range.map(|r| r.start.character).unwrap_or(0);
            let end_line = range.map(|r| r.end.line).unwrap_or(0);
            let end_char = range.map(|r| r.end.character).unwrap_or(0);
            serde_json::json!({
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": start_line, "character": start_char },
                    "end": { "line": end_line, "character": end_char },
                },
                "options": {
                    "tabSize": tab_size,
                    "insertSpaces": insert_spaces,
                },
            })
        }
        "onTypeFormatting" => {
            let opts = body.options.as_ref();
            let tab_size = opts.map(|o| o.tab_size).unwrap_or(4);
            let insert_spaces = opts.map(|o| o.insert_spaces).unwrap_or(true);
            let ch = body
                .character_typed
                .clone()
                .or_else(|| body.trigger_character.clone())
                .unwrap_or_default();
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": body.line, "character": body.character },
                "ch": ch,
                "options": {
                    "tabSize": tab_size,
                    "insertSpaces": insert_spaces,
                },
            })
        }
        "codeAction" => {
            let range = body.range.as_ref();
            let start_line = range.map(|r| r.start.line).unwrap_or(body.line);
            let start_char = range.map(|r| r.start.character).unwrap_or(body.character);
            let end_line = range.map(|r| r.end.line).unwrap_or(body.line);
            let end_char = range.map(|r| r.end.character).unwrap_or(body.character);
            let mut context = serde_json::json!({
                "diagnostics": body.diagnostics.clone().unwrap_or_default(),
            });
            if let Some(only) = body.only.as_ref() {
                if !only.is_empty() {
                    context["only"] = serde_json::json!(only);
                }
            }
            serde_json::json!({
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": start_line, "character": start_char },
                    "end": { "line": end_line, "character": end_char },
                },
                "context": context,
            })
        }
        _ => serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": body.line, "character": body.character },
        }),
    };

    match svc
        .request(&language, &workspace, Some(&path), lsp_method, params)
        .await
    {
        Ok(result) => Json(serde_json::json!({ "result": result })).into_response(),
        Err(err) => Json(serde_json::json!({
            "result": null,
            "error": format!("{err:#}"),
        }))
        .into_response(),
    }
}

async fn handle_lsp_pathless_request(
    state: &AppState,
    body: &LspRequestBody,
) -> axum::response::Response {
    let lsp_method = match body.method.as_str() {
        "completionItem/resolve" | "completionResolve" => "completionItem/resolve",
        "workspaceSymbol" | "workspace/symbol" => "workspace/symbol",
        other => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("unsupported method: {other}")})),
            )
                .into_response();
        }
    };

    let (file_path, hint_language): (Option<std::path::PathBuf>, Option<String>) =
        if body.uri.is_empty() {
            (None, body.language_id.clone())
        } else {
            match uri_to_path(&body.uri) {
                Some(p) => {
                    let lang = body
                        .language_id
                        .clone()
                        .or_else(|| crate::services::lsp::detect_language(&p).map(str::to_string));
                    (Some(p), lang)
                }
                None => (None, body.language_id.clone()),
            }
        };

    let snapshot = state.config.lock().clone();
    let workspace = snapshot.workspace_dir.clone();
    let lsp_enabled = snapshot.lsp.enabled;
    let language = match hint_language {
        Some(l) => {
            match resolve_lsp_server_language(&snapshot, &l, file_path.as_deref()) {
                Some(routed) => routed,
                None => return Json(serde_json::json!({"result": null})).into_response(),
            }
        }
        None => {
            let candidates: Vec<String> = snapshot
                .lsp
                .servers
                .iter()
                .filter(|s| s.enabled && s.resolved_command().is_some())
                .map(|s| s.language_id.clone())
                .collect();
            drop(snapshot);
            if !lsp_enabled || candidates.is_empty() {
                return Json(serde_json::json!({"result": null})).into_response();
            }
            if candidates.len() == 1 {
                match candidates.into_iter().next() {
                    Some(lang) => lang,
                    None => return Json(serde_json::json!({"result": null})).into_response(),
                }
            } else {
                return Json(serde_json::json!({"result": null})).into_response();
            }
        }
    };

    let svc = state.lsp.service();
    let params = match lsp_method {
        "completionItem/resolve" => body
            .item
            .clone()
            .unwrap_or_else(|| serde_json::json!({})),
        "workspace/symbol" => serde_json::json!({
            "query": body.query.clone().unwrap_or_default(),
        }),
        _ => serde_json::json!({}),
    };

    match svc
        .request(&language, &workspace, file_path.as_deref(), lsp_method, params)
        .await
    {
        Ok(result) => Json(serde_json::json!({ "result": result })).into_response(),
        Err(err) => Json(serde_json::json!({
            "result": null,
            "error": format!("{err:#}"),
        }))
        .into_response(),
    }
}

async fn handle_lsp_execute_command(
    state: &AppState,
    body: &LspRequestBody,
) -> axum::response::Response {
    let Some(command) = body.command.as_deref() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "missing command"})),
        )
            .into_response();
    };
    if command.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "empty command"})),
        )
            .into_response();
    }

    let (file_path, hint_language): (Option<std::path::PathBuf>, Option<String>) =
        if body.uri.is_empty() {
            (None, body.language_id.clone())
        } else {
            match uri_to_path(&body.uri) {
                Some(p) => {
                    let lang = body
                        .language_id
                        .clone()
                        .or_else(|| crate::services::lsp::detect_language(&p).map(str::to_string));
                    (Some(p), lang)
                }
                None => (None, body.language_id.clone()),
            }
        };

    let snapshot = state.config.lock().clone();
    let workspace = snapshot.workspace_dir.clone();
    let lsp_enabled = snapshot.lsp.enabled;
    let language = match hint_language {
        Some(l) => {
            match resolve_lsp_server_language(&snapshot, &l, file_path.as_deref()) {
                Some(routed) => routed,
                None => return Json(serde_json::json!({"result": null})).into_response(),
            }
        }
        None => {
            let candidates: Vec<String> = snapshot
                .lsp
                .servers
                .iter()
                .filter(|s| s.enabled && s.resolved_command().is_some())
                .map(|s| s.language_id.clone())
                .collect();
            drop(snapshot);
            if !lsp_enabled || candidates.is_empty() {
                return Json(serde_json::json!({"result": null})).into_response();
            }
            if candidates.len() == 1 {
                match candidates.into_iter().next() {
                    Some(lang) => lang,
                    None => return Json(serde_json::json!({"result": null})).into_response(),
                }
            } else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": "executeCommand requires uri or languageId hint when multiple servers are enabled",
                    })),
                )
                    .into_response();
            }
        }
    };

    let svc = state.lsp.service();
    let params = serde_json::json!({
        "command": command,
        "arguments": body.arguments.clone().unwrap_or_default(),
    });

    match svc
        .request(
            &language,
            &workspace,
            file_path.as_deref(),
            "workspace/executeCommand",
            params,
        )
        .await
    {
        Ok(result) => Json(serde_json::json!({ "result": result })).into_response(),
        Err(err) => Json(serde_json::json!({
            "result": null,
            "error": format!("{err:#}"),
        }))
        .into_response(),
    }
}

fn background_signal_to_json(
    sig: &crate::tools::background_registry::BackgroundShellSignal,
) -> serde_json::Value {
    use crate::tools::background_registry::{BackgroundShellSignal, BgStream};
    match sig {
        BackgroundShellSignal::Spawned {
            id,
            command,
            session_id,
        } => serde_json::json!({
            "type": "spawned",
            "id": id,
            "command": command,
            "sessionId": session_id,
        }),
        BackgroundShellSignal::Chunk {
            id,
            stream,
            line,
            session_id,
        } => serde_json::json!({
            "type": "chunk",
            "id": id,
            "stream": match stream {
                BgStream::Stdout => "stdout",
                BgStream::Stderr => "stderr",
            },
            "line": line,
            "sessionId": session_id,
        }),
        BackgroundShellSignal::Heartbeat {
            id,
            elapsed_secs,
            session_id,
        } => serde_json::json!({
            "type": "heartbeat",
            "id": id,
            "elapsedSecs": elapsed_secs,
            "sessionId": session_id,
        }),
        BackgroundShellSignal::Exited {
            id,
            elapsed_secs,
            exit_code,
            session_id,
        } => serde_json::json!({
            "type": "exited",
            "id": id,
            "elapsedSecs": elapsed_secs,
            "exitCode": exit_code,
            "sessionId": session_id,
        }),
    }
}

pub async fn handle_background_shell_stream(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<SseEvent, Infallible>>>, (StatusCode, Json<serde_json::Value>)>
{
    require_auth(&state, &headers)?;
    let rx = crate::tools::background_registry::subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|item| async move {
        match item {
            Ok(sig) => {
                let payload = background_signal_to_json(&sig);
                match serde_json::to_string(&payload) {
                    Ok(json) => Some(Ok(SseEvent::default().data(json))),
                    Err(_) => None,
                }
            }
            Err(_) => None,
        }
    });
    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
}
