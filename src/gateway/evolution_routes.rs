// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::AppState;
use super::api::require_auth;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
};
use chrono::Utc;
use serde::Deserialize;
use std::sync::Arc;

use crate::evolution::{
    self, DistillRequest, EvolutionEngine, ExportFilter, ExportOptions, Lesson, PurgeScope,
    ReflectionDepth, ReflectionTriggerMode, ReflectionWritebackTarget, ThumbVote,
};
use crate::evolution::types::EvolutionExportFormat;

fn engine_or_disabled() -> Result<Arc<EvolutionEngine>, (StatusCode, Json<serde_json::Value>)> {
    match evolution::try_global() {
        Some(engine) if engine.enabled() => Ok(engine),
        _ => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "evolution_disabled"})),
        )),
    }
}

fn engine_initialized() -> Result<Arc<EvolutionEngine>, (StatusCode, Json<serde_json::Value>)> {
    match evolution::try_global() {
        Some(engine) => Ok(engine),
        None => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "evolution_uninitialized"})),
        )),
    }
}

fn json_error(status: StatusCode, msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({"error": msg})))
}

fn json_error_with(
    status: StatusCode,
    msg: &str,
    extras: serde_json::Value,
) -> (StatusCode, Json<serde_json::Value>) {
    let mut body = serde_json::json!({"error": msg});
    if let serde_json::Value::Object(map) = extras {
        if let serde_json::Value::Object(target) = &mut body {
            for (k, v) in map {
                target.insert(k, v);
            }
        }
    }
    (status, Json(body))
}

fn collect_registered_model_names(
    state: &AppState,
) -> (Vec<String>, std::collections::HashSet<String>) {
    let cfg = state.config.lock();
    let mut ordered: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for profile in cfg.model_providers.values() {
        for name in effective_model_names_for_profile(profile) {
            if seen.insert(name.clone()) {
                ordered.push(name);
            }
        }
    }
    (ordered, seen)
}

fn collect_registered_provider_ids(state: &AppState) -> std::collections::HashSet<String> {
    let cfg = state.config.lock();
    cfg.model_providers.keys().cloned().collect()
}

fn effective_model_names_for_profile(
    profile: &crate::config::ModelProviderConfig,
) -> Vec<String> {
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

fn registered_models_payload(names: &[String]) -> serde_json::Value {
    serde_json::json!({
        "registeredModels": names,
        "providersConfigured": !names.is_empty(),
    })
}

pub async fn handle_overview(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_initialized() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    let store = engine.store();
    let snapshot = engine.config_snapshot();
    let persistence = store.persistence_status().unwrap_or_default();
    let lessons = store.list_lessons(false).unwrap_or_default();
    let active_lessons = lessons.iter().filter(|l| l.enabled).count();
    let total_hits: u64 = lessons.iter().map(|l| l.hits).sum();
    let exports = store.list_exports().unwrap_or_default();
    let judge_metrics = engine.judge_worker_metrics();
    let scheduler_metrics = engine.reflection_scheduler_metrics();
    let recycling_metrics = engine.recycling_metrics();
    let services_opt = crate::services::try_get_services();
    let tools_metrics = services_opt
        .map(|svc| svc.tool_search_metrics_snapshot())
        .unwrap_or_default();
    let deferred_builtin_count = services_opt
        .map(|svc| svc.deferred_builtin_names.read().len() as u64)
        .unwrap_or(0);
    Json(serde_json::json!({
        "enabled": snapshot.enabled,
        "persistTrainingData": snapshot.persist_training_data,
        "nextStateJudgeEnabled": snapshot.next_state_judge_enabled,
        "judgeModel": snapshot.judge_model,
        "totalTurns": persistence.turns_count,
        "lessonsTotal": lessons.len(),
        "lessonsActive": active_lessons,
        "lessonHitsTotal": total_hits,
        "exportsCount": persistence.exports_count,
        "exportsBytes": persistence.exports_total_bytes,
        "turnsFileSize": persistence.turns_file_size,
        "eventsFileSize": persistence.events_file_size,
        "pushReceiptsCount": persistence.push_receipts_count,
        "exports": exports.iter().take(5).map(|e| serde_json::json!({
            "id": e.id,
            "format": e.format.as_str(),
            "sampleCount": e.sample_count,
            "sizeBytes": e.size_bytes,
            "createdAt": e.created_at,
        })).collect::<Vec<_>>(),
        "judgeWorker": {
            "running": judge_metrics.running,
            "enqueuedTotal": judge_metrics.enqueued_total,
            "processed": judge_metrics.processed_total,
            "lastErrorAt": judge_metrics.last_error_at,
            "lastErrorMessage": judge_metrics.last_error_message,
        },
        "reflectionScheduler": {
            "running": scheduler_metrics.running,
            "intervalMinutes": scheduler_metrics.interval_minutes,
            "lastTickAt": scheduler_metrics.last_tick_at,
            "nextTickAtEstimate": scheduler_metrics.next_tick_at_estimate,
        },
        "recycling": {
            "totalHarvested": recycling_metrics.total_harvested,
            "recent24hHarvested": recycling_metrics.recent_24h_harvested,
            "lastHarvestAt": recycling_metrics.last_harvest_at,
        },
        "tools": {
            "invocations": tools_metrics.invocations,
            "activations": tools_metrics.activations,
            "highRiskBlocked": tools_metrics.high_risk_blocked,
            "avgLatencyMs": tools_metrics.avg_latency_ms,
            "deferredBuiltinCount": deferred_builtin_count,
        },
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
pub struct LessonsQuery {
    #[serde(default)]
    pub only_enabled: Option<bool>,
}

pub async fn handle_lessons_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<LessonsQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_or_disabled() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    let store = engine.store();
    let only_enabled = q.only_enabled.unwrap_or(false);
    let lessons = match store.list_lessons(only_enabled) {
        Ok(l) => l,
        Err(error) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string())
                .into_response();
        }
    };
    let payload: Vec<serde_json::Value> = lessons.into_iter().map(lesson_to_json).collect();
    Json(serde_json::json!({ "items": payload })).into_response()
}

fn lesson_to_json(l: Lesson) -> serde_json::Value {
    serde_json::json!({
        "id": l.id,
        "title": l.title,
        "body": l.body,
        "tags": l.tags,
        "codingMode": l.coding_mode,
        "sourceTurnIds": l.source_turn_ids,
        "hits": l.hits,
        "enabled": l.enabled,
        "createdAt": l.created_at,
        "updatedAt": l.updated_at,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LessonUpsert {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default, alias = "coding_mode")]
    pub coding_mode: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

pub async fn handle_lesson_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<LessonUpsert>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_or_disabled() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    let store = engine.store();
    let lessons = match store.list_lessons(false) {
        Ok(l) => l,
        Err(error) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string())
                .into_response();
        }
    };
    let mut existing = match lessons.into_iter().find(|l| l.id == id) {
        Some(l) => l,
        None => return json_error(StatusCode::NOT_FOUND, "lesson_not_found").into_response(),
    };
    if let Some(title) = body.title {
        existing.title = title;
    }
    if let Some(body) = body.body {
        existing.body = body;
    }
    if let Some(tags) = body.tags {
        existing.tags = tags;
    }
    if let Some(mode) = body.coding_mode {
        existing.coding_mode = if mode.is_empty() { None } else { Some(mode) };
    }
    if let Some(enabled) = body.enabled {
        existing.enabled = enabled;
    }
    existing.updated_at = Utc::now();
    if let Err(error) = store.upsert_lesson(&existing) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()).into_response();
    }
    Json(lesson_to_json(existing)).into_response()
}

