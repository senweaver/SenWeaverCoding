// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket},
        Query, State, WebSocketUpgrade,
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;

use super::api::require_auth;
use super::AppState;
use crate::lan::group::op::GroupRole;
use crate::lan::group::service::TaskInput;
use crate::lan::group::GroupService;
use crate::lan::share::ShareService;
use crate::lan::LanService;

fn lan_service() -> Option<Arc<LanService>> {
    crate::services::try_get_services().and_then(|svc| svc.lan.clone())
}

fn group_service() -> Option<Arc<GroupService>> {
    lan_service().map(|lan| lan.group())
}

fn share_service() -> Option<Arc<ShareService>> {
    lan_service().map(|lan| lan.share())
}

fn service_unavailable() -> axum::response::Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({ "error": "lan service unavailable" })),
    )
        .into_response()
}

fn desktop_user_settings_path(state: &AppState) -> PathBuf {
    let config_path = state.live_config.load().config_path.clone();
    config_path
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("desktop_user.json")
}

fn persist_user_setting(state: &AppState, key: &str, value: serde_json::Value) {
    let path = desktop_user_settings_path(state);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut existing: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if !existing.is_object() {
        existing = serde_json::json!({});
    }
    if let Some(obj) = existing.as_object_mut() {
        obj.insert(key.to_string(), value);
    }
    let serialized = serde_json::to_string_pretty(&existing).unwrap_or_else(|_| existing.to_string());
    let _ = crate::util::atomic_write(&path, serialized.as_bytes());
}

pub async fn handle_lan_identity_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(lan) = lan_service() else {
        return service_unavailable();
    };
    Json(lan.identity_snapshot()).into_response()
}

#[derive(Debug, Deserialize)]
pub struct ProfileUpdate {
    pub nickname: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    pub email: Option<Option<String>>,
}

fn deserialize_optional_field<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(Some(value))
}

pub async fn handle_lan_profile_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ProfileUpdate>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(lan) = lan_service() else {
        return service_unavailable();
    };
    match lan.set_profile(body.nickname, body.email) {
        Ok(()) => Json(lan.identity_snapshot()).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct DiscoveryToggle {
    pub enabled: bool,
}

pub async fn handle_lan_discovery_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<DiscoveryToggle>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(lan) = lan_service() else {
        return service_unavailable();
    };
    persist_user_setting(&state, "lanDiscoveryEnabled", serde_json::json!(body.enabled));
    if body.enabled {
        if let Err(err) = lan.start().await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": err.to_string() })),
            )
                .into_response();
        }
    } else {
        lan.stop();
    }
    Json(serde_json::json!({ "ok": true, "running": lan.is_running() })).into_response()
}

