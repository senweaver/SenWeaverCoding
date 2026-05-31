// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::AppState;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Json},
};
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: Option<String>,
    pub device_type: Option<String>,
    pub paired_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub ip_address: Option<String>,
}

#[derive(Debug)]
pub struct DeviceRegistry {
    cache: Mutex<HashMap<String, DeviceInfo>>,
    db_path: PathBuf,
}

impl DeviceRegistry {
    pub fn new(workspace_dir: &Path) -> Result<Self, String> {
        let db_path = workspace_dir.join("devices.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open device registry database: {e}"))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS devices (
                token_hash TEXT PRIMARY KEY,
                id TEXT NOT NULL,
                name TEXT,
                device_type TEXT,
                paired_at TEXT NOT NULL,
                last_seen TEXT NOT NULL,
                ip_address TEXT
            )",
        )
        .map_err(|e| format!("Failed to create devices table: {e}"))?;

        let mut cache = HashMap::new();
        let mut stmt = conn
            .prepare("SELECT token_hash, id, name, device_type, paired_at, last_seen, ip_address FROM devices")
            .map_err(|e| format!("Failed to prepare device select: {e}"))?;
        let rows = stmt
            .query_map([], |row| {
                let token_hash: String = row.get(0)?;
                let id: String = row.get(1)?;
                let name: Option<String> = row.get(2)?;
                let device_type: Option<String> = row.get(3)?;
                let paired_at_str: String = row.get(4)?;
                let last_seen_str: String = row.get(5)?;
                let ip_address: Option<String> = row.get(6)?;
                let paired_at = DateTime::parse_from_rfc3339(&paired_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                let last_seen = DateTime::parse_from_rfc3339(&last_seen_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                Ok((
                    token_hash,
                    DeviceInfo {
                        id,
                        name,
                        device_type,
                        paired_at,
                        last_seen,
                        ip_address,
                    },
                ))
            })
            .map_err(|e| format!("Failed to query devices: {e}"))?;
        for (hash, info) in rows.flatten() {
            cache.insert(hash, info);
        }

        Ok(Self {
            cache: Mutex::new(cache),
            db_path,
        })
    }

    fn open_db(&self) -> Result<Connection, String> {
        Connection::open(&self.db_path)
            .map_err(|e| format!("Failed to open device registry database: {e}"))
    }

    pub fn register(&self, token_hash: String, info: DeviceInfo) {
        let conn = match self.open_db() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("device register: {e}");
                return;
            }
        };
        if let Err(e) = conn.execute(
            "INSERT OR REPLACE INTO devices (token_hash, id, name, device_type, paired_at, last_seen, ip_address) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                token_hash,
                info.id,
                info.name,
                info.device_type,
                info.paired_at.to_rfc3339(),
                info.last_seen.to_rfc3339(),
                info.ip_address,
            ],
        ) {
            tracing::error!("Failed to insert device: {e}");
            return;
        }
        self.cache.lock().insert(token_hash, info);
    }

    pub fn list(&self) -> Vec<DeviceInfo> {
        let conn = match self.open_db() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("device list: {e}");
                return vec![];
            }
        };
        let mut stmt = match conn
            .prepare("SELECT token_hash, id, name, device_type, paired_at, last_seen, ip_address FROM devices")
        {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to prepare device select: {e}");
                return vec![];
            }
        };
        let rows = match stmt.query_map([], |row| {
            let id: String = row.get(1)?;
            let name: Option<String> = row.get(2)?;
            let device_type: Option<String> = row.get(3)?;
            let paired_at_str: String = row.get(4)?;
            let last_seen_str: String = row.get(5)?;
            let ip_address: Option<String> = row.get(6)?;
            let paired_at = DateTime::parse_from_rfc3339(&paired_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let last_seen = DateTime::parse_from_rfc3339(&last_seen_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(DeviceInfo {
                id,
                name,
                device_type,
                paired_at,
                last_seen,
                ip_address,
            })
        }) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("Failed to query devices: {e}");
                return vec![];
            }
        };
        rows.filter_map(|r| r.ok()).collect()
    }

    pub fn revoke(&self, device_id: &str) -> bool {
        let conn = match self.open_db() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("device revoke: {e}");
                return false;
            }
        };
        let deleted = conn
            .execute(
                "DELETE FROM devices WHERE id = ?1",
                rusqlite::params![device_id],
            )
            .unwrap_or(0);
        if deleted > 0 {
            let mut cache = self.cache.lock();
            let key = cache
                .iter()
                .find(|(_, v)| v.id == device_id)
                .map(|(k, _)| k.clone());
            if let Some(key) = key {
                cache.remove(&key);
            }
            true
        } else {
            false
        }
    }

    pub fn update_last_seen(&self, token_hash: &str) {
        let now = Utc::now();
        let conn = match self.open_db() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("device update_last_seen: {e}");
                return;
            }
        };
        conn.execute(
            "UPDATE devices SET last_seen = ?1 WHERE token_hash = ?2",
            rusqlite::params![now.to_rfc3339(), token_hash],
        )
        .ok();
        if let Some(device) = self.cache.lock().get_mut(token_hash) {
            device.last_seen = now;
        }
    }

    pub fn device_count(&self) -> usize {
        self.cache.lock().len()
    }
}

fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|auth| auth.strip_prefix("Bearer "))
}

fn require_auth(state: &AppState, headers: &HeaderMap) -> Result<(), (StatusCode, &'static str)> {
    if state.pairing.require_pairing() {
        let token = extract_bearer(headers).unwrap_or("");
        if !state.pairing.is_authenticated(token) {
            return Err((StatusCode::UNAUTHORIZED, "Unauthorized"));
        }
    }
    Ok(())
}

pub async fn initiate_pairing(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    match state.pairing.generate_new_pairing_code() {
        Some(code) => Json(serde_json::json!({
            "pairing_code": code,
            "message": "New pairing code generated"
        }))
        .into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Pairing is disabled or not available",
        )
            .into_response(),
    }
}

pub async fn submit_pairing_enhanced(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let code = body["code"].as_str().unwrap_or("");
    let device_name = body["device_name"].as_str().map(String::from);
    let device_type = body["device_type"].as_str().map(String::from);

    let client_id = headers
        .get("x-real-ip")
        .or_else(|| headers.get("x-forwarded-for"))
        .and_then(|v| v.to_str().ok())
        .map(|s| {
            let ip = s.split(',').next().unwrap_or(s).trim().to_string();
            tracing::debug!(
                header_ip = %ip,
                "rate-limit client_id derived from proxy header (spoofable)"
            );
            ip
        })
        .unwrap_or_else(|| "unknown".to_string());

    match state.pairing.try_pair(code, &client_id).await {
        Ok(Some(token)) => {

            let token_hash = {
                use sha2::{Digest, Sha256};
                let hash = Sha256::digest(token.as_bytes());
                hex::encode(hash)
            };
            if let Some(ref registry) = state.device_registry {
                let registry = std::sync::Arc::clone(registry);
                let info = DeviceInfo {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: device_name,
                    device_type,
                    paired_at: Utc::now(),
                    last_seen: Utc::now(),
                    ip_address: Some(client_id),
                };
                let _ =
                    tokio::task::spawn_blocking(move || registry.register(token_hash, info)).await;
            }
            Json(serde_json::json!({
                "token": token,
                "message": "Pairing successful"
            }))
            .into_response()
        }
        Ok(None) => (StatusCode::BAD_REQUEST, "Invalid or expired pairing code").into_response(),
        Err(lockout_secs) => (
            StatusCode::TOO_MANY_REQUESTS,
            format!("Too many attempts. Locked out for {lockout_secs}s"),
        )
            .into_response(),
    }
}

pub async fn list_devices(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let devices = if let Some(registry) = state.device_registry.as_ref() {
        let registry = std::sync::Arc::clone(registry);
        tokio::task::spawn_blocking(move || registry.list())
            .await
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let count = devices.len();
    Json(serde_json::json!({
        "devices": devices,
        "count": count
    }))
    .into_response()
}

pub async fn revoke_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(device_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let revoked = if let Some(registry) = state.device_registry.as_ref() {
        let registry = std::sync::Arc::clone(registry);
        let device_id = device_id.clone();
        tokio::task::spawn_blocking(move || registry.revoke(&device_id))
            .await
            .unwrap_or(false)
    } else {
        false
    };

    if revoked {
        Json(serde_json::json!({
            "message": "Device revoked",
            "device_id": device_id
        }))
        .into_response()
    } else {
        (StatusCode::NOT_FOUND, "Device not found").into_response()
    }
}

pub async fn rotate_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(device_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    match state.pairing.generate_new_pairing_code() {
        Some(code) => Json(serde_json::json!({
            "device_id": device_id,
            "pairing_code": code,
            "message": "Use this code to re-pair the device"
        }))
        .into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Cannot generate new pairing code",
        )
            .into_response(),
    }
}
