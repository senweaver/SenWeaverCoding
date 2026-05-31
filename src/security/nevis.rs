// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NevisIdentity {

    pub user_id: String,

    pub roles: Vec<String>,

    pub scopes: Vec<String>,

    pub mfa_verified: bool,

    pub session_expiry: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenValidationMode {

    Local,

    Remote,
}

impl TokenValidationMode {
    pub fn from_str_config(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "remote" => Ok(Self::Remote),
            other => bail!("invalid token_validation mode '{other}': expected 'local' or 'remote'"),
        }
    }
}

pub struct NevisAuthProvider {

    instance_url: String,

    realm: String,

    client_id: String,

    client_secret: Option<String>,

    validation_mode: TokenValidationMode,

    jwks_url: Option<String>,

    require_mfa: bool,

    session_timeout: Duration,

    http_client: reqwest::Client,
}

impl std::fmt::Debug for NevisAuthProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NevisAuthProvider")
            .field("instance_url", &self.instance_url)
            .field("realm", &self.realm)
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "[REDACTED]"),
            )
            .field("validation_mode", &self.validation_mode)
            .field("jwks_url", &self.jwks_url)
            .field("require_mfa", &self.require_mfa)
            .field("session_timeout", &self.session_timeout)
            .finish_non_exhaustive()
    }
}

#[allow(clippy::used_underscore_items)]
const _: () = {
    fn _assert_send_sync<T: Send + Sync>() {}
    fn _assert() {
        _assert_send_sync::<NevisAuthProvider>();
    }
};

impl NevisAuthProvider {

    pub fn new(
        instance_url: String,
        realm: String,
        client_id: String,
        client_secret: Option<String>,
        token_validation: &str,
        jwks_url: Option<String>,
        require_mfa: bool,
        session_timeout_secs: u64,
    ) -> Result<Self> {
        let validation_mode = TokenValidationMode::from_str_config(token_validation)?;

        if validation_mode == TokenValidationMode::Local && jwks_url.is_none() {
            bail!(
                "Nevis token_validation is 'local' but no jwks_url is configured. \
                 Either set jwks_url or use token_validation = 'remote'."
            );
        }

        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .context("Failed to create HTTP client for Nevis")?;

        Ok(Self {
            instance_url,
            realm,
            client_id,
            client_secret,
            validation_mode,
            jwks_url,
            require_mfa,
            session_timeout: Duration::from_secs(session_timeout_secs),
            http_client,
        })
    }

