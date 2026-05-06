// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct Microsoft365ResolvedConfig {
    pub tenant_id: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub auth_flow: String,
    pub scopes: Vec<String>,
    pub token_cache_encrypted: bool,
    pub user_id: String,
}

impl std::fmt::Debug for Microsoft365ResolvedConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Microsoft365ResolvedConfig")
            .field("tenant_id", &self.tenant_id)
            .field("client_id", &self.client_id)
            .field("client_secret", &self.client_secret.as_ref().map(|_| "***"))
            .field("auth_flow", &self.auth_flow)
            .field("scopes", &self.scopes)
            .field("token_cache_encrypted", &self.token_cache_encrypted)
            .field("user_id", &self.user_id)
            .finish()
    }
}