pub async fn handle_lan_peers_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(lan) = lan_service() else {
        return service_unavailable();
    };
    Json(serde_json::json!({ "peers": lan.peers() })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct MessagesQuery {
    #[serde(rename = "peerId")]
    pub peer_id: String,
    pub limit: Option<i64>,
}

pub async fn handle_lan_messages_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<MessagesQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(lan) = lan_service() else {
        return service_unavailable();
    };
    let limit = query.limit.unwrap_or(200).clamp(1, 1000);
    match lan.history(&query.peer_id, limit) {
        Ok(messages) => Json(serde_json::json!({ "messages": messages })).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct SendMessage {
    #[serde(rename = "peerId")]
    pub peer_id: String,
    pub body: String,
}

pub async fn handle_lan_messages_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SendMessage>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(lan) = lan_service() else {
        return service_unavailable();
    };
    if body.body.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "body must not be empty" })),
        )
            .into_response();
    }
    match lan.send_text(&body.peer_id, &body.body).await {
        Ok(id) => Json(serde_json::json!({ "ok": true, "id": id })).into_response(),
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

pub async fn handle_lan_conversations_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(lan) = lan_service() else {
        return service_unavailable();
    };
    match lan.conversations() {
        Ok(conversations) => Json(serde_json::json!({
            "conversations": conversations,
            "unread": lan.unread_total(),
        }))
        .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct ReadRequest {
    #[serde(rename = "peerId")]
    pub peer_id: String,
}

pub async fn handle_lan_read_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ReadRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(lan) = lan_service() else {
        return service_unavailable();
    };
    match lan.mark_read(&body.peer_id) {
        Ok(()) => Json(serde_json::json!({ "ok": true, "unread": lan.unread_total() }))
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct SendFileRequest {
    #[serde(rename = "peerId")]
    pub peer_id: String,
    pub path: String,
}

pub async fn handle_lan_files_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SendFileRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(lan) = lan_service() else {
        return service_unavailable();
    };
    if body.path.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "path must not be empty" })),
        )
            .into_response();
    }
    let transfer_id = lan.send_path(&body.peer_id, &body.path);
    Json(serde_json::json!({ "ok": true, "transferId": transfer_id })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct SaveFileRequest {
    pub path: String,
    pub dest: String,
}

pub async fn handle_lan_files_save_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SaveFileRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(lan) = lan_service() else {
        return service_unavailable();
    };
    if body.path.trim().is_empty() || body.dest.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "path and dest must not be empty" })),
        )
            .into_response();
    }
    match lan.save_received(&body.path, &body.dest).await {
        Ok(saved) => Json(serde_json::json!({ "ok": true, "path": saved })).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct SendImageRequest {
    #[serde(rename = "peerId")]
    pub peer_id: String,
    #[serde(rename = "fileName", default)]
    pub file_name: String,
    #[serde(rename = "dataBase64")]
    pub data_base64: String,
}

pub async fn handle_lan_image_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SendImageRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(lan) = lan_service() else {
        return service_unavailable();
    };
    if body.peer_id.trim().is_empty() {
        return bad_request("peerId must not be empty");
    }
    let bytes = match decode_base64_payload(&body.data_base64) {
        Ok(b) => b,
        Err(err) => return bad_request(&err),
    };
    let name = if body.file_name.trim().is_empty() {
        default_image_name()
    } else {
        body.file_name.trim().to_string()
    };
    match lan.send_image(&body.peer_id, &name, bytes).await {
        Ok(transfer_id) => {
            Json(serde_json::json!({ "ok": true, "transferId": transfer_id })).into_response()
        }
        Err(err) => internal_error(err),
    }
}

#[derive(Debug, Deserialize)]
pub struct RawFileQuery {
    pub path: String,
}

pub async fn handle_lan_file_raw_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<RawFileQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(lan) = lan_service() else {
        return service_unavailable();
    };
    let path = params.path.clone();
    let result = tokio::task::spawn_blocking(move || lan.read_shared_file(&path)).await;
    match result {
        Ok(Ok((bytes, mime))) => raw_bytes_response(bytes, &mime),
        Ok(Err(err)) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("file read task failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn handle_lan_transfers_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(lan) = lan_service() else {
        return service_unavailable();
    };
    match lan.transfers() {
        Ok(transfers) => Json(serde_json::json!({ "transfers": transfers })).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct LanWsQuery {
    pub token: Option<String>,
}

pub async fn handle_ws_lan(
    State(state): State<AppState>,
    Query(params): Query<LanWsQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if state.pairing.require_pairing() {
        let token = extract_ws_token(&headers, params.token.as_deref()).unwrap_or("");
        if !state.pairing.is_authenticated(token) {
            return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
        }
    }
    let event_tx = state.event_tx.clone();
    ws.on_upgrade(move |socket| forward_lan_events(socket, event_tx))
        .into_response()
}

async fn forward_lan_events(
    socket: WebSocket,
    event_tx: tokio::sync::broadcast::Sender<serde_json::Value>,
) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = event_tx.subscribe();
    loop {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Ok(payload) => {
                        if payload.get("type").and_then(|v| v.as_str()) == Some("lan_event") {
                            if let Ok(text) = serde_json::to_string(&payload) {
                                if sender.send(Message::Text(text.into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            incoming = receiver.next() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
        }
    }
}

fn extract_ws_token<'a>(headers: &'a HeaderMap, query_token: Option<&'a str>) -> Option<&'a str> {
    if let Some(token) = query_token {
        if !token.is_empty() {
            return Some(token);
        }
    }
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim_start_matches("Bearer ").trim())
}

fn bad_request(message: &str) -> axum::response::Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": message })),
    )
        .into_response()
}

fn internal_error(err: impl std::fmt::Display) -> axum::response::Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": err.to_string() })),
    )
        .into_response()
}