    pub async fn validate_token(&self, token: &str) -> Result<NevisIdentity> {
        if token.is_empty() {
            bail!("empty bearer token");
        }

        let identity = match self.validation_mode {
            TokenValidationMode::Local => self.validate_token_local(token).await?,
            TokenValidationMode::Remote => self.validate_token_remote(token).await?,
        };

        if self.require_mfa && !identity.mfa_verified {
            bail!(
                "MFA is required but user '{}' has not completed MFA verification",
                crate::security::redact(&identity.user_id)
            );
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if identity.session_expiry == 0 {
            tracing::warn!("Nevis token has no expiry claim; applying default session timeout");
            let default_expiry = now + self.session_timeout.as_secs();
            if default_expiry < now {
                bail!("Nevis session expired (no exp claim, default timeout exceeded)");
            }
        } else if identity.session_expiry < now {
            bail!("Nevis session expired");
        }

        Ok(identity)
    }

    async fn validate_token_remote(&self, token: &str) -> Result<NevisIdentity> {
        let introspect_url = format!(
            "{}/auth/realms/{}/protocol/openid-connect/token/introspect",
            self.instance_url.trim_end_matches('/'),
            self.realm,
        );

        let mut form = vec![("token", token), ("client_id", &self.client_id)];

        let secret_ref;
        if let Some(ref secret) = self.client_secret {
            secret_ref = secret.as_str();
            form.push(("client_secret", secret_ref));
        }

        let resp = self
            .http_client
            .post(&introspect_url)
            .form(&form)
            .send()
            .await
            .context("Failed to reach Nevis introspection endpoint")?;

        if !resp.status().is_success() {
            bail!(
                "Nevis introspection returned HTTP {}",
                resp.status().as_u16()
            );
        }

        let body: IntrospectionResponse = resp
            .json()
            .await
            .context("Failed to parse Nevis introspection response")?;

        if !body.active {
            bail!("Token is not active (revoked or expired)");
        }

        let user_id = body
            .sub
            .filter(|s| !s.trim().is_empty())
            .context("Token has missing or empty `sub` claim")?;

        let mut roles = body.realm_access.map(|ra| ra.roles).unwrap_or_default();
        roles.sort();
        roles.dedup();

        Ok(NevisIdentity {
            user_id,
            roles,
            scopes: body
                .scope
                .unwrap_or_default()
                .split_whitespace()
                .map(String::from)
                .collect(),
            mfa_verified: body.acr.as_deref() == Some("mfa")
                || body
                    .amr
                    .iter()
                    .flatten()
                    .any(|m| m == "fido2" || m == "passkey" || m == "otp" || m == "webauthn"),
            session_expiry: body.exp.unwrap_or(0),
        })
    }

    async fn validate_token_local(&self, token: &str) -> Result<NevisIdentity> {
        let jwks_url = self
            .jwks_url
            .as_deref()
            .filter(|u| !u.trim().is_empty())
            .context("Nevis token_validation is 'local' but jwks_url is not configured")?;

        let jwks: jsonwebtoken::jwk::JwkSet = self
            .http_client
            .get(jwks_url)
            .send()
            .await
            .context("Failed to fetch Nevis JWKS")?
            .json()
            .await
            .context("Failed to parse Nevis JWKS")?;

        let header = jsonwebtoken::decode_header(token).context("Invalid JWT header")?;
        let kid = header
            .kid
            .as_deref()
            .context("JWT header missing kid for JWKS lookup")?;
        let jwk = jwks
            .find(kid)
            .with_context(|| format!("JWT kid '{kid}' not found in JWKS"))?;
        let decoding_key =
            jsonwebtoken::DecodingKey::from_jwk(jwk).context("Unsupported JWKS key material")?;

        let issuer = format!(
            "{}/auth/realms/{}",
            self.instance_url.trim_end_matches('/'),
            self.realm,
        );
        let mut validation = jsonwebtoken::Validation::new(header.alg);
        validation.set_audience(&[&self.client_id]);
        validation.set_issuer(&[issuer.as_str()]);
        validation.validate_exp = true;

        let token_data = jsonwebtoken::decode::<LocalJwtClaims>(token, &decoding_key, &validation)
            .context("JWT signature or claim validation failed")?;
        let body = token_data.claims;

        let user_id = body
            .sub
            .filter(|s| !s.trim().is_empty())
            .context("Token has missing or empty `sub` claim")?;

        let mut roles = body.realm_access.map(|ra| ra.roles).unwrap_or_default();
        roles.sort();
        roles.dedup();

        Ok(NevisIdentity {
            user_id,
            roles,
            scopes: body
                .scope
                .unwrap_or_default()
                .split_whitespace()
                .map(String::from)
                .collect(),
            mfa_verified: body.acr.as_deref() == Some("mfa")
                || body
                    .amr
                    .iter()
                    .flatten()
                    .any(|m| m == "fido2" || m == "passkey" || m == "otp" || m == "webauthn"),
            session_expiry: body.exp.unwrap_or(0),
        })
    }

    pub async fn validate_session(&self, session_token: &str) -> Result<NevisIdentity> {
        if session_token.is_empty() {
            bail!("empty session token");
        }

        let session_url = format!(
            "{}/auth/realms/{}/protocol/openid-connect/userinfo",
            self.instance_url.trim_end_matches('/'),
            self.realm,
        );

        let resp = self
            .http_client
            .get(&session_url)
            .bearer_auth(session_token)
            .send()
            .await
            .context("Failed to reach Nevis userinfo endpoint")?;

        if !resp.status().is_success() {
            bail!(
                "Nevis session validation returned HTTP {}",
                resp.status().as_u16()
            );
        }

        let body: UserInfoResponse = resp
            .json()
            .await
            .context("Failed to parse Nevis userinfo response")?;

        if body.sub.trim().is_empty() {
            bail!("Userinfo response has missing or empty `sub` claim");
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut roles = body.realm_access.map(|ra| ra.roles).unwrap_or_default();
        roles.sort();
        roles.dedup();

        let identity = NevisIdentity {
            user_id: body.sub,
            roles,
            scopes: body
                .scope
                .unwrap_or_default()
                .split_whitespace()
                .map(String::from)
                .collect(),
            mfa_verified: body.acr.as_deref() == Some("mfa")
                || body
                    .amr
                    .iter()
                    .flatten()
                    .any(|m| m == "fido2" || m == "passkey" || m == "otp" || m == "webauthn"),
            session_expiry: now + self.session_timeout.as_secs(),
        };

        if self.require_mfa && !identity.mfa_verified {
            bail!(
                "MFA is required but user '{}' has not completed MFA verification",
                crate::security::redact(&identity.user_id)
            );
        }

        Ok(identity)
    }

    pub async fn health_check(&self) -> Result<()> {
        let health_url = format!(
            "{}/auth/realms/{}",
            self.instance_url.trim_end_matches('/'),
            self.realm,
        );

        let resp = self
            .http_client
            .get(&health_url)
            .send()
            .await
            .context("Nevis health check failed: cannot reach instance")?;

        if !resp.status().is_success() {
            bail!("Nevis health check failed: HTTP {}", resp.status().as_u16());
        }

        Ok(())
    }

    pub fn instance_url(&self) -> &str {
        &self.instance_url
    }

    pub fn realm(&self) -> &str {
        &self.realm
    }
}

#[derive(Debug, Deserialize)]
struct LocalJwtClaims {
    sub: Option<String>,
    scope: Option<String>,
    exp: Option<u64>,
    #[serde(rename = "realm_access")]
    realm_access: Option<RealmAccess>,
    acr: Option<String>,
    amr: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct IntrospectionResponse {
    active: bool,
    sub: Option<String>,
    scope: Option<String>,
    exp: Option<u64>,
    #[serde(rename = "realm_access")]
    realm_access: Option<RealmAccess>,

    acr: Option<String>,

    amr: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct RealmAccess {
    #[serde(default)]
    roles: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct UserInfoResponse {
    sub: String,
    #[serde(rename = "realm_access")]
    realm_access: Option<RealmAccess>,
    scope: Option<String>,
    acr: Option<String>,

    amr: Option<Vec<String>>,
}

