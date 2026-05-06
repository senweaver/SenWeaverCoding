// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Proxy configuration — mirrors claude-code-typescript-src`upstreamproxy/upstreamproxy.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {

    pub enabled: bool,

    pub url: Option<String>,

    pub use_system_proxy: bool,

    pub auth_token: Option<String>,

    pub timeout_ms: u64,

    pub verify_ssl: bool,

    pub extra_headers: std::collections::HashMap<String, String>,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: None,
            use_system_proxy: true,
            auth_token: None,
            timeout_ms: 30_000,
            verify_ssl: true,
            extra_headers: std::collections::HashMap::new(),
        }
    }
}

impl ProxyConfig {

    pub fn effective_url(&self) -> Option<String> {
        if let Some(ref url) = self.url {
            return Some(url.clone());
        }
        if self.use_system_proxy {
            if let Ok(val) = std::env::var("HTTPS_PROXY") {
                return Some(val);
            }
            if let Ok(val) = std::env::var("HTTP_PROXY") {
                return Some(val);
            }
            if let Ok(val) = std::env::var("https_proxy") {
                return Some(val);
            }
            if let Ok(val) = std::env::var("http_proxy") {
                return Some(val);
            }
        }
        None
    }
}