pub async fn handle_lesson_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_or_disabled() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    match engine.store().delete_lesson(&id) {
        Ok(true) => Json(serde_json::json!({"ok": true})).into_response(),
        Ok(false) => json_error(StatusCode::NOT_FOUND, "lesson_not_found").into_response(),
        Err(error) => {
            json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()).into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbBody {
    #[serde(alias = "session_id")]
    pub session_id: String,
    #[serde(alias = "turn_id")]
    pub turn_id: String,
    pub score: i8,
    #[serde(default)]
    pub comment: Option<String>,
}

pub async fn handle_thumbs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ThumbBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_or_disabled() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    let vote = ThumbVote {
        id: format!("thumb_{}", uuid::Uuid::new_v4().simple()),
        session_id: body.session_id,
        turn_id: body.turn_id.clone(),
        score: body.score.clamp(-1, 1),
        comment: body.comment,
        ts: Utc::now(),
    };
    let vote_id = vote.id.clone();
    let audit_only = !engine.persist_training_data();
    match engine.submit_thumb_vote(vote, None) {
        Ok(merged) => Json(serde_json::json!({
            "ok": true,
            "voteId": vote_id,
            "finalReward": merged.final_score,
            "auditOnly": audit_only,
        }))
        .into_response(),
        Err(error) => {
            tracing::warn!(error = %error, "failed to submit thumb vote");
            json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()).into_response()
        }
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DistillBody {
    #[serde(default, alias = "turn_id")]
    pub turn_id: Option<String>,
}

pub async fn handle_distill(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<DistillBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_or_disabled() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    let turn_id = match body.turn_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_string(),
        None => {
            return json_error(StatusCode::BAD_REQUEST, "missing_turn_id").into_response();
        }
    };
    if !engine.persist_training_data() {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "persistence_disabled"})),
        )
            .into_response();
    }
    let store = engine.store();
    let turn = match store.find_turn_record(&turn_id) {
        Ok(Some(t)) => t,
        Ok(None) => {
            return json_error(StatusCode::NOT_FOUND, "turn_not_found").into_response();
        }
        Err(error) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string())
                .into_response();
        }
    };
    if let Err(error) = engine.enqueue_distill_forced(DistillRequest { turn }) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()).into_response();
    }
    Json(serde_json::json!({"ok": true, "queued": true, "turnId": turn_id})).into_response()
}

pub async fn handle_rescore(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_or_disabled() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    if !engine.persist_training_data() {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "persistence_disabled"})),
        )
            .into_response();
    }
    let store = engine.store();
    let weights = engine.config_snapshot().signal_weights;
    let mut rescored: u64 = 0;
    let mut errors: u64 = 0;
    let outcome = store.for_each_turn(|mut turn| {
        let scores = crate::evolution::run_fast_evaluators(&turn);
        turn.reward = crate::evolution::fuse_signals(&scores, &weights);
        match store.update_turn_reward(&turn.id, &turn.reward) {
            Ok(()) => rescored = rescored.saturating_add(1),
            Err(_) => errors = errors.saturating_add(1),
        }
        Ok(())
    });
    let total_seen = match outcome {
        Ok(n) => n,
        Err(error) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string())
                .into_response();
        }
    };
    Json(serde_json::json!({
        "ok": true,
        "rescored": rescored,
        "errors": errors,
        "totalSeen": total_seen,
    }))
    .into_response()
}

