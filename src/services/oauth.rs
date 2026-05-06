// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// OAuth service — mirrors claude-code-typescript-src`services/oauth/`.
// Handles OAuth2 flows for API authentication and third-party integrations.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_type: String,
    pub expires_at_epoch_ms: Option<u64>,
    pub scope: Option<String>,
}

impl OAuthTokens {

    pub fn is_expired(&self) -> bool {
        if let Some(exp) = self.expires_at_epoch_ms {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            now + 60_000 >= exp
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthProviderConfig {
    pub provider_name: String,
    pub client_id: String,
    pub auth_url: String,
    pub token_url: String,
    pub scopes: Vec<String>,
    pub redirect_uri: String,
}

#[derive(Clone)]
pub struct OAuthService {
    inner: Arc<RwLock<OAuthInner>>,
}

struct OAuthInner {
    providers: HashMap<String, OAuthProviderConfig>,
    tokens: HashMap<String, OAuthTokens>,
    pending_flows: HashMap<String, PendingOAuthFlow>,
}

struct PendingOAuthFlow {
    state: String,
    code_verifier: Option<String>,
    provider: String,
    started_at_ms: u64,
}

impl OAuthService {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(OAuthInner {
                providers: HashMap::new(),
                tokens: HashMap::new(),
                pending_flows: HashMap::new(),
            })),
        }
    }

    pub async fn register_provider(&self, config: OAuthProviderConfig) {
        let mut inner = self.inner.write().await;
        inner.providers.insert(config.provider_name.clone(), config);
    }

    pub async fn get_tokens(&self, provider: &str) -> Option<OAuthTokens> {
        let inner = self.inner.read().await;
        inner.tokens.get(provider).cloned()
    }

    pub async fn set_tokens(&self, provider: &str, tokens: OAuthTokens) {
        let mut inner = self.inner.write().await;
        inner.tokens.insert(provider.to_string(), tokens);
    }

    pub async fn clear_tokens(&self, provider: &str) {
        let mut inner = self.inner.write().await;
        inner.tokens.remove(provider);
    }

    pub async fn is_authenticated(&self, provider: &str) -> bool {
        let inner = self.inner.read().await;
        inner
            .tokens
            .get(provider)
            .map(|t| !t.is_expired())
            .unwrap_or(false)
    }

    pub async fn start_auth_flow(&self, provider: &str) -> anyhow::Result<String> {
        let inner = self.inner.read().await;
        let config = inner
            .providers
            .get(provider)
            .ok_or_else(|| anyhow::anyhow!("Unknown OAuth provider: {provider}"))?;

        let state = uuid::Uuid::new_v4().to_string();
        let scopes = config.scopes.join(" ");
        let url = format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}",
            config.auth_url,
            urlencoding::encode(&config.client_id),
            urlencoding::encode(&config.redirect_uri),
            urlencoding::encode(&scopes),
            urlencoding::encode(&state),
        );

        drop(inner);

        let mut inner = self.inner.write().await;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        inner.pending_flows.insert(
            state.clone(),
            PendingOAuthFlow {
                state,
                code_verifier: None,
                provider: provider.to_string(),
                started_at_ms: now,
            },
        );

        Ok(url)
    }

    pub async fn list_providers(&self) -> Vec<String> {
        let inner = self.inner.read().await;
        inner.providers.keys().cloned().collect()
    }
}

impl Default for OAuthService {
    fn default() -> Self {
        Self::new()
    }
}