fn decode_base64_payload(input: &str) -> Result<Vec<u8>, String> {
    use base64::Engine as _;
    let trimmed = input.trim();
    let encoded = match trimmed.find(";base64,") {
        Some(idx) => &trimmed[idx + ";base64,".len()..],
        None => trimmed,
    };
    if encoded.is_empty() {
        return Err("image payload is empty".to_string());
    }
    base64::engine::general_purpose::STANDARD
        .decode(encoded.as_bytes())
        .map_err(|e| format!("invalid base64 payload: {e}"))
}

fn default_image_name() -> String {
    format!("pasted-{}.png", chrono::Utc::now().timestamp_millis())
}

fn raw_bytes_response(bytes: Vec<u8>, mime: &str) -> axum::response::Response {
    let mut response = axum::response::Response::new(axum::body::Body::from(bytes));
    let value = axum::http::HeaderValue::from_str(mime)
        .unwrap_or_else(|_| axum::http::HeaderValue::from_static("application/octet-stream"));
    response
        .headers_mut()
        .insert(axum::http::header::CONTENT_TYPE, value);
    response
        .headers_mut()
        .insert(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("private, max-age=86400"),
        );
    response
}

fn parse_role(value: &str) -> Option<GroupRole> {
    GroupRole::parse(value)
}

pub async fn handle_lan_groups_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(group) = group_service() else {
        return service_unavailable();
    };
    Json(serde_json::json!({
        "groups": group.list_groups(),
        "unread": group.unread_total(),
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
pub struct CreateGroupRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

pub async fn handle_lan_groups_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateGroupRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(group) = group_service() else {
        return service_unavailable();
    };
    match group.create_group(&body.name, &body.description) {
        Ok(summary) => Json(serde_json::json!({ "ok": true, "group": summary })).into_response(),
        Err(err) => bad_request(&err.to_string()),
    }
}

#[derive(Debug, Deserialize)]
pub struct GroupIdQuery {
    #[serde(rename = "groupId")]
    pub group_id: String,
}

pub async fn handle_lan_group_snapshot_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<GroupIdQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(group) = group_service() else {
        return service_unavailable();
    };
    match group.snapshot(&query.group_id) {
        Some(snapshot) => Json(snapshot).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "group not found" })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct GroupMessagesQuery {
    #[serde(rename = "groupId")]
    pub group_id: String,
    pub limit: Option<i64>,
}

pub async fn handle_lan_group_messages_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<GroupMessagesQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(group) = group_service() else {
        return service_unavailable();
    };
    let limit = query.limit.unwrap_or(300).clamp(1, 2000);
    Json(serde_json::json!({ "messages": group.messages(&query.group_id, limit) }))
        .into_response()
}

#[derive(Debug, Deserialize)]
pub struct GroupMessagePost {
    #[serde(rename = "groupId")]
    pub group_id: String,
    pub body: String,
}

pub async fn handle_lan_group_messages_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<GroupMessagePost>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(group) = group_service() else {
        return service_unavailable();
    };
    match group.post_message(&body.group_id, &body.body) {
        Ok(id) => Json(serde_json::json!({ "ok": true, "id": id })).into_response(),
        Err(err) => bad_request(&err.to_string()),
    }
}