pub async fn handle_config_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_initialized() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    let snapshot = engine.config_snapshot();
    Json(serde_json::json!({
        "enabled": snapshot.enabled,
        "persistTrainingData": snapshot.persist_training_data,
        "nextStateJudgeEnabled": snapshot.next_state_judge_enabled,
        "judgeModel": snapshot.judge_model,
        "signalWeights": {
            "thumbs": snapshot.signal_weights.thumbs,
            "nextState": snapshot.signal_weights.next_state,
            "tool": snapshot.signal_weights.tool,
            "verification": snapshot.signal_weights.verification,
            "cost": snapshot.signal_weights.cost,
        },
        "maxLessonsInPrompt": snapshot.max_lessons_in_prompt,
        "lessonTokenBudget": snapshot.lesson_token_budget,
        "autoDistillOnSessionEnd": snapshot.auto_distill_on_session_end,
        "export": {
            "defaultFormat": snapshot.export.default_format.as_str(),
            "autoPush": snapshot.export.auto_push,
            "autoPushTargetId": snapshot.export.auto_push_target_id,
            "autoPushMinSamples": snapshot.export.auto_push_min_samples,
            "autoPushMinIntervalHours": snapshot.export.auto_push_min_interval_hours,
            "redactWorkspacePaths": snapshot.export.redact_workspace_paths,
            "redactSecrets": snapshot.export.redact_secrets,
        },
    }))
    .into_response()
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConfigPutBody {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default, alias = "next_state_judge_enabled")]
    pub next_state_judge_enabled: Option<bool>,
    #[serde(default, alias = "judge_model")]
    pub judge_model: Option<String>,
    #[serde(default, alias = "signal_weights")]
    pub signal_weights: Option<SignalWeightsBody>,
    #[serde(default, alias = "max_lessons_in_prompt")]
    pub max_lessons_in_prompt: Option<usize>,
    #[serde(default, alias = "lesson_token_budget")]
    pub lesson_token_budget: Option<usize>,
    #[serde(default, alias = "auto_distill_on_session_end")]
    pub auto_distill_on_session_end: Option<bool>,
    #[serde(default)]
    pub export: Option<ExportConfigBody>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalWeightsBody {
    #[serde(default)]
    pub thumbs: Option<f32>,
    #[serde(default, alias = "next_state")]
    pub next_state: Option<f32>,
    #[serde(default)]
    pub tool: Option<f32>,
    #[serde(default)]
    pub verification: Option<f32>,
    #[serde(default)]
    pub cost: Option<f32>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExportConfigBody {
    #[serde(default, alias = "default_format")]
    pub default_format: Option<String>,
    #[serde(default, alias = "auto_push")]
    pub auto_push: Option<bool>,
    #[serde(default, alias = "auto_push_target_id")]
    pub auto_push_target_id: Option<String>,
    #[serde(default, alias = "auto_push_min_samples")]
    pub auto_push_min_samples: Option<usize>,
    #[serde(default, alias = "auto_push_min_interval_hours")]
    pub auto_push_min_interval_hours: Option<u32>,
    #[serde(default, alias = "redact_workspace_paths")]
    pub redact_workspace_paths: Option<bool>,
    #[serde(default, alias = "redact_secrets")]
    pub redact_secrets: Option<bool>,
}

pub async fn handle_config_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ConfigPutBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_initialized() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    let mut snapshot = engine.config_snapshot();
    let (registered_names, registered_set) = collect_registered_model_names(&state);
    if let Some(v) = body.enabled {
        snapshot.enabled = v;
    }
    if let Some(v) = body.next_state_judge_enabled {
        snapshot.next_state_judge_enabled = v;
    }
    if let Some(raw) = body.judge_model {
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() {
            snapshot.judge_model = None;
        } else if registered_set.contains(&trimmed) {
            snapshot.judge_model = Some(trimmed);
        } else {
            return json_error_with(
                StatusCode::BAD_REQUEST,
                "model_not_registered",
                registered_models_payload(&registered_names),
            )
            .into_response();
        }
    }
    if let Some(weights) = body.signal_weights {
        if let Some(v) = weights.thumbs {
            snapshot.signal_weights.thumbs = v;
        }
        if let Some(v) = weights.next_state {
            snapshot.signal_weights.next_state = v;
        }
        if let Some(v) = weights.tool {
            snapshot.signal_weights.tool = v;
        }
        if let Some(v) = weights.verification {
            snapshot.signal_weights.verification = v;
        }
        if let Some(v) = weights.cost {
            snapshot.signal_weights.cost = v;
        }
    }
    if let Some(v) = body.max_lessons_in_prompt {
        snapshot.max_lessons_in_prompt = v.clamp(0, 32);
    }
    if let Some(v) = body.lesson_token_budget {
        snapshot.lesson_token_budget = v.clamp(64, 16_000);
    }
    if let Some(v) = body.auto_distill_on_session_end {
        snapshot.auto_distill_on_session_end = v;
    }
    if let Some(export) = body.export {
        if let Some(fmt) = export.default_format {
            if let Some(parsed) =
                crate::evolution::types::EvolutionExportFormat::parse(&fmt)
            {
                snapshot.export.default_format = parsed;
            }
        }
        if let Some(v) = export.auto_push {
            snapshot.export.auto_push = v;
        }
        if let Some(v) = export.auto_push_target_id {
            snapshot.export.auto_push_target_id = if v.is_empty() { None } else { Some(v) };
        }
        if let Some(v) = export.auto_push_min_samples {
            snapshot.export.auto_push_min_samples = v.clamp(1, 1_000_000);
        }
        if let Some(v) = export.auto_push_min_interval_hours {
            snapshot.export.auto_push_min_interval_hours = v.clamp(0, 24 * 365);
        }
        if let Some(v) = export.redact_workspace_paths {
            snapshot.export.redact_workspace_paths = v;
        }
        if let Some(v) = export.redact_secrets {
            snapshot.export.redact_secrets = v;
        }
    }
    let snapshot_full: crate::config::Config = {
        let mut cfg = state.config.lock();
        cfg.evolution = snapshot.clone();
        cfg.clone()
    };
    if let Err(e) = snapshot_full.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "config_save_failed",
                "detail": format!("{e:#}"),
            })),
        )
            .into_response();
    }
    state.push_live_config(snapshot_full);
    engine.set_config(snapshot);
    Json(serde_json::json!({"ok": true})).into_response()
}

