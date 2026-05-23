// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::AppState;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;
use tokio::io::AsyncWriteExt as _;

const MAX_APPEND_BYTES: usize = 32_768;

fn require_auth(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if !state.pairing.require_pairing() {
        return Ok(());
    }
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|auth| auth.strip_prefix("Bearer "))
        .unwrap_or("");
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

fn hardware_dir() -> Result<PathBuf, String> {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().join(".senweavercoding").join("hardware"))
        .ok_or_else(|| "Cannot determine home directory".to_string())
}

fn validate_device_alias(alias: &str) -> Result<(), &'static str> {
    if alias.is_empty() || alias.len() > 64 {
        return Err("Device alias must be 1–64 characters");
    }
    if !alias
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Device alias must contain only alphanumerics, hyphens, and underscores");
    }
    Ok(())
}

fn device_file_path(hw_dir: &std::path::Path, alias: &str) -> Result<PathBuf, &'static str> {
    validate_device_alias(alias)?;
    Ok(hw_dir.join("devices").join(format!("{alias}.md")))
}

#[derive(Debug, Deserialize)]
pub struct PinRegistrationBody {

    #[serde(default = "default_device")]
    pub device: String,

    pub pin: u32,

    pub component: String,

    #[serde(default)]
    pub notes: String,
}

fn default_device() -> String {
    "rpi0".to_string()
}

pub async fn handle_hardware_pin(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<PinRegistrationBody>, axum::extract::rejection::JsonRejection>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let Json(req) = match body {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Invalid JSON: {e}") })),
            )
                .into_response();
        }
    };

    if req.component.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "\"component\" must not be empty" })),
        )
            .into_response();
    }

    let component = req.component.replace(['\n', '\r'], " ");
    let notes = req.notes.replace(['\n', '\r'], " ");

    let hw_dir = match hardware_dir() {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response();
        }
    };

    let device_path = match device_file_path(&hw_dir, &req.device) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response();
        }
    };

    if let Some(parent) = device_path.parent() {
        if let Err(e) = fs::create_dir_all(parent).await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to create directory: {e}") })),
            )
                .into_response();
        }
    }

    let line = if notes.is_empty() {
        format!("- GPIO {}: {}\n", req.pin, component)
    } else {
        format!("- GPIO {}: {} — {}\n", req.pin, component, notes)
    };

    match append_to_file(&device_path, &line).await {
        Ok(()) => {
            let message = format!(
                "GPIO {} registered as {} on {}",
                req.pin, component, req.device
            );
            tracing::info!(device = %req.device, pin = req.pin, component = %component, "{}", message);
            (
                StatusCode::OK,
                Json(serde_json::json!({ "ok": true, "message": message })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to write: {e}") })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct ContextAppendBody {

    #[serde(default = "default_device")]
    pub device: String,

    pub content: String,
}

pub async fn handle_hardware_context_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<ContextAppendBody>, axum::extract::rejection::JsonRejection>,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let Json(req) = match body {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Invalid JSON: {e}") })),
            )
                .into_response();
        }
    };

    if req.content.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "\"content\" must not be empty" })),
        )
            .into_response();
    }
    if req.content.len() > MAX_APPEND_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(serde_json::json!({
                "error": format!("Content too large — max {} bytes", MAX_APPEND_BYTES)
            })),
        )
            .into_response();
    }

    let hw_dir = match hardware_dir() {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response();
        }
    };

    let device_path = match device_file_path(&hw_dir, &req.device) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response();
        }
    };

    if let Some(parent) = device_path.parent() {
        if let Err(e) = fs::create_dir_all(parent).await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to create directory: {e}") })),
            )
                .into_response();
        }
    }

    let mut content = req.content.clone();
    if !content.ends_with('\n') {
        content.push('\n');
    }

    match append_to_file(&device_path, &content).await {
        Ok(()) => {
            tracing::info!(device = %req.device, bytes = content.len(), "Hardware context appended");
            (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to write: {e}") })),
        )
            .into_response(),
    }
}

