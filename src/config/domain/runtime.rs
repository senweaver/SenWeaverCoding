// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeConfig {

    #[serde(default = "default_runtime_kind")]
    pub kind: String,

    #[serde(default)]
    pub docker: DockerRuntimeConfig,

    #[serde(default)]
    pub wasm: WasmRuntimeConfig,

    #[serde(default)]
    pub reasoning_enabled: Option<bool>,

    #[serde(
        default,
        deserialize_with = "crate::config::schema::deserialize_reasoning_effort_opt"
    )]
    pub reasoning_effort: Option<String>,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            kind: default_runtime_kind(),
            docker: DockerRuntimeConfig::default(),
            wasm: WasmRuntimeConfig::default(),
            reasoning_enabled: None,
            reasoning_effort: None,
        }
    }
}

impl RuntimeConfig {

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        let kind = self.kind.trim();
        match kind {
            "native" | "docker" => {}
            "wasm" => {
                #[cfg(not(feature = "runtime-wasm"))]
                {
                    errors.push(
                        "runtime.kind='wasm' requires the `runtime-wasm` feature \
                         (rebuild with --features runtime-wasm)"
                            .to_string(),
                    );
                }
            }
            "cloudflare" => {
                if std::env::var("SEN_CF_EXPERIMENTAL").is_err() {
                    errors.push(
                        "runtime.kind='cloudflare' is experimental and requires \
                         SEN_CF_EXPERIMENTAL=1"
                            .to_string(),
                    );
                }
            }
            "" => {
                errors.push(
                    "runtime.kind cannot be empty. Supported values: native, docker, wasm \
                     (cloudflare requires SEN_CF_EXPERIMENTAL=1)"
                        .to_string(),
                );
            }
            other => {
                errors.push(format!(
                    "runtime.kind '{other}' must be one of: native, docker, wasm \
                     (cloudflare requires SEN_CF_EXPERIMENTAL=1)"
                ));
            }
        }
        if self.kind.trim() == "docker" {
            errors.extend(self.docker.validate());
        }
        if self.kind.trim() == "wasm" {
            errors.extend(self.wasm.validate());
        }
        if let Some(ref effort) = self.reasoning_effort {
            if let Err(msg) = crate::config::schema::normalize_reasoning_effort(effort) {
                errors.push(msg);
            }
        }
        errors
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DockerRuntimeConfig {

    #[serde(default = "default_docker_image")]
    pub image: String,

    #[serde(default = "default_docker_network")]
    pub network: String,

    #[serde(default = "default_docker_memory_limit_mb")]
    pub memory_limit_mb: Option<u64>,

    #[serde(default = "default_docker_cpu_limit")]
    pub cpu_limit: Option<f64>,

    #[serde(default = "default_true_bool")]
    pub read_only_rootfs: bool,

    #[serde(default = "default_true_bool")]
    pub mount_workspace: bool,

    #[serde(default)]
    pub allowed_workspace_roots: Vec<String>,
}

impl Default for DockerRuntimeConfig {
    fn default() -> Self {
        Self {
            image: default_docker_image(),
            network: default_docker_network(),
            memory_limit_mb: default_docker_memory_limit_mb(),
            cpu_limit: default_docker_cpu_limit(),
            read_only_rootfs: true,
            mount_workspace: true,
            allowed_workspace_roots: Vec::new(),
        }
    }
}

impl DockerRuntimeConfig {
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.image.trim().is_empty() {
            errors.push("runtime.docker.image must be non-empty".into());
        }
        if self.network.trim().is_empty() {
            errors.push("runtime.docker.network must be non-empty".into());
        }
        if let Some(0) = self.memory_limit_mb {
            errors.push("runtime.docker.memory_limit_mb must be > 0 when set".into());
        }
        if let Some(c) = self.cpu_limit {
            if c <= 0.0 {
                errors.push("runtime.docker.cpu_limit must be > 0 when set".into());
            }
        }
        errors
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WasmRuntimeConfig {

    #[serde(default = "default_wasm_fuel_limit")]
    pub fuel_limit: u64,

    #[serde(default = "default_wasm_memory_limit")]
    pub memory_limit_mb: u64,

    #[serde(default = "default_wasm_tools_dir")]
    pub tools_dir: String,

    #[serde(default)]
    pub allow_workspace_read: bool,

    #[serde(default)]
    pub allow_workspace_write: bool,

    #[serde(default)]
    pub allowed_hosts: Vec<String>,
}

impl Default for WasmRuntimeConfig {
    fn default() -> Self {
        Self {
            fuel_limit: default_wasm_fuel_limit(),
            memory_limit_mb: default_wasm_memory_limit(),
            tools_dir: default_wasm_tools_dir(),
            allow_workspace_read: false,
            allow_workspace_write: false,
            allowed_hosts: Vec::new(),
        }
    }
}

impl WasmRuntimeConfig {
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.fuel_limit == 0 {
            errors.push("runtime.wasm.fuel_limit must be > 0".into());
        }
        if self.memory_limit_mb == 0 {
            errors.push("runtime.wasm.memory_limit_mb must be > 0".into());
        }
        if self.tools_dir.trim().is_empty() {
            errors.push("runtime.wasm.tools_dir must be non-empty".into());
        }
        errors
    }
}

pub(crate) fn default_runtime_kind() -> String {
    "native".into()
}
pub(crate) fn default_docker_image() -> String {
    "alpine:3.20".into()
}
pub(crate) fn default_docker_network() -> String {
    "none".into()
}
pub(crate) fn default_docker_memory_limit_mb() -> Option<u64> {
    Some(512)
}
pub(crate) fn default_docker_cpu_limit() -> Option<f64> {
    Some(1.0)
}
pub(crate) fn default_wasm_fuel_limit() -> u64 {
    1_000_000
}
pub(crate) fn default_wasm_memory_limit() -> u64 {
    64
}
pub(crate) fn default_wasm_tools_dir() -> String {
    "tools/wasm".into()
}
pub(crate) fn default_true_bool() -> bool {
    true
}
