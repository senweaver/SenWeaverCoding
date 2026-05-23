// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::config::ProxyConfig;

pub struct ProxyRelay {
    config: ProxyConfig,
}

impl ProxyRelay {
    pub fn new(config: ProxyConfig) -> Self {
        Self { config }
    }

    pub fn is_active(&self) -> bool {
        self.config.enabled && self.config.effective_url().is_some()
    }

    pub fn proxy_url(&self) -> Option<String> {
        if self.config.enabled {
            self.config.effective_url()
        } else {
            None
        }
    }

    pub fn proxy_headers(&self) -> std::collections::HashMap<String, String> {
        let mut headers = self.config.extra_headers.clone();
        if let Some(ref token) = self.config.auth_token {
            headers.insert("Proxy-Authorization".to_string(), format!("Bearer {token}"));
        }
        headers
    }
}
