// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
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
    #[serde(default)]
    pub client_secret: Option<String>,
}

#[derive(Clone)]
pub struct OAuthService {
    inner: Arc<RwLock<OAuthInner>>,
    storage: Arc<RwLock<Option<OAuthStorage>>>,
}

struct OAuthStorage {
    path: std::path::PathBuf,
    secrets: crate::security::secrets::SecretStore,
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

const OAUTH_PENDING_FLOW_TTL_MS: u64 = 10 * 60 * 1000;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn generate_pkce_pair() -> (String, String) {
    use base64::Engine as _;
    use sha2::{Digest, Sha256};

    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

impl OAuthService {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(OAuthInner {
                providers: HashMap::new(),
                tokens: HashMap::new(),
                pending_flows: HashMap::new(),
            })),
            storage: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn configure_persistence(&self, state_dir: &std::path::Path, encrypt: bool) {
        let path = state_dir.join("oauth-tokens.json");
        let secrets = crate::security::secrets::SecretStore::new(state_dir, encrypt);
        let loaded: HashMap<String, OAuthTokens> = std::fs::read_to_string(&path)
            .ok()
            .and_then(|blob| secrets.decrypt(&blob).ok())
            .and_then(|plain| serde_json::from_str(&plain).ok())
            .unwrap_or_default();
        if !loaded.is_empty() {
            let mut inner = self.inner.write().await;
            for (k, v) in loaded {
                inner.tokens.entry(k).or_insert(v);
            }
        }
        *self.storage.write().await = Some(OAuthStorage { path, secrets });
    }

    async fn persist_tokens(&self) {
        let storage_guard = self.storage.read().await;
        let Some(storage) = storage_guard.as_ref() else {
            return;
        };
        let snapshot = {
            let inner = self.inner.read().await;
            inner.tokens.clone()
        };
        let json = match serde_json::to_string(&snapshot) {
            Ok(j) => j,
            Err(_) => return,
        };
        match storage.secrets.encrypt(&json) {
            Ok(blob) => {
                if let Err(e) = crate::util::atomic_write(&storage.path, blob.as_bytes()) {
                    tracing::warn!("failed to persist oauth tokens: {e}");
                }
            }
            Err(e) => tracing::warn!("failed to encrypt oauth tokens: {e}"),
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
        {
            let mut inner = self.inner.write().await;
            inner.tokens.insert(provider.to_string(), tokens);
        }
        self.persist_tokens().await;
    }

    pub async fn clear_tokens(&self, provider: &str) {
        {
            let mut inner = self.inner.write().await;
            inner.tokens.remove(provider);
        }
        self.persist_tokens().await;
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
        self.start_auth_flow_with_pkce(provider, true).await
    }

    pub async fn start_auth_flow_with_pkce(
        &self,
        provider: &str,
        enable_pkce: bool,
    ) -> anyhow::Result<String> {
        let inner = self.inner.read().await;
        let config = inner
            .providers
            .get(provider)
            .ok_or_else(|| anyhow::anyhow!("Unknown OAuth provider: {provider}"))?
            .clone();
        drop(inner);

        let state = uuid::Uuid::new_v4().to_string();
        let scopes = config.scopes.join(" ");
        let (code_verifier, challenge_segment) = if enable_pkce {
            let (verifier, challenge) = generate_pkce_pair();
            let segment = format!(
                "&code_challenge={}&code_challenge_method=S256",
                urlencoding::encode(&challenge)
            );
            (Some(verifier), segment)
        } else {
            (None, String::new())
        };

        let url = format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}{}",
            config.auth_url,
            urlencoding::encode(&config.client_id),
            urlencoding::encode(&config.redirect_uri),
            urlencoding::encode(&scopes),
            urlencoding::encode(&state),
            challenge_segment,
        );

        let mut inner = self.inner.write().await;
        let cutoff = now_ms().saturating_sub(OAUTH_PENDING_FLOW_TTL_MS);
        inner
            .pending_flows
            .retain(|_, flow| flow.started_at_ms >= cutoff);
        inner.pending_flows.insert(
            state.clone(),
            PendingOAuthFlow {
                state: state.clone(),
                code_verifier,
                provider: provider.to_string(),
                started_at_ms: now_ms(),
            },
        );

        Ok(url)
    }

    pub async fn consume_pending_flow(
        &self,
        state: &str,
    ) -> anyhow::Result<(String, Option<String>)> {
        let mut inner = self.inner.write().await;
        let flow = inner
            .pending_flows
            .remove(state)
            .ok_or_else(|| anyhow::anyhow!("OAuth state token not recognized or already used"))?;
        if flow.state != state {
            anyhow::bail!("OAuth state mismatch");
        }
        if flow.started_at_ms + OAUTH_PENDING_FLOW_TTL_MS < now_ms() {
            anyhow::bail!("OAuth flow expired; restart authentication");
        }
        Ok((flow.provider, flow.code_verifier))
    }

    pub async fn complete_auth_flow(
        &self,
        state: &str,
        tokens: OAuthTokens,
    ) -> anyhow::Result<String> {
        let (provider, _code_verifier) = self.consume_pending_flow(state).await?;
        self.set_tokens(&provider, tokens).await;
        Ok(provider)
    }

    pub async fn exchange_code_and_complete(
        &self,
        state: &str,
        code: &str,
    ) -> anyhow::Result<String> {
        let (provider, code_verifier) = self.consume_pending_flow(state).await?;
        let config = {
            let inner = self.inner.read().await;
            inner
                .providers
                .get(&provider)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Unknown OAuth provider: {provider}"))?
        };

        let mut form = vec![
            ("grant_type", "authorization_code".to_string()),
            ("code", code.to_string()),
            ("redirect_uri", config.redirect_uri.clone()),
            ("client_id", config.client_id.clone()),
        ];
        if let Some(secret) = &config.client_secret {
            form.push(("client_secret", secret.clone()));
        }
        if let Some(verifier) = &code_verifier {
            form.push(("code_verifier", verifier.clone()));
        }

        let client = reqwest::Client::new();
        let response = client
            .post(&config.token_url)
            .form(&form)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("OAuth token request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| String::new());
        if !status.is_success() {
            anyhow::bail!("OAuth token exchange failed ({status}): {body}");
        }
        let value: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| anyhow::anyhow!("Invalid OAuth token JSON: {e}"))?;
        let access_token = value
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("OAuth token response missing access_token"))?
            .to_string();
        let refresh_token = value
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let token_type = value
            .get("token_type")
            .and_then(|v| v.as_str())
            .unwrap_or("Bearer")
            .to_string();
        let scope = value
            .get("scope")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let expires_at_epoch_ms = value
            .get("expires_in")
            .and_then(|v| v.as_u64())
            .map(|secs| now_ms().saturating_add(secs.saturating_mul(1000)));

        self.set_tokens(
            &provider,
            OAuthTokens {
                access_token,
                refresh_token,
                token_type,
                expires_at_epoch_ms,
                scope,
            },
        )
        .await;
        Ok(provider)
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
