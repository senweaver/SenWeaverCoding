// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::AppState;
use super::api::require_auth;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
};
use serde::Deserialize;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use crate::inline_completion::registry::RegistryHandle;

const COMPLETION_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_COMPLETION_WINDOW_BYTES: usize = 64 * 1024;
const MAX_INLINE_EDIT_SELECTION_BYTES: usize = 512 * 1024;

fn completion_registry_cache() -> &'static parking_lot::RwLock<Option<(u64, RegistryHandle)>> {
    static CACHE: OnceLock<parking_lot::RwLock<Option<(u64, RegistryHandle)>>> = OnceLock::new();
    CACHE.get_or_init(|| parking_lot::RwLock::new(None))
}

fn completion_config_fingerprint(config: &crate::config::Config) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    config.default_provider.hash(&mut hasher);
    config.default_model.hash(&mut hasher);
    config.api_url.hash(&mut hasher);
    config.api_path.hash(&mut hasher);
    config.api_key.hash(&mut hasher);
    config.provider_timeout_secs.hash(&mut hasher);
    hasher.finish()
}

async fn resolve_completion_registry(state: &AppState) -> Option<RegistryHandle> {
    let config = state.live_config.load_ref();
    let fingerprint = completion_config_fingerprint(&config);
    if let Some((cached_fp, handle)) = completion_registry_cache().read().as_ref()
        && *cached_fp == fingerprint
    {
        return Some(handle.clone());
    }
    let cfg = (*config).clone();
    let built = tokio::task::spawn_blocking(move || {
        crate::inline_completion::registry::default_provider(&cfg)
    })
    .await
    .ok()
    .flatten()?;
    *completion_registry_cache().write() = Some((fingerprint, built.clone()));
    Some(built)
}

fn clip_tail(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut idx = s.len() - max_bytes;
    while idx < s.len() && !s.is_char_boundary(idx) {
        idx += 1;
    }
    &s[idx..]
}

fn clip_head(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut idx = max_bytes;
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    &s[..idx]
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineCompletionBody {
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub root: Option<String>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

pub async fn handle_editor_inline_completion(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<InlineCompletionBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    if body.prefix.trim().is_empty() {
        return Json(serde_json::json!({
            "provider": "none",
            "latencyMs": 0,
            "cached": false,
            "suggestions": [],
        }))
        .into_response();
    }
    let Some(registry) = resolve_completion_registry(&state).await else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "inline completion unavailable: no provider configured"
            })),
        )
            .into_response();
    };

    let prefix = clip_tail(&body.prefix, MAX_COMPLETION_WINDOW_BYTES).to_string();
    let suffix = clip_head(&body.suffix, MAX_COMPLETION_WINDOW_BYTES).to_string();
    let workspace_root = body
        .root
        .as_deref()
        .filter(|r| !r.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| state.live_config.load_ref().workspace_dir.clone());
    let file_path = body
        .path
        .as_deref()
        .filter(|p| !p.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root.join("<scratch>"));
    let language = file_path
        .extension()
        .and_then(|e| e.to_str())
        .map(crate::inline_completion::Language::from_extension)
        .unwrap_or(crate::inline_completion::Language::Other);
    let context =
        crate::inline_completion::context_builder::build_context_from_window(&prefix, &suffix);

    let req = crate::inline_completion::InlineCompletionRequest {
        prefix,
        suffix,
        language,
        file_path,
        workspace_root,
        context,
        max_tokens: body.max_tokens.unwrap_or(128).clamp(8, 512),
        stop_sequences: Vec::new(),
        request_id: uuid::Uuid::new_v4(),
    };

    match tokio::time::timeout(COMPLETION_TIMEOUT, registry.request(req)).await {
        Ok(Ok(resp)) => {
            let suggestions: Vec<serde_json::Value> = resp
                .suggestions
                .iter()
                .filter(|s| !s.insert_text.is_empty())
                .map(|s| {
                    serde_json::json!({
                        "insertText": s.insert_text,
                        "confidence": s.confidence,
                    })
                })
                .collect();
            Json(serde_json::json!({
                "provider": resp.provider,
                "latencyMs": resp.latency_ms,
                "cached": resp.cached,
                "suggestions": suggestions,
            }))
            .into_response()
        }
        Ok(Err(
            crate::inline_completion::InlineCompletionError::Empty { provider },
        )) => Json(serde_json::json!({
            "provider": provider,
            "latencyMs": 0,
            "cached": false,
            "suggestions": [],
        }))
        .into_response(),
        Ok(Err(crate::inline_completion::InlineCompletionError::Disabled { .. })) => {
            Json(serde_json::json!({
                "provider": "throttled",
                "latencyMs": 0,
                "cached": false,
                "suggestions": [],
            }))
            .into_response()
        }
        Ok(Err(e)) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
        Err(_) => (
            StatusCode::GATEWAY_TIMEOUT,
            Json(serde_json::json!({ "error": "inline completion timed out" })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct CompletionFeedbackBody {
    pub event: String,
}

pub async fn handle_editor_completion_feedback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CompletionFeedbackBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let event = match body.event.as_str() {
        "shown" => crate::inline_completion::AcceptanceEvent::Shown,
        "accepted" => crate::inline_completion::AcceptanceEvent::Accepted,
        "accepted_partial" => crate::inline_completion::AcceptanceEvent::AcceptedPartial,
        "rejected" => crate::inline_completion::AcceptanceEvent::Rejected,
        "timed_out" => crate::inline_completion::AcceptanceEvent::TimedOut,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "unknown feedback event" })),
            )
                .into_response();
        }
    };
    crate::inline_completion::global_stats().record(event);
    Json(serde_json::json!({ "ok": true })).into_response()
}