pub async fn handle_lan_group_read_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<GroupIdQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(group) = group_service() else {
        return service_unavailable();
    };
    group.mark_read(&body.group_id);
    Json(serde_json::json!({ "ok": true, "unread": group.unread_total() })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct GroupMetaPost {
    #[serde(rename = "groupId")]
    pub group_id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
}

pub async fn handle_lan_group_meta_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<GroupMetaPost>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(group) = group_service() else {
        return service_unavailable();
    };
    match group.update_meta(&body.group_id, &body.name, &body.description) {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(err) => bad_request(&err.to_string()),
    }
}

#[derive(Debug, Deserialize)]
pub struct GroupMemberPost {
    #[serde(rename = "groupId")]
    pub group_id: String,
    #[serde(rename = "userId")]
    pub user_id: String,
    #[serde(default)]
    pub role: Option<String>,
}

pub async fn handle_lan_group_invite_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<GroupMemberPost>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(group) = group_service() else {
        return service_unavailable();
    };
    let role = body
        .role
        .as_deref()
        .and_then(parse_role)
        .unwrap_or(GroupRole::Member);
    match group.invite_member(&body.group_id, &body.user_id, role) {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(err) => bad_request(&err.to_string()),
    }
}

pub async fn handle_lan_group_role_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<GroupMemberPost>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(group) = group_service() else {
        return service_unavailable();
    };
    let Some(role) = body.role.as_deref().and_then(parse_role) else {
        return bad_request("invalid role");
    };
    match group.set_role(&body.group_id, &body.user_id, role) {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(err) => bad_request(&err.to_string()),
    }
}

pub async fn handle_lan_group_member_remove_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<GroupMemberPost>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(group) = group_service() else {
        return service_unavailable();
    };
    match group.remove_member(&body.group_id, &body.user_id) {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(err) => bad_request(&err.to_string()),
    }
}

pub async fn handle_lan_group_leave_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<GroupIdQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(group) = group_service() else {
        return service_unavailable();
    };
    match group.leave_group(&body.group_id) {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(err) => bad_request(&err.to_string()),
    }
}

#[derive(Debug, Deserialize)]
pub struct PhasePost {
    #[serde(rename = "groupId")]
    pub group_id: String,
    #[serde(rename = "phaseId", default)]
    pub phase_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub order: i64,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub color: String,
}

pub async fn handle_lan_group_phase_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PhasePost>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(group) = group_service() else {
        return service_unavailable();
    };
    match group.upsert_phase(
        &body.group_id,
        body.phase_id,
        &body.name,
        body.order,
        &body.status,
        &body.color,
    ) {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(err) => bad_request(&err.to_string()),
    }
}

#[derive(Debug, Deserialize)]
pub struct PhaseRemovePost {
    #[serde(rename = "groupId")]
    pub group_id: String,
    #[serde(rename = "phaseId")]
    pub phase_id: String,
}

pub async fn handle_lan_group_phase_remove_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PhaseRemovePost>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(group) = group_service() else {
        return service_unavailable();
    };
    match group.remove_phase(&body.group_id, &body.phase_id) {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(err) => bad_request(&err.to_string()),
    }
}

#[derive(Debug, Deserialize)]
pub struct DocumentUploadPost {
    #[serde(rename = "groupId")]
    pub group_id: String,
    pub path: String,
    #[serde(rename = "phaseId", default)]
    pub phase_id: String,
    #[serde(default)]
    pub note: String,
}

pub async fn handle_lan_group_document_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<DocumentUploadPost>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(group) = group_service() else {
        return service_unavailable();
    };
    if body.path.trim().is_empty() {
        return bad_request("path must not be empty");
    }
    match group
        .upload_document(&body.group_id, &body.path, &body.phase_id, &body.note)
        .await
    {
        Ok(doc_id) => Json(serde_json::json!({ "ok": true, "docId": doc_id })).into_response(),
        Err(err) => internal_error(err),
    }
}

