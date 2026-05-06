// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! WASM plugin system for SenWeaverCoding.
//!
//! Plugins are WebAssembly modules loaded via Extism that can extend
//! SenWeaverCoding with custom tools and channels. Enable with `--features plugins-wasm`.

pub mod error;
pub mod host;
pub mod signature;
pub mod wasm_channel;
pub mod wasm_tool;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {

    pub name: String,

    pub version: String,

    pub description: Option<String>,

    pub author: Option<String>,

    pub wasm_path: String,

    pub capabilities: Vec<PluginCapability>,

    #[serde(default)]
    pub permissions: Vec<PluginPermission>,

    #[serde(default)]
    pub signature: Option<String>,

    #[serde(default)]
    pub publisher_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginCapability {

    Tool,

    Channel,

    Memory,

    Observer,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginPermission {

    HttpClient,

    FileRead,

    FileWrite,

    EnvRead,

    MemoryRead,

    MemoryWrite,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub capabilities: Vec<PluginCapability>,
    pub permissions: Vec<PluginPermission>,
    pub wasm_path: PathBuf,
    pub loaded: bool,
}