pub async fn handle_persistence_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_initialized() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    let snapshot = engine.config_snapshot();
    let status = engine.store().persistence_status().unwrap_or_default();
    Json(serde_json::json!({
        "persistTrainingData": snapshot.persist_training_data,
        "turnsCount": status.turns_count,
        "turnsFileSize": status.turns_file_size,
        "eventsFileSize": status.events_file_size,
        "exportsCount": status.exports_count,
        "exportsTotalBytes": status.exports_total_bytes,
        "pushReceiptsCount": status.push_receipts_count,
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistencePutBody {
    #[serde(alias = "persist_training_data")]
    pub persist_training_data: bool,
}

pub async fn handle_persistence_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PersistencePutBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_initialized() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    let snapshot_full: crate::config::Config = {
        let mut cfg = state.config.lock();
        cfg.evolution.persist_training_data = body.persist_training_data;
        cfg.clone()
    };
    if let Err(e) = snapshot_full.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "config_save_failed",
                "detail": format!("{e:#}"),
            })),
        )
            .into_response();
    }
    state.push_live_config(snapshot_full);
    engine.set_persist_training_data(body.persist_training_data);
    Json(serde_json::json!({
        "ok": true,
        "persistTrainingData": body.persist_training_data,
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurgeBody {
    pub scope: String,
    #[serde(default, alias = "before_ms")]
    pub before_ms: Option<i64>,
    pub confirm: String,
}

pub async fn handle_persistence_purge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PurgeBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    if body.confirm != "I_UNDERSTAND" {
        return json_error(StatusCode::PRECONDITION_FAILED, "missing_confirmation").into_response();
    }
    let engine = match engine_or_disabled() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    let scope = match PurgeScope::parse(&body.scope) {
        Some(s) => s,
        None => return json_error(StatusCode::BAD_REQUEST, "invalid_scope").into_response(),
    };
    let report = match engine.store().purge(scope, body.before_ms) {
        Ok(r) => r,
        Err(error) => {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string())
                .into_response();
        }
    };
    Json(serde_json::json!({
        "ok": true,
        "scope": body.scope,
        "removedTurns": report.turns,
        "removedExports": report.exports,
        "removedPushHistory": report.push_history,
        "removedEvents": report.events,
        "freedBytes": report.freed_bytes,
    }))
    .into_response()
}

pub async fn handle_export_formats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let formats: Vec<serde_json::Value> = EvolutionExportFormat::all()
        .iter()
        .map(|f| {
            serde_json::json!({
                "id": f.as_str(),
                "label": f.as_str(),
            })
        })
        .collect();
    Json(serde_json::json!({"items": formats})).into_response()
}

#[derive(Debug, Deserialize, Default)]
pub struct ExportCreateBody {
    pub format: String,
    #[serde(default)]
    pub filter: ExportFilter,
    #[serde(default)]
    pub options: ExportOptions,
    #[serde(default)]
    pub preview: Option<bool>,
}

pub async fn handle_export_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ExportCreateBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_or_disabled() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    let format = match EvolutionExportFormat::parse(&body.format) {
        Some(f) => f,
        None => return json_error(StatusCode::BAD_REQUEST, "invalid_format").into_response(),
    };
    if !engine.persist_training_data() {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "persistence_disabled"})),
        )
            .into_response();
    }
    if body.preview.unwrap_or(false) {
        match crate::evolution::preview_export(&engine, format, &body.filter, &body.options, 5) {
            Ok(preview) => Json(serde_json::json!({
                "format": preview.format.as_str(),
                "totalEligible": preview.total_eligible,
                "samples": preview.samples,
            }))
            .into_response(),
            Err(error) => {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()).into_response()
            }
        }
    } else {
        match crate::evolution::export_to_file(&engine, format, &body.filter, &body.options) {
            Ok(record) => Json(export_record_to_json(record)).into_response(),
            Err(error) => {
                let msg = error.to_string();
                if msg == "persistence_disabled" {
                    return (
                        StatusCode::CONFLICT,
                        Json(serde_json::json!({"error": "persistence_disabled"})),
                    )
                        .into_response();
                }
                json_error(StatusCode::INTERNAL_SERVER_ERROR, &msg).into_response()
            }
        }
    }
}

pub async fn handle_export_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_or_disabled() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    let exports = engine.store().list_exports().unwrap_or_default();
    let items: Vec<serde_json::Value> = exports.into_iter().map(export_record_to_json).collect();
    Json(serde_json::json!({"items": items})).into_response()
}