const BOARD_DESCRIPTIONS: &[(&str, &str, &str)] = &[
    (
        "nucleo-f401re",
        "STM32F401RET6",
        "ARM Cortex-M4, 84 MHz · Flash 512 KB · RAM 128 KB · User LED on PA5",
    ),
    (
        "nucleo-f411re",
        "STM32F411RET6",
        "ARM Cortex-M4, 100 MHz · Flash 512 KB · RAM 128 KB · User LED on PA5",
    ),
    (
        "arduino-uno",
        "ATmega328P",
        "8-bit AVR, 16 MHz · Flash 16 KB · SRAM 2 KB · Built-in LED on pin 13",
    ),
    (
        "arduino-uno-q",
        "STM32U585 + Qualcomm",
        "Dual-core: STM32 (MCU) + Linux (aarch64) · GPIO via Bridge app on port 9999",
    ),
    (
        "esp32",
        "ESP32",
        "Dual-core Xtensa LX6, 240 MHz · Flash 4 MB · Built-in LED on GPIO 2",
    ),
    (
        "rpi-gpio",
        "Raspberry Pi",
        "ARM Linux · Native GPIO via sysfs/rppal",
    ),
];

#[derive(Debug, Serialize)]
pub struct BoardInfo {
    pub board: String,
    pub transport: String,
    pub path: Option<String>,
    pub baud: u32,
    pub chip: String,
    pub description: String,
}

#[derive(Debug, Serialize)]
pub struct HardwareBoardsResponse {
    pub enabled: bool,
    pub boards: Vec<BoardInfo>,
}

pub async fn handle_hardware_boards(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let config = state.config.lock().clone();
    let peripherals = &config.peripherals;

    let boards: Vec<BoardInfo> = peripherals
        .boards
        .iter()
        .map(|b| {
            let (chip, description) = BOARD_DESCRIPTIONS
                .iter()
                .find(|(name, _, _)| *name == b.board)
                .map(|(_, c, d)| (*c, *d))
                .unwrap_or((
                    "Unknown",
                    "No static description available for this board type.",
                ));
            BoardInfo {
                board: b.board.clone(),
                transport: b.transport.clone(),
                path: b.path.clone(),
                baud: b.baud,
                chip: chip.to_string(),
                description: description.to_string(),
            }
        })
        .collect();

    let resp = HardwareBoardsResponse {
        enabled: peripherals.enabled,
        boards,
    };

    (StatusCode::OK, Json(resp)).into_response()
}

#[derive(Debug, Serialize)]
struct HardwareContextResponse {
    hardware_md: String,
    devices: std::collections::HashMap<String, String>,
}

pub async fn handle_hardware_context_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let hw_dir = match hardware_dir() {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response();
        }
    };

    let hardware_md = fs::read_to_string(hw_dir.join("HARDWARE.md"))
        .await
        .unwrap_or_default();

    let devices_dir = hw_dir.join("devices");
    let mut devices = std::collections::HashMap::new();
    if let Ok(mut entries) = fs::read_dir(&devices_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                let alias = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                if !alias.is_empty() {
                    let content = fs::read_to_string(&path).await.unwrap_or_default();
                    devices.insert(alias, content);
                }
            }
        }
    }

    let resp = HardwareContextResponse {
        hardware_md,
        devices,
    };
    (StatusCode::OK, Json(resp)).into_response()
}

pub async fn handle_hardware_reload(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    let tool_count = state.tools_registry.len();

    let context = crate::hardware::load_hardware_context_prompt(&[]);
    let context_length = context.len();

    tracing::info!(
        context_length,
        tool_count,
        "Hardware context reloaded (on-disk read)"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "ok": true,
            "tools": tool_count,
            "context_length": context_length,
        })),
    )
        .into_response()
}

async fn append_to_file(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(content.as_bytes()).await?;
    file.flush().await?;
    Ok(())
}
