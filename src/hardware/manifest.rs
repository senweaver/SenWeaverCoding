// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Plugin manifest — `~/.senweavercoding/tools/<name>/tool.toml` schema.
//!
//! Each user plugin lives in its own subdirectory and carries a `tool.toml`
//! that describes the tool, how to invoke it, and what parameters it accepts.
//!
//! Example `tool.toml`:
//! ```toml
//! [tool]
//! name        = "i2c_scan"
//! version     = "1.0.0"
//! description = "Scan the I2C bus for connected devices"
//!
//! [exec]
//! binary = "i2c_scan.py"
//!
//! [transport]
//! preferred       = "serial"
//! device_required = true
//!
//! [[parameters]]
//! name        = "device"
//! type        = "string"
//! description = "Device alias e.g. pico0"
//! required    = true
//!
//! [[parameters]]
//! name        = "bus"
//! type        = "integer"
//! description = "I2C bus number (default 0)"
//! required    = false
//! default     = 0
//! ```

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ToolManifest {

    pub tool: ToolMeta,

    pub exec: ExecConfig,

    pub transport: Option<TransportConfig>,

    #[serde(default)]
    pub parameters: Vec<ParameterDef>,
}

#[derive(Debug, Deserialize)]
pub struct ToolMeta {

    pub name: String,

    pub version: String,

    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct ExecConfig {

    pub binary: String,
}

#[derive(Debug, Deserialize)]
pub struct TransportConfig {

    pub preferred: String,

    pub device_required: bool,
}

#[derive(Debug, Deserialize)]
pub struct ParameterDef {

    pub name: String,

    #[serde(rename = "type")]
    pub r#type: String,

    pub description: String,

    pub required: bool,

    pub default: Option<serde_json::Value>,
}