pub async fn handle_export_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_or_disabled() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    let store = std::sync::Arc::clone(engine.store());
    let id_owned = id.clone();
    let result = tokio::task::spawn_blocking(move || {
        let record = match store.get_export(&id_owned) {
            Ok(Some(r)) => r,
            Ok(None) => return Err(None),
            Err(error) => return Err(Some(error.to_string())),
        };
        let _ = std::fs::remove_file(&record.path);
        let _ = store.delete_export(&id_owned);
        Ok(())
    })
    .await;
    match result {
        Ok(Ok(())) => Json(serde_json::json!({"ok": true})).into_response(),
        Ok(Err(None)) => json_error(StatusCode::NOT_FOUND, "export_not_found").into_response(),
        Ok(Err(Some(msg))) => {
            json_error(StatusCode::INTERNAL_SERVER_ERROR, &msg).into_response()
        }
        Err(_) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "delete_failed").into_response(),
    }
}

fn export_record_to_json(record: crate::evolution::types::ExportRecord) -> serde_json::Value {
    serde_json::json!({
        "id": record.id,
        "format": record.format.as_str(),
        "path": record.path,
        "sampleCount": record.sample_count,
        "sizeBytes": record.size_bytes,
        "contentDigest": record.md5,
        "digestAlgorithm": "md5",
        "timeWindowStart": record.time_window_start,
        "timeWindowEnd": record.time_window_end,
        "createdAt": record.created_at,
    })
}

fn target_to_json(t: crate::evolution::types::CloudTarget) -> serde_json::Value {
    serde_json::json!({
        "id": t.id,
        "name": t.name,
        "kind": t.kind.as_str(),
        "endpoint": t.endpoint,
        "headers": t.headers,
        "secretRef": t.secret_ref,
        "defaultFormat": t.default_format.as_str(),
        "enabled": t.enabled,
        "autoPush": t.auto_push,
        "autoPushMinSamples": t.auto_push_min_samples,
        "autoPushMinIntervalHours": t.auto_push_min_interval_hours,
        "lastPushedAt": t.last_pushed_at,
        "createdAt": t.created_at,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudTargetUpsertBody {
    pub id: Option<String>,
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,
    #[serde(default, alias = "secret_ref")]
    pub secret_ref: Option<String>,
    #[serde(default, alias = "default_format")]
    pub default_format: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, alias = "auto_push")]
    pub auto_push: bool,
    #[serde(default, alias = "auto_push_min_samples")]
    pub auto_push_min_samples: u32,
    #[serde(default, alias = "auto_push_min_interval_hours")]
    pub auto_push_min_interval_hours: u32,
}

fn default_true() -> bool {
    true
}

pub async fn handle_cloud_targets_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_or_disabled() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    let targets = engine.store().list_cloud_targets().unwrap_or_default();
    let items: Vec<serde_json::Value> = targets.into_iter().map(target_to_json).collect();
    Json(serde_json::json!({"items": items})).into_response()
}

pub async fn handle_cloud_targets_upsert(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CloudTargetUpsertBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_or_disabled() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    let kind = match crate::evolution::types::CloudTargetKind::parse(&body.kind) {
        Some(k) => k,
        None => return json_error(StatusCode::BAD_REQUEST, "invalid_kind").into_response(),
    };
    let default_format = match body.default_format.as_deref() {
        Some(s) => crate::evolution::types::EvolutionExportFormat::parse(s)
            .unwrap_or(crate::evolution::types::EvolutionExportFormat::default()),
        None => crate::evolution::types::EvolutionExportFormat::default(),
    };
    let id = body
        .id
        .clone()
        .unwrap_or_else(|| format!("target_{}", uuid::Uuid::new_v4().simple()));
    let target = crate::evolution::types::CloudTarget {
        id: id.clone(),
        name: body.name,
        kind,
        endpoint: body.endpoint,
        headers: body.headers,
        secret_ref: body.secret_ref,
        default_format,
        enabled: body.enabled,
        auto_push: body.auto_push,
        auto_push_min_samples: body.auto_push_min_samples,
        auto_push_min_interval_hours: body.auto_push_min_interval_hours,
        last_pushed_at: None,
        created_at: Utc::now(),
    };
    if let Err(error) = engine.store().upsert_cloud_target(&target) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()).into_response();
    }
    Json(target_to_json(target)).into_response()
}

pub async fn handle_cloud_target_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_or_disabled() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    match engine.store().delete_cloud_target(&id) {
        Ok(true) => Json(serde_json::json!({"ok": true})).into_response(),
        Ok(false) => json_error(StatusCode::NOT_FOUND, "target_not_found").into_response(),
        Err(error) => {
            json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()).into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushBody {
    #[serde(alias = "target_id")]
    pub target_id: String,
    #[serde(alias = "export_id")]
    pub export_id: String,
}

pub async fn handle_cloud_push(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PushBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_or_disabled() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    if !engine.persist_training_data() {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "persistence_disabled"})),
        )
            .into_response();
    }
    match crate::evolution::push_export_to_target(&engine, &body.target_id, &body.export_id).await {
        Ok(receipt) => Json(serde_json::json!({
            "id": receipt.id,
            "exportId": receipt.export_id,
            "targetId": receipt.target_id,
            "status": receipt.status,
            "latencyMs": receipt.latency_ms,
            "responseExcerpt": receipt.response_excerpt,
            "ts": receipt.ts,
        }))
        .into_response(),
        Err(error) => {
            json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()).into_response()
        }
    }
}

pub async fn handle_push_history(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_or_disabled() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    let receipts = engine.store().list_push_receipts(50).unwrap_or_default();
    let items: Vec<serde_json::Value> = receipts
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id,
                "exportId": r.export_id,
                "targetId": r.target_id,
                "status": r.status,
                "latencyMs": r.latency_ms,
                "responseExcerpt": r.response_excerpt,
                "ts": r.ts,
            })
        })
        .collect();
    Json(serde_json::json!({"items": items})).into_response()
}

