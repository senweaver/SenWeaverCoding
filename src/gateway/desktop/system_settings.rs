// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::AppState;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
};
use crate::config::{ProxyConfig, ProxyScope, SandboxBackend};
use super::super::api::require_auth;

fn sandbox_backend_label(backend: &SandboxBackend) -> &'static str {
    match backend {
        SandboxBackend::Auto => "auto",
        SandboxBackend::Landlock => "landlock",
        SandboxBackend::Firejail => "firejail",
        SandboxBackend::Bubblewrap => "bubblewrap",
        SandboxBackend::Docker => "docker",
        SandboxBackend::SandboxExec => "sandbox-exec",
        SandboxBackend::Wasm => "wasm",
        SandboxBackend::None => "none",
    }
}

fn parse_sandbox_backend(raw: &str) -> Result<SandboxBackend, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "auto" => Ok(SandboxBackend::Auto),
        "landlock" => Ok(SandboxBackend::Landlock),
        "firejail" => Ok(SandboxBackend::Firejail),
        "bubblewrap" => Ok(SandboxBackend::Bubblewrap),
        "docker" => Ok(SandboxBackend::Docker),
        "sandbox-exec" | "sandboxexec" => Ok(SandboxBackend::SandboxExec),
        "wasm" => Ok(SandboxBackend::Wasm),
        "none" => Ok(SandboxBackend::None),
        other => Err(format!("unsupported sandbox backend: {other}")),
    }
}

fn available_sandbox_backends() -> Vec<&'static str> {
    let mut backends = vec!["auto", "none"];
    #[cfg(target_os = "linux")]
    {
        backends.extend(["landlock", "firejail", "bubblewrap", "docker"]);
    }
    #[cfg(target_os = "macos")]
    {
        backends.extend(["sandbox-exec", "docker"]);
    }
    #[cfg(target_os = "windows")]
    {
        backends.push("docker");
    }
    backends
}

fn build_network_settings_payload(proxy: &ProxyConfig) -> serde_json::Value {
    let scope = match proxy.scope {
        ProxyScope::Environment => "environment",
        ProxyScope::Internal => "internal",
        ProxyScope::Services => "services",
    };
    serde_json::json!({
        "proxy": {
            "enabled": proxy.enabled,
            "httpProxy": proxy.http_proxy,
            "httpsProxy": proxy.https_proxy,
            "allProxy": proxy.all_proxy,
            "noProxy": proxy.no_proxy,
            "scope": scope,
            "systemDetect": proxy.system_detect,
        }
    })
}

fn build_security_settings_payload(config: &crate::config::Config) -> serde_json::Value {
    let sandbox = &config.security.sandbox;
    let resources = &config.security.resources;
    serde_json::json!({
        "sandbox": {
            "enabled": sandbox.enabled != Some(false),
            "backend": sandbox_backend_label(&sandbox.backend),
            "confineFilesystem": sandbox.confine_filesystem,
            "availableBackends": available_sandbox_backends(),
        },
        "resources": {
            "maxMemoryMb": resources.max_memory_mb,
            "maxCpuTimeSeconds": resources.max_cpu_time_seconds,
            "maxSubprocesses": resources.max_subprocesses,
        }
    })
}

fn build_service_tokens_payload(config: &crate::config::Config) -> serde_json::Value {
    let rpc_set = crate::rpc::server::resolve_rpc_auth_token(&config.rpc).is_some();
    let mcp_set = crate::services::mcp_server::sse::resolve_sse_auth_token(
        config.mcp_server.sse_token.as_deref(),
    )
    .is_some();
    serde_json::json!({
        "rpcTokenSet": rpc_set,
        "mcpSseTokenSet": mcp_set,
    })
}

fn parse_proxy_scope(raw: &str) -> Result<ProxyScope, String> {
    crate::config::schema::parse_proxy_scope(raw)
        .ok_or_else(|| format!("unsupported proxy scope: {raw}"))
}

fn optional_trimmed_string(value: Option<&serde_json::Value>) -> Option<String> {
    value
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn apply_optional_token_field(
    body: &serde_json::Value,
    key: &str,
) -> Result<Option<Option<String>>, String> {
    let Some(obj) = body.as_object() else {
        return Ok(None);
    };
    if !obj.contains_key(key) {
        return Ok(None);
    }
    match obj.get(key) {
        Some(serde_json::Value::Null) => Ok(Some(None)),
        Some(serde_json::Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Ok(Some(None))
            } else {
                Ok(Some(Some(trimmed.to_string())))
            }
        }
        _ => Err(format!("{key} must be string or null")),
    }
}

pub async fn handle_network_settings_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.live_config.load_ref();
    Json(build_network_settings_payload(&config.proxy)).into_response()
}

