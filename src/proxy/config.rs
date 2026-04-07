// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Proxy configuration — mirrors claude-code-typescript-src`upstreamproxy/upstreamproxy.ts`.

use serde::{Deserialize, Serialize};

/// Upstream proxy configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// Whether upstream proxying is enabled.
    pub enabled: bool,
    /// The upstream proxy URL (e.g. "http://proxy.internal:8080").
    pub url: Option<String>,
    /// Whether to use the system proxy settings (HTTP_PROXY / HTTPS_PROXY).
    pub use_system_proxy: bool,
    /// Optional bearer token for proxy authentication.
    pub auth_token: Option<String>,
    /// Request timeout in milliseconds (0 = no timeout).
    pub timeout_ms: u64,
    /// Whether to verify SSL certificates (set to false for self-signed certs).
    pub verify_ssl: bool,
    /// Additional headers to include in proxied requests.
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
    /// Resolve the effective proxy URL from config or environment.
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