#[derive(Debug, Deserialize)]
pub struct DocumentRefPost {
    #[serde(rename = "groupId")]
    pub group_id: String,
    #[serde(rename = "docId")]
    pub doc_id: String,
}

pub async fn handle_lan_group_document_download_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<DocumentRefPost>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(group) = group_service() else {
        return service_unavailable();
    };
    match group.request_download(&body.group_id, &body.doc_id) {
        Ok(available) => {
            Json(serde_json::json!({ "ok": true, "available": available })).into_response()
        }
        Err(err) => bad_request(&err.to_string()),
    }
}

#[derive(Debug, Deserialize)]
pub struct DocumentSavePost {
    #[serde(rename = "groupId")]
    pub group_id: String,
    #[serde(rename = "docId")]
    pub doc_id: String,
    pub dest: String,
}

pub async fn handle_lan_group_document_save_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<DocumentSavePost>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(group) = group_service() else {
        return service_unavailable();
    };
    if body.dest.trim().is_empty() {
        return bad_request("dest must not be empty");
    }
    match group.save_document(&body.group_id, &body.doc_id, &body.dest).await {
        Ok(path) => Json(serde_json::json!({ "ok": true, "path": path })).into_response(),
        Err(err) => bad_request(&err.to_string()),
    }
}

#[derive(Debug, Deserialize)]
pub struct GroupImageUploadPost {
    #[serde(rename = "groupId")]
    pub group_id: String,
    #[serde(rename = "phaseId", default)]
    pub phase_id: String,
    #[serde(default)]
    pub note: String,
    #[serde(rename = "fileName", default)]
    pub file_name: String,
    #[serde(rename = "dataBase64")]
    pub data_base64: String,
}

pub async fn handle_lan_group_image_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<GroupImageUploadPost>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(lan) = lan_service() else {
        return service_unavailable();
    };
    if body.group_id.trim().is_empty() {
        return bad_request("groupId must not be empty");
    }
    let bytes = match decode_base64_payload(&body.data_base64) {
        Ok(b) => b,
        Err(err) => return bad_request(&err),
    };
    if bytes.is_empty() {
        return bad_request("image payload is empty");
    }
    let name = if body.file_name.trim().is_empty() {
        default_image_name()
    } else {
        body.file_name.trim().to_string()
    };
    let dir = lan.outbox_dir();
    if let Err(err) = tokio::fs::create_dir_all(&dir).await {
        return internal_error(err);
    }
    let temp_path = dir.join(format!("{}-{}", uuid::Uuid::new_v4(), name));
    if let Err(err) = tokio::fs::write(&temp_path, &bytes).await {
        return internal_error(err);
    }
    let group = lan.group();
    let result = group
        .upload_document(
            &body.group_id,
            &temp_path.to_string_lossy(),
            &body.phase_id,
            &body.note,
        )
        .await;
    let _ = tokio::fs::remove_file(&temp_path).await;
    match result {
        Ok(doc_id) => Json(serde_json::json!({ "ok": true, "docId": doc_id })).into_response(),
        Err(err) => internal_error(err),
    }
}

#[derive(Debug, Deserialize)]
pub struct GroupDocRawQuery {
    #[serde(rename = "groupId")]
    pub group_id: String,
    #[serde(rename = "docId")]
    pub doc_id: String,
}

pub async fn handle_lan_group_document_raw_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<GroupDocRawQuery>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(group) = group_service() else {
        return service_unavailable();
    };
    let group_for_read = group.clone();
    let gid = params.group_id.clone();
    let did = params.doc_id.clone();
    let result =
        tokio::task::spawn_blocking(move || group_for_read.document_content(&gid, &did)).await;
    match result {
        Ok(Ok(Some((bytes, mime, _name)))) => raw_bytes_response(bytes, &mime),
        Ok(Ok(None)) => {
            let _ = group.request_download(&params.group_id, &params.doc_id);
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "document not available yet" })),
            )
                .into_response()
        }
        Ok(Err(err)) => bad_request(&err.to_string()),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("document read task failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn handle_lan_group_document_remove_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<DocumentRefPost>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(group) = group_service() else {
        return service_unavailable();
    };
    match group.delete_document(&body.group_id, &body.doc_id) {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(err) => bad_request(&err.to_string()),
    }
}