pub async fn handle_network_settings_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let Some(proxy_body) = body.get("proxy") else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "missing proxy object"})),
        )
            .into_response();
    };

    let parsed_scope = match proxy_body.get("scope").and_then(|v| v.as_str()) {
        Some(raw) => match parse_proxy_scope(raw) {
            Ok(scope) => Some(scope),
            Err(err) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": err})),
                )
                    .into_response();
            }
        },
        None => None,
    };

    let snapshot = {
        let mut cfg = state.config.lock();
        let services = cfg.proxy.services.clone();
        if let Some(v) = proxy_body.get("enabled").and_then(|v| v.as_bool()) {
            cfg.proxy.enabled = v;
        }
        if proxy_body.get("httpProxy").is_some() {
            cfg.proxy.http_proxy = optional_trimmed_string(proxy_body.get("httpProxy"));
        }
        if proxy_body.get("httpsProxy").is_some() {
            cfg.proxy.https_proxy = optional_trimmed_string(proxy_body.get("httpsProxy"));
        }
        if proxy_body.get("allProxy").is_some() {
            cfg.proxy.all_proxy = optional_trimmed_string(proxy_body.get("allProxy"));
        }
        if let Some(arr) = proxy_body.get("noProxy").and_then(|v| v.as_array()) {
            cfg.proxy.no_proxy = arr
                .iter()
                .filter_map(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
        }
        if let Some(scope) = parsed_scope {
            cfg.proxy.scope = scope;
        }
        if let Some(v) = proxy_body.get("systemDetect").and_then(|v| v.as_bool()) {
            cfg.proxy.system_detect = v;
        }
        cfg.proxy.services = services;
        cfg.clone()
    };

    if let Err(e) = crate::gateway::persist_config(&snapshot).await {
        tracing::error!("Failed to save config (network-settings): {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to save configuration"})),
        )
            .into_response();
    }
    *state.config.lock() = snapshot.clone();
    state.push_live_config(snapshot.clone());
    crate::services::proxy::runtime::ProxyRuntime::global().replace(snapshot.proxy.clone());
    Json(build_network_settings_payload(&snapshot.proxy)).into_response()
}

pub async fn handle_security_settings_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.live_config.load_ref();
    Json(build_security_settings_payload(&config)).into_response()
}

pub async fn handle_security_settings_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let parsed_backend = match body
        .get("sandbox")
        .and_then(|s| s.get("backend"))
        .and_then(|v| v.as_str())
    {
        Some(raw) => match parse_sandbox_backend(raw) {
            Ok(backend) => Some(backend),
            Err(err) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": err})),
                )
                    .into_response();
            }
        },
        None => None,
    };

    let snapshot = {
        let mut cfg = state.config.lock();
        if let Some(sandbox_body) = body.get("sandbox") {
            if let Some(v) = sandbox_body.get("enabled").and_then(|v| v.as_bool()) {
                cfg.security.sandbox.enabled = Some(v);
            }
            if let Some(backend) = parsed_backend {
                cfg.security.sandbox.backend = backend;
            }
            if let Some(v) = sandbox_body
                .get("confineFilesystem")
                .and_then(|v| v.as_bool())
            {
                cfg.security.sandbox.confine_filesystem = v;
            }
        }
        if let Some(resources_body) = body.get("resources") {
            if resources_body.get("maxMemoryMb").is_some() {
                cfg.security.resources.max_memory_mb = resources_body
                    .get("maxMemoryMb")
                    .and_then(|v| {
                        if v.is_null() {
                            None
                        } else {
                            v.as_u64()
                        }
                    })
                    .map(|v| v as u32)
                    .filter(|v| *v > 0);
            }
            if resources_body.get("maxCpuTimeSeconds").is_some() {
                cfg.security.resources.max_cpu_time_seconds = resources_body
                    .get("maxCpuTimeSeconds")
                    .and_then(|v| {
                        if v.is_null() {
                            None
                        } else {
                            v.as_u64()
                        }
                    })
                    .filter(|v| *v > 0);
            }
            if resources_body.get("maxSubprocesses").is_some() {
                cfg.security.resources.max_subprocesses = resources_body
                    .get("maxSubprocesses")
                    .and_then(|v| {
                        if v.is_null() {
                            None
                        } else {
                            v.as_u64()
                        }
                    })
                    .map(|v| v as u32)
                    .filter(|v| *v > 0);
            }
        }
        cfg.clone()
    };

    if let Err(e) = crate::gateway::persist_config(&snapshot).await {
        tracing::error!("Failed to save config (security-settings): {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to save configuration"})),
        )
            .into_response();
    }
    *state.config.lock() = snapshot.clone();
    state.push_live_config(snapshot.clone());
    Json(build_security_settings_payload(&snapshot)).into_response()
}

pub async fn handle_service_tokens_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.live_config.load_ref();
    Json(build_service_tokens_payload(&config)).into_response()
}

pub async fn handle_service_tokens_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let rpc_update = match apply_optional_token_field(&body, "rpcToken") {
        Ok(v) => v,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": err})),
            )
                .into_response();
        }
    };
    let mcp_update = match apply_optional_token_field(&body, "mcpSseToken") {
        Ok(v) => v,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": err})),
            )
                .into_response();
        }
    };

    let snapshot = {
        let mut cfg = state.config.lock();
        if let Some(token) = rpc_update {
            cfg.rpc.auth_token = token;
        }
        if let Some(token) = mcp_update {
            cfg.mcp_server.sse_token = token;
        }
        cfg.clone()
    };

    if let Err(e) = crate::gateway::persist_config(&snapshot).await {
        tracing::error!("Failed to save config (service-tokens): {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to save configuration"})),
        )
            .into_response();
    }
    *state.config.lock() = snapshot.clone();
    state.push_live_config(snapshot.clone());
    Json(build_service_tokens_payload(&snapshot)).into_response()
}