pub async fn handle_editor_completion_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let stats = crate::inline_completion::global_stats();
    let (shown, accepted, accepted_partial, rejected, timed_out) = stats.snapshot();
    Json(serde_json::json!({
        "shown": shown,
        "accepted": accepted,
        "acceptedPartial": accepted_partial,
        "rejected": rejected,
        "timedOut": timed_out,
        "acceptanceRate": stats.acceptance_rate(),
        "averageLatencyMs": stats.average_latency_ms(),
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineEditBody {
    pub path: String,
    pub selection: String,
    pub instruction: String,
    #[serde(default)]
    pub context_lines: Option<Vec<String>>,
}

pub async fn handle_editor_inline_edit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<InlineEditBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    if body.instruction.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "instruction is required" })),
        )
            .into_response();
    }
    if body.selection.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "selection is required" })),
        )
            .into_response();
    }
    if body.selection.len() > MAX_INLINE_EDIT_SELECTION_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(serde_json::json!({ "error": "selection too large for inline edit" })),
        )
            .into_response();
    }

    let runner = {
        let cfg = (*state.live_config.load_ref()).clone();
        tokio::task::spawn_blocking(move || crate::inline_edit::service::default_runner(&cfg))
            .await
            .ok()
            .flatten()
    };
    let Some(runner) = runner else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "inline edit unavailable: no provider configured"
            })),
        )
            .into_response();
    };

    let selection_len = body.selection.len();
    let req = crate::inline_edit::InlineEditRequest {
        file_path: PathBuf::from(&body.path),
        selection: body.selection.clone(),
        selection_bytes: (0, selection_len),
        instruction: body.instruction.clone(),
        context_lines: body.context_lines.clone(),
        request_id: uuid::Uuid::new_v4(),
    };

    match runner.run(&body.selection, req).await {
        Ok(outcome) => Json(serde_json::json!({
            "diff": outcome.diff,
            "applied": outcome.applied,
            "hunksExact": outcome.hunks_exact,
            "hunksFuzzy": outcome.hunks_fuzzy,
            "validatorIssues": outcome.validator_issues,
        }))
        .into_response(),
        Err(e) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
