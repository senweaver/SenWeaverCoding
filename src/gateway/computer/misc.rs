// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::AppState;
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{IntoResponse, Json},
};
use std::path::PathBuf;

use crate::config::Config;

fn sanitize(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains("..")
    {
        return None;
    }
    Some(trimmed.to_string())
}

fn recording_dir(state: &AppState, name: &str) -> Option<PathBuf> {
    let safe = sanitize(name)?;
    let dir = state
        .live_config
        .load_ref()
        .workspace_dir
        .join("skills")
        .join(&safe);
    dir.join("recording.json").is_file().then_some(dir)
}

fn started_epoch(dir: &std::path::Path) -> i64 {
    crate::computer::activity::events::read_events(dir)
        .first()
        .map(|e| e.epoch)
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis())
}

pub async fn handle_upload_audio(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Query(params): Query<std::collections::HashMap<String, String>>,
    body: Bytes,
) -> impl IntoResponse {
    if let Err(e) = super::super::api::require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(safe) = sanitize(&name) else {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "invalid recording name" })),
        )
            .into_response();
    };
    let dir = state
        .live_config
        .load_ref()
        .workspace_dir
        .join("skills")
        .join(&safe);
    if !dir.exists() {
        let _ = tokio::fs::create_dir_all(&dir).await;
    }
    let language = params
        .get("language")
        .map(|s| s.as_str())
        .unwrap_or("en")
        .to_string();
    let start_epoch = params
        .get("startEpoch")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
    let stop_epoch = params
        .get("stopEpoch")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(start_epoch);
    let data = body.to_vec();
    let dir_clone = dir.clone();
    let result = tokio::task::spawn_blocking(move || {
        crate::computer::narration::append_segment(
            &dir_clone,
            &language,
            start_epoch,
            stop_epoch,
            &data,
        )
    })
    .await;
    match result {
        Ok(Ok(segment)) => Json(serde_json::json!({ "ok": true, "segment": segment })).into_response(),
        Ok(Err(e)) => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("audio write task failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn handle_transcribe(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = super::super::api::require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(dir) = recording_dir(&state, &name) else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("recording '{name}' not found") })),
        )
            .into_response();
    };
    let config: Config = state.live_config.load_ref().as_ref().clone();
    let epoch = started_epoch(&dir);
    match crate::computer::narration::transcribe::transcribe_recording(&config, &dir, epoch).await {
        Ok(transcript) => Json(serde_json::json!({
            "ok": true,
            "segmentCount": transcript.segments.len(),
        }))
        .into_response(),
        Err(e) => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn handle_get_privacy(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = super::super::api::require_auth(&state, &headers) {
        return e.into_response();
    }
    let workspace = state.live_config.load_ref().workspace_dir.clone();
    let settings = crate::computer::sensitive::load_privacy_settings(&workspace);
    Json(serde_json::json!({ "advancedProtection": settings.advanced_protection })).into_response()
}

pub async fn handle_put_privacy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = super::super::api::require_auth(&state, &headers) {
        return e.into_response();
    }
    let workspace = state.live_config.load_ref().workspace_dir.clone();
    let advanced = body
        .get("advancedProtection")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let settings = crate::computer::sensitive::PrivacySettings {
        advanced_protection: advanced,
    };
    match crate::computer::sensitive::save_privacy_settings(&workspace, &settings) {
        Ok(()) => Json(serde_json::json!({ "ok": true, "advancedProtection": advanced }))
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn handle_build_targets(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = super::super::api::require_auth(&state, &headers) {
        return e.into_response();
    }
    Json(serde_json::json!({ "targets": crate::computer::build::build_targets() })).into_response()
}

pub async fn handle_doctor(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Err(e) = super::super::api::require_auth(&state, &headers) {
        return e.into_response();
    }
    let config = state.live_config.load_ref();
    let vision = crate::computer::list_vision_models(config.as_ref());
    let transcription_configured =
        crate::computer::narration::transcribe::transcription_configured(config.as_ref());
    let ocr_available = crate::computer::sensitive::ocr::ocr_available();
    Json(serde_json::json!({
        "platform": std::env::consts::OS,
        "recordingSupported": cfg!(windows),
        "visionModelCount": vision.len(),
        "visionRecommended": vision.iter().find(|m| m.recommended).map(|m| {
            serde_json::json!({ "provider": m.provider, "model": m.model })
        }),
        "transcriptionConfigured": transcription_configured,
        "ocrAvailable": ocr_available,
    }))
    .into_response()
}

pub async fn handle_export_debug(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = super::super::api::require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(dir) = recording_dir(&state, &name) else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("recording '{name}' not found") })),
        )
            .into_response();
    };
    let dest = body
        .get("destDir")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(dest) = dest else {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "destDir is required" })),
        )
            .into_response();
    };
    let safe = sanitize(&name).unwrap_or_else(|| "recording".to_string());
    let out_path = PathBuf::from(dest).join(format!("computer-recording-{safe}.zip"));
    let result = tokio::task::spawn_blocking(move || zip_dir(&dir, &out_path)).await;
    match result {
        Ok(Ok(path)) => Json(serde_json::json!({ "ok": true, "path": path })).into_response(),
        Ok(Err(e)) => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("debug export task failed: {e}") })),
        )
            .into_response(),
    }
}

fn zip_dir(dir: &std::path::Path, out_path: &std::path::Path) -> anyhow::Result<String> {
    use std::io::Write as _;
    let file = std::fs::File::create(out_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(rel) = path.strip_prefix(dir) else {
                continue;
            };
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(bytes) = std::fs::read(&path) {
                zip.start_file(rel_str, options)?;
                zip.write_all(&bytes)?;
            }
        }
    }
    zip.finish()?;
    Ok(out_path.to_string_lossy().to_string())
}
