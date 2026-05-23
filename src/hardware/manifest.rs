// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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
