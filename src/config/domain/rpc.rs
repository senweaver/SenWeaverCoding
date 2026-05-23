// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct RpcConfig {

    #[serde(default = "default_rpc_enabled")]
    pub enabled: bool,

    #[serde(default = "default_rpc_stdio")]
    pub stdio: bool,

    #[serde(default)]
    pub unix_socket: Option<String>,

    #[serde(default)]
    pub http: Option<RpcHttpConfig>,

    #[serde(default = "default_rpc_session_timeout")]
    pub session_timeout_secs: u64,

    #[serde(default = "default_rpc_max_sessions")]
    pub max_sessions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RpcHttpConfig {
    #[serde(default = "default_rpc_http_host")]
    pub host: String,
    #[serde(default = "default_rpc_http_port")]
    pub port: u16,
}

pub(crate) fn default_rpc_enabled() -> bool {
    true
}
pub(crate) fn default_rpc_stdio() -> bool {
    true
}
pub(crate) fn default_rpc_session_timeout() -> u64 {
    300
}
pub(crate) fn default_rpc_max_sessions() -> usize {
    100
}
pub(crate) fn default_rpc_http_host() -> String {
    "127.0.0.1".into()
}
pub(crate) fn default_rpc_http_port() -> u16 {
    42_618
}

impl Default for RpcHttpConfig {
    fn default() -> Self {
        Self {
            host: default_rpc_http_host(),
            port: default_rpc_http_port(),
        }
    }
}

impl RpcConfig {

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if !self.enabled {
            return errors;
        }
        if !self.stdio && self.unix_socket.is_none() && self.http.is_none() {
            errors.push(
                "rpc.enabled=true but no transport configured (stdio/unix_socket/http)".into(),
            );
        }
        if self.session_timeout_secs == 0 {
            errors.push("rpc.session_timeout_secs must be > 0".into());
        }
        if self.max_sessions == 0 {
            errors.push("rpc.max_sessions must be >= 1".into());
        }
        if self.max_sessions > 10_000 {
            errors.push("rpc.max_sessions > 10_000 is unusual — likely misconfigured".into());
        }
        if let Some(ref http) = self.http {
            if http.port == 0 {
                errors.push("rpc.http.port must be > 0".into());
            }
            if http.host.trim().is_empty() {
                errors.push("rpc.http.host must be non-empty".into());
            }
        }
        if let Some(ref path) = self.unix_socket {
            if path.trim().is_empty() {
                errors.push("rpc.unix_socket must be a non-empty path".into());
            }
        }
        errors
    }
}