pub async fn handle_recycling_config_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_initialized() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    let snapshot = engine.config_snapshot();
    Json(recycling_config_to_json(&snapshot.recycling)).into_response()
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RecyclingConfigPutBody {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default, alias = "sample_rate")]
    pub sample_rate: Option<f32>,
    #[serde(default, alias = "min_reward")]
    pub min_reward: Option<f32>,
    #[serde(default, alias = "max_retained")]
    pub max_retained: Option<usize>,
    #[serde(default, alias = "max_replay_in_prompt")]
    pub max_replay_in_prompt: Option<usize>,
    #[serde(default, alias = "replay_token_budget")]
    pub replay_token_budget: Option<usize>,
    #[serde(default, alias = "redact_workspace_paths")]
    pub redact_workspace_paths: Option<bool>,
    #[serde(default, alias = "redact_secrets")]
    pub redact_secrets: Option<bool>,
    #[serde(default, alias = "redact_user_text")]
    pub redact_user_text: Option<bool>,
    #[serde(default, alias = "include_successes")]
    pub include_successes: Option<bool>,
    #[serde(default, alias = "include_failures")]
    pub include_failures: Option<bool>,
    #[serde(default, alias = "weight_quality")]
    pub weight_quality: Option<f32>,
    #[serde(default, alias = "weight_recency")]
    pub weight_recency: Option<f32>,
    #[serde(default, alias = "weight_diversity")]
    pub weight_diversity: Option<f32>,
}

pub async fn handle_recycling_config_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RecyclingConfigPutBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_initialized() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    let mut snapshot = engine.config_snapshot();
    let cfg = &mut snapshot.recycling;
    if let Some(v) = body.enabled {
        cfg.enabled = v;
    }
    if let Some(v) = body.sample_rate {
        cfg.sample_rate = v.clamp(0.0, 1.0);
    }
    if let Some(v) = body.min_reward {
        cfg.min_reward = v.clamp(-1.0, 1.0);
    }
    if let Some(v) = body.max_retained {
        cfg.max_retained = v.clamp(0, 100_000);
    }
    if let Some(v) = body.max_replay_in_prompt {
        cfg.max_replay_in_prompt = v.clamp(0, 32);
    }
    if let Some(v) = body.replay_token_budget {
        cfg.replay_token_budget = v.clamp(64, 16_000);
    }
    if let Some(v) = body.redact_workspace_paths {
        cfg.redact_workspace_paths = v;
    }
    if let Some(v) = body.redact_secrets {
        cfg.redact_secrets = v;
    }
    if let Some(v) = body.redact_user_text {
        cfg.redact_user_text = v;
    }
    if let Some(v) = body.include_successes {
        cfg.include_successes = v;
    }
    if let Some(v) = body.include_failures {
        cfg.include_failures = v;
    }
    if let Some(v) = body.weight_quality {
        cfg.weight_quality = v.clamp(0.0, 1.0);
    }
    if let Some(v) = body.weight_recency {
        cfg.weight_recency = v.clamp(0.0, 1.0);
    }
    if let Some(v) = body.weight_diversity {
        cfg.weight_diversity = v.clamp(0.0, 1.0);
    }
    let auto_enable_persist =
        snapshot.recycling.enabled && !snapshot.persist_training_data;
    if auto_enable_persist {
        snapshot.persist_training_data = true;
    }
    let snapshot_full: crate::config::Config = {
        let mut cfg = state.config.lock();
        cfg.evolution = snapshot.clone();
        cfg.clone()
    };
    if let Err(e) = snapshot_full.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "config_save_failed",
                "detail": format!("{e:#}"),
            })),
        )
            .into_response();
    }
    state.push_live_config(snapshot_full);
    engine.set_config(snapshot.clone());
    let mut body = recycling_config_to_json(&snapshot.recycling);
    if let serde_json::Value::Object(ref mut map) = body {
        map.insert(
            "persistTrainingDataAutoEnabled".to_string(),
            serde_json::Value::Bool(auto_enable_persist),
        );
        map.insert(
            "persistTrainingData".to_string(),
            serde_json::Value::Bool(snapshot.persist_training_data),
        );
    }
    Json(body).into_response()
}

pub async fn handle_recycling_recent(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_initialized() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    let Some(store) = engine.recycling_store() else {
        return Json(serde_json::json!({"items": Vec::<serde_json::Value>::new()})).into_response();
    };
    let limit: usize = 25;
    let items = store.list_recent(limit).unwrap_or_default();
    let total = store.count().unwrap_or(0);
    let payload: Vec<serde_json::Value> = items
        .into_iter()
        .map(|exp| {
            serde_json::json!({
                "id": exp.id,
                "sessionId": exp.session_id,
                "turnId": exp.turn_id,
                "codingMode": exp.coding_mode,
                "outcome": exp.outcome.as_str(),
                "reward": exp.reward,
                "headline": exp.headline,
                "contextExcerpt": exp.context_excerpt,
                "responseExcerpt": exp.response_excerpt,
                "toolsSummary": exp.tools_summary,
                "tags": exp.tags,
                "hits": exp.hits,
                "createdAt": exp.created_at,
            })
        })
        .collect();
    Json(serde_json::json!({"items": payload, "total": total})).into_response()
}

pub async fn handle_recycling_purge(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_initialized() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    let Some(store) = engine.recycling_store() else {
        return Json(serde_json::json!({"ok": true, "removed": 0})).into_response();
    };
    let removed = store.purge_all().unwrap_or(0);
    Json(serde_json::json!({"ok": true, "removed": removed})).into_response()
}

