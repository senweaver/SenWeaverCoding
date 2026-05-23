// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashSet;
use std::sync::{OnceLock, RwLock};

const SEED_SERVICE_KEYS: &[&str] = &[
    "provider.anthropic",
    "provider.compatible",
    "provider.copilot",
    "provider.gemini",
    "provider.glm",
    "provider.ollama",
    "provider.openai",
    "provider.openrouter",
    "channel.dingtalk",
    "channel.discord",
    "channel.feishu",
    "channel.lark",
    "channel.matrix",
    "channel.mattermost",
    "channel.nextcloud_talk",
    "channel.qq",
    "channel.signal",
    "channel.slack",
    "channel.telegram",
    "channel.wati",
    "channel.whatsapp",
    "tool.browser",
    "tool.composio",
    "tool.http_request",
    "tool.pushover",
    "tool.web_search",
    "memory.embeddings",
    "memory.qdrant",
    "rag.embedding.ollama",
    "tunnel.custom",
    "transcription.groq",
];

const WILDCARD_SELECTORS: &[&str] = &[
    "provider.*",
    "channel.*",
    "tool.*",
    "memory.*",
    "rag.*",
    "tunnel.*",
    "transcription.*",
    "**",
];

pub struct ProxyServiceRegistry {
    services: RwLock<HashSet<String>>,
}

impl ProxyServiceRegistry {
    pub fn new() -> Self {
        let mut set = HashSet::with_capacity(SEED_SERVICE_KEYS.len());
        for key in SEED_SERVICE_KEYS {
            set.insert((*key).to_string());
        }
        Self {
            services: RwLock::new(set),
        }
    }

    pub fn register(&self, service_key: &str) {
        let key = service_key.trim().to_ascii_lowercase();
        if key.is_empty() {
            return;
        }
        let mut guard = match self.services.write() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.insert(key);
    }

    pub fn contains_service(&self, service_key: &str) -> bool {
        let key = service_key.trim().to_ascii_lowercase();
        if key.is_empty() {
            return false;
        }
        match self.services.read() {
            Ok(g) => g.contains(&key),
            Err(p) => p.into_inner().contains(&key),
        }
    }

    pub fn is_valid_selector(&self, selector: &str) -> bool {
        let sel = selector.trim();
        if sel.is_empty() {
            return false;
        }
        let lower = sel.to_ascii_lowercase();
        if WILDCARD_SELECTORS
            .iter()
            .any(|w| w.eq_ignore_ascii_case(&lower))
        {
            return true;
        }
        if lower.ends_with(".*") {
            let prefix = &lower[..lower.len().saturating_sub(2)];
            if !prefix.is_empty() {
                let guard = match self.services.read() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                return guard.iter().any(|k| k.starts_with(&format!("{prefix}.")));
            }
        }
        self.contains_service(&lower)
    }

    pub fn matches(&self, selector: &str, service_key: &str) -> bool {
        let sel = selector.trim().to_ascii_lowercase();
        let key = service_key.trim().to_ascii_lowercase();
        if sel.is_empty() || key.is_empty() {
            return false;
        }
        if sel == "**" {
            return true;
        }
        if sel == key {
            return true;
        }
        if let Some(prefix) = sel.strip_suffix(".*") {
            return key.starts_with(&format!("{prefix}."));
        }
        false
    }

    pub fn snapshot_services(&self) -> Vec<String> {
        let guard = match self.services.read() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let mut v: Vec<String> = guard.iter().cloned().collect();
        v.sort();
        v
    }
}

impl Default for ProxyServiceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

static GLOBAL_PROXY_REGISTRY: OnceLock<ProxyServiceRegistry> = OnceLock::new();

pub fn global() -> &'static ProxyServiceRegistry {
    GLOBAL_PROXY_REGISTRY.get_or_init(ProxyServiceRegistry::new)
}

pub fn register(service_key: &str) {
    global().register(service_key);
}

pub fn is_valid_selector(selector: &str) -> bool {
    global().is_valid_selector(selector)
}

pub fn matches(selector: &str, service_key: &str) -> bool {
    global().matches(selector, service_key)
}

pub fn snapshot_services() -> Vec<String> {
    global().snapshot_services()
}
