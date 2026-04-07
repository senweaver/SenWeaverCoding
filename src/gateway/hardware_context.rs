// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Hardware context management endpoints.
//!
//! These endpoints let remote callers (phone, laptop) register GPIO pins and
//! append context to the running agent's hardware knowledge base without SSH.
//!
//! ## Endpoints
//!
//! - `POST /api/hardware/pin`     — register a single GPIO pin assignment
//! - `POST /api/hardware/context` — append raw markdown to a device file
//! - `GET  /api/hardware/context` — read all current hardware context files
//! - `POST /api/hardware/reload`  — verify on-disk context; report what will be
//!                                  used on the next chat request
//!
//! ## Live update semantics
//!
//! SenWeaverCoding's agent loop calls [`crate::hardware::boot`] on **every** request,
//! which re-reads `~/.senweavercoding/hardware/` from disk.  Writing to those files
//! therefore takes effect on the very next `/api/chat` call — no daemon restart
//! needed.  The `/api/hardware/reload` endpoint verifies what is on disk and
//! reports what will be injected into the system prompt next time.
//!
//! ## Security
//!
//! - **Auth**: same `require_auth` helper used by all `/api/*` routes.
//! - **Path traversal**: device aliases are validated to be alphanumeric +
//!   hyphens/underscores only; they are never used as raw path components.
//! - **Append-only**: all writes use `OpenOptions::append(true)` — existing
//!   content cannot be truncated or overwritten through these endpoints.
//! - **Size limit**: individual append payloads are capped at 32 KB.

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

/// Maximum bytes allowed in a single append payload.
const MAX_APPEND_BYTES: usize = 32_768; // 32 KB

// ── Auth helper (re-uses the pattern from api.rs) ─────────────────────────────

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

// ── Path helpers ──────────────────────────────────────────────────────────────

/// Return `~/.senweavercoding/hardware/` or an error string.
fn hardware_dir() -> Result<PathBuf, String> {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().join(".senweavercoding").join("hardware"))
        .ok_or_else(|| "Cannot determine home directory".to_string())
}

/// Validate a device alias: must be non-empty, ≤64 chars, and consist only of
/// alphanumerics, hyphens, and underscores.  Returns an error message on failure.
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

/// Return the path to a device context file, after validating the alias.
fn device_file_path(hw_dir: &std::path::Path, alias: &str) -> Result<PathBuf, &'static str> {
    validate_device_alias(alias)?;
    Ok(hw_dir.join("devices").join(format!("{alias}.md")))
}

// ── POST /api/hardware/pin ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PinRegistrationBody {
    /// Device alias (default: "rpi0").
    #[serde(default = "default_device")]
    pub device: String,
    /// BCM GPIO number.
    pub pin: u32,
    /// Component type/name, e.g. "LED", "Button", "Servo".
    pub component: String,
    /// Optional human notes about this pin, e.g. "red LED, active HIGH".
    #[serde(default)]
    pub notes: String,
}

fn default_device() -> String {
    "rpi0".to_string()
}

/// `POST /api/hardware/pin` — register a single GPIO pin assignment.
///
/// Appends one line to `~/.senweavercoding/hardware/devices/<device>.md`:
/// ```text
/// - GPIO <pin>: <component> — <notes>
/// ```
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
    // Sanitize component + notes: strip newlines to prevent line-injection.
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

    // Create devices dir + file if missing, then append.
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

// ── POST /api/hardware/context ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ContextAppendBody {
    /// Device alias (default: "rpi0").
    #[serde(default = "default_device")]
    pub device: String,
    /// Raw markdown string to append to the device file.
    pub content: String,
}

/// `POST /api/hardware/context` — append raw markdown to a device file.
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

    // Ensure content ends with a newline so successive appends don't merge lines.
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

// ── GET /api/hardware/boards ─────────────────────────────────────────────────

/// Static board metadata for the web dashboard.
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

/// `GET /api/hardware/boards` — list configured boards with metadata for the dashboard.
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

// ── GET /api/hardware/context ─────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct HardwareContextResponse {
    hardware_md: String,
    devices: std::collections::HashMap<String, String>,
}

/// `GET /api/hardware/context` — return all current hardware context file contents.
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

    // Read HARDWARE.md
    let hardware_md = fs::read_to_string(hw_dir.join("HARDWARE.md"))
        .await
        .unwrap_or_default();

    // Read all device files
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

// ── POST /api/hardware/reload ─────────────────────────────────────────────────

/// `POST /api/hardware/reload` — verify on-disk hardware context and report what  
/// will be loaded on the next chat request.
///
/// Since [`crate::hardware::boot`] re-reads from disk on every agent invocation,
/// writing to the hardware files via the other endpoints already takes effect on
/// the next `/api/chat` call.  This endpoint reads the same files and reports
/// the current state so callers can confirm the update landed.
pub async fn handle_hardware_reload(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = require_auth(&state, &headers) {
        return e.into_response();
    }

    // Count currently-registered tools in the gateway state
    let tool_count = state.tools_registry.len();

    // Reload hardware context from disk (same function used by the agent loop)
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

// ── File I/O helper ───────────────────────────────────────────────────────────

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