fn recycling_config_to_json(
    cfg: &crate::evolution::ExperienceRecyclingConfig,
) -> serde_json::Value {
    serde_json::json!({
        "enabled": cfg.enabled,
        "sampleRate": cfg.sample_rate,
        "minReward": cfg.min_reward,
        "maxRetained": cfg.max_retained,
        "maxReplayInPrompt": cfg.max_replay_in_prompt,
        "replayTokenBudget": cfg.replay_token_budget,
        "redactWorkspacePaths": cfg.redact_workspace_paths,
        "redactSecrets": cfg.redact_secrets,
        "redactUserText": cfg.redact_user_text,
        "includeSuccesses": cfg.include_successes,
        "includeFailures": cfg.include_failures,
        "weightQuality": cfg.weight_quality,
        "weightRecency": cfg.weight_recency,
        "weightDiversity": cfg.weight_diversity,
    })
}

pub async fn handle_reflection_config_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_initialized() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    let snapshot = engine.config_snapshot();
    Json(reflection_config_to_json(&snapshot.reflection)).into_response()
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReflectionConfigPutBody {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default, alias = "trigger_mode")]
    pub trigger_mode: Option<String>,
    #[serde(default)]
    pub depth: Option<String>,
    #[serde(default, alias = "reflection_model")]
    pub reflection_model: Option<String>,
    #[serde(default, alias = "reflection_provider")]
    pub reflection_provider: Option<String>,
    #[serde(default, alias = "schedule_interval_minutes")]
    pub schedule_interval_minutes: Option<u32>,
    #[serde(default, alias = "min_turns_for_auto")]
    pub min_turns_for_auto: Option<usize>,
    #[serde(default, alias = "failure_threshold")]
    pub failure_threshold: Option<u32>,
    #[serde(default, alias = "writeback_targets")]
    pub writeback_targets: Option<Vec<String>>,
    #[serde(default, alias = "max_lessons_per_run")]
    pub max_lessons_per_run: Option<usize>,
    #[serde(default, alias = "max_total_lessons")]
    pub max_total_lessons: Option<usize>,
    #[serde(default, alias = "include_user_thumbs_down")]
    pub include_user_thumbs_down: Option<bool>,
    #[serde(default, alias = "lookback_turns")]
    pub lookback_turns: Option<usize>,
}

pub async fn handle_reflection_config_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ReflectionConfigPutBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_initialized() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    let mut snapshot = engine.config_snapshot();
    let (registered_names, registered_set) = collect_registered_model_names(&state);
    let registered_provider_ids = collect_registered_provider_ids(&state);
    let cfg = &mut snapshot.reflection;
    if let Some(v) = body.enabled {
        cfg.enabled = v;
    }
    if let Some(v) = body.trigger_mode {
        if let Some(parsed) = ReflectionTriggerMode::parse(&v) {
            cfg.trigger_mode = parsed;
        }
    }
    if let Some(v) = body.depth {
        if let Some(parsed) = ReflectionDepth::parse(&v) {
            cfg.depth = parsed;
        }
    }
    if let Some(raw) = body.reflection_model {
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() {
            cfg.reflection_model = None;
        } else if registered_set.contains(&trimmed) {
            cfg.reflection_model = Some(trimmed);
        } else {
            return json_error_with(
                StatusCode::BAD_REQUEST,
                "model_not_registered",
                registered_models_payload(&registered_names),
            )
            .into_response();
        }
    }
    if let Some(raw) = body.reflection_provider {
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() {
            cfg.reflection_provider = None;
        } else if registered_provider_ids.contains(&trimmed) {
            cfg.reflection_provider = Some(trimmed);
        } else {
            return json_error_with(
                StatusCode::BAD_REQUEST,
                "provider_not_registered",
                serde_json::json!({
                    "registeredProviderIds": registered_provider_ids.iter().cloned().collect::<Vec<_>>(),
                }),
            )
            .into_response();
        }
    }
    if let Some(v) = body.schedule_interval_minutes {
        cfg.schedule_interval_minutes = v.clamp(5, 24 * 60);
    }
    if let Some(v) = body.min_turns_for_auto {
        cfg.min_turns_for_auto = v.clamp(1, 64);
    }
    if let Some(v) = body.failure_threshold {
        cfg.failure_threshold = v.clamp(1, 32);
    }
    if let Some(values) = body.writeback_targets {
        let mut parsed: Vec<ReflectionWritebackTarget> = Vec::new();
        for raw in values {
            if let Some(target) = ReflectionWritebackTarget::parse(&raw) {
                if !parsed.contains(&target) {
                    parsed.push(target);
                }
            }
        }
        if parsed.is_empty() {
            parsed.push(ReflectionWritebackTarget::Lessons);
        }
        cfg.writeback_targets = parsed;
    }
    if let Some(v) = body.max_lessons_per_run {
        cfg.max_lessons_per_run = v.clamp(1, 16);
    }
    if let Some(v) = body.max_total_lessons {
        cfg.max_total_lessons = v.clamp(1, 10_000);
    }
    if let Some(v) = body.include_user_thumbs_down {
        cfg.include_user_thumbs_down = v;
    }
    if let Some(v) = body.lookback_turns {
        cfg.lookback_turns = v.clamp(1, 64);
    }
    let auto_enable_persist =
        snapshot.reflection.enabled && !snapshot.persist_training_data;
    if auto_enable_persist {
        snapshot.persist_training_data = true;
    }
    let snapshot_full: crate::config::Config = {
        let mut cfg = state.config.lock();
        cfg.evolution = snapshot.clone();
        cfg.clone()
    };
    if let Err(e) = snapshot_full.save().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "config_save_failed",
                "detail": format!("{e:#}"),
            })),
        )
            .into_response();
    }
    state.push_live_config(snapshot_full);
    engine.set_config(snapshot.clone());
    let mut body = reflection_config_to_json(&snapshot.reflection);
    if let serde_json::Value::Object(ref mut map) = body {
        map.insert(
            "persistTrainingDataAutoEnabled".to_string(),
            serde_json::Value::Bool(auto_enable_persist),
        );
        map.insert(
            "persistTrainingData".to_string(),
            serde_json::Value::Bool(snapshot.persist_training_data),
        );
    }
    Json(body).into_response()
}