#[derive(Debug, Deserialize)]
pub struct TaskPost {
    #[serde(rename = "groupId")]
    pub group_id: String,
    #[serde(rename = "taskId", default)]
    pub task_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "phaseId", default)]
    pub phase_id: String,
    #[serde(default)]
    pub assignee: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub priority: String,
    #[serde(rename = "dueMs", default)]
    pub due_ms: i64,
    #[serde(default)]
    pub deps: Vec<String>,
    #[serde(default)]
    pub parent: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub progress: i64,
}

pub async fn handle_lan_group_task_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<TaskPost>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(group) = group_service() else {
        return service_unavailable();
    };
    let input = TaskInput {
        title: body.title,
        description: body.description,
        phase_id: body.phase_id,
        assignee: body.assignee,
        status: body.status,
        priority: body.priority,
        due_ms: body.due_ms,
        deps: body.deps,
        parent: body.parent,
        kind: body.kind,
        progress: body.progress,
    };
    match group.upsert_task(&body.group_id, body.task_id, input) {
        Ok(task_id) => Json(serde_json::json!({ "ok": true, "taskId": task_id })).into_response(),
        Err(err) => bad_request(&err.to_string()),
    }
}

#[derive(Debug, Deserialize)]
pub struct TaskRemovePost {
    #[serde(rename = "groupId")]
    pub group_id: String,
    #[serde(rename = "taskId")]
    pub task_id: String,
}

pub async fn handle_lan_group_task_remove_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<TaskRemovePost>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(group) = group_service() else {
        return service_unavailable();
    };
    match group.remove_task(&body.group_id, &body.task_id) {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(err) => bad_request(&err.to_string()),
    }
}

pub async fn handle_lan_shares_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(share) = share_service() else {
        return service_unavailable();
    };
    Json(serde_json::json!({ "shares": share.my_shares() })).into_response()
}

pub async fn handle_lan_share_peers_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(share) = share_service() else {
        return service_unavailable();
    };
    Json(serde_json::json!({ "shares": share.peer_shares() })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct ShareAddPost {
    pub path: String,
    #[serde(default)]
    pub note: String,
}

pub async fn handle_lan_shares_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ShareAddPost>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(share) = share_service() else {
        return service_unavailable();
    };
    if body.path.trim().is_empty() {
        return bad_request("path must not be empty");
    }
    match share.add_share(&body.path, &body.note).await {
        Ok(id) => Json(serde_json::json!({ "ok": true, "id": id })).into_response(),
        Err(err) => bad_request(&err.to_string()),
    }
}

#[derive(Debug, Deserialize)]
pub struct ShareRemovePost {
    #[serde(rename = "shareId")]
    pub share_id: String,
}

pub async fn handle_lan_shares_remove_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ShareRemovePost>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(share) = share_service() else {
        return service_unavailable();
    };
    match share.remove_share(&body.share_id) {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(err) => bad_request(&err.to_string()),
    }
}

#[derive(Debug, Deserialize)]
pub struct ShareDownloadPost {
    #[serde(rename = "ownerId")]
    pub owner_id: String,
    #[serde(rename = "shareId")]
    pub share_id: String,
}

pub async fn handle_lan_share_download_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ShareDownloadPost>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }
    let Some(share) = share_service() else {
        return service_unavailable();
    };
    match share.request_download(&body.owner_id, &body.share_id) {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(err) => bad_request(&err.to_string()),
    }
}