pub async fn handle_reflection_runs(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_initialized() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    if let Some(detail) = engine.reflection_store_health() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "reflection_store_unavailable",
                "detail": detail,
            })),
        )
            .into_response();
    }
    let Some(store) = engine.reflection_store() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "reflection_store_unavailable",
                "detail": "reflection_store_bind_failed",
            })),
        )
            .into_response();
    };
    let runs = store.list_recent(20).unwrap_or_default();
    let summary = store.summary().unwrap_or_default();
    let avg_lessons = store.average_lessons_per_run();
    let last_run = store.last_run_at_and_status();
    let items: Vec<serde_json::Value> = runs
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id,
                "sessionId": r.session_id,
                "trigger": r.trigger,
                "depth": r.depth,
                "status": r.status.as_str(),
                "model": r.model,
                "lessonsProduced": r.lessons_produced,
                "turnsAnalyzed": r.turns_analyzed,
                "summary": r.summary,
                "error": r.error,
                "startedAt": r.started_at,
                "completedAt": r.completed_at,
            })
        })
        .collect();
    let (last_run_at_value, last_status_value) = match (last_run.clone(), summary.last_run_at) {
        (Some((ts, status)), _) => (
            serde_json::Value::String(ts.to_rfc3339()),
            serde_json::Value::String(status),
        ),
        (None, Some(ts)) => (
            serde_json::Value::String(ts.to_rfc3339()),
            summary
                .last_status
                .clone()
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null),
        ),
        _ => (serde_json::Value::Null, serde_json::Value::Null),
    };
    Json(serde_json::json!({
        "items": items,
        "summary": {
            "totalRuns": summary.total_runs,
            "completedRuns": summary.completed_runs,
            "failedRuns": summary.failed_runs,
            "lastRunAt": last_run_at_value,
            "lastStatus": last_status_value,
            "totalLessonsProduced": summary.total_lessons_produced,
            "avgLessonsPerRun": avg_lessons,
        }
    }))
    .into_response()
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReflectionRunBody {
    #[serde(default, alias = "session_id")]
    pub session_id: Option<String>,
}

pub async fn handle_reflection_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ReflectionRunBody>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let engine = match engine_initialized() {
        Ok(e) => e,
        Err(resp) => return resp.into_response(),
    };
    let session_id = body
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if !engine.persist_training_data() {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "persistence_required",
                "detail": "请先在『自我进化』总卡片开启『持久化训练数据』",
            })),
        )
            .into_response();
    }
    match engine.trigger_manual_reflection(session_id) {
        Ok(run_id) => Json(serde_json::json!({"ok": true, "runId": run_id})).into_response(),
        Err(error) => {
            let msg = error.to_string();
            let (status, detail) = match msg.as_str() {
                "reflection_disabled" => (
                    StatusCode::CONFLICT,
                    "请先开启『自我反思』并配置反思模型",
                ),
                "persistence_required" => (
                    StatusCode::CONFLICT,
                    "请先在『自我进化』总卡片开启『持久化训练数据』",
                ),
                "no_turns_available" => (
                    StatusCode::BAD_REQUEST,
                    "没有可用的回合记录，请先与模型对话再触发反思",
                ),
                "reflection_queue_full" => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "反思队列已满，请稍后再试",
                ),
                "reflection_worker_unavailable" => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "反思工作线程未就绪",
                ),
                _ => (StatusCode::INTERNAL_SERVER_ERROR, ""),
            };
            if detail.is_empty() {
                json_error(status, &msg).into_response()
            } else {
                (
                    status,
                    Json(serde_json::json!({
                        "error": msg,
                        "detail": detail,
                    })),
                )
                    .into_response()
            }
        }
    }
}

fn reflection_config_to_json(
    cfg: &crate::evolution::SelfReflectionConfig,
) -> serde_json::Value {
    serde_json::json!({
        "enabled": cfg.enabled,
        "triggerMode": cfg.trigger_mode.as_str(),
        "depth": cfg.depth.as_str(),
        "reflectionModel": cfg.reflection_model,
        "reflectionProvider": cfg.reflection_provider,
        "scheduleIntervalMinutes": cfg.schedule_interval_minutes,
        "minTurnsForAuto": cfg.min_turns_for_auto,
        "failureThreshold": cfg.failure_threshold,
        "writebackTargets": cfg
            .writeback_targets
            .iter()
            .map(|t| t.as_str().to_string())
            .collect::<Vec<_>>(),
        "maxLessonsPerRun": cfg.max_lessons_per_run,
        "maxTotalLessons": cfg.max_total_lessons,
        "includeUserThumbsDown": cfg.include_user_thumbs_down,
        "lookbackTurns": cfg.lookback_turns,
    })
}

