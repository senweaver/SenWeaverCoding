// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnthropicAuthKind {

    ApiKey,

    Authorization,
}

impl AnthropicAuthKind {
    pub fn as_metadata_value(self) -> &'static str {
        match self {
            Self::ApiKey => "api-key",
            Self::Authorization => "authorization",
        }
    }

    pub fn from_metadata_value(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "api-key" | "x-api-key" | "apikey" => Some(Self::ApiKey),
            "authorization" | "bearer" | "auth-token" | "oauth" => Some(Self::Authorization),
            _ => None,
        }
    }
}

pub fn detect_auth_kind(token: &str, explicit: Option<&str>) -> AnthropicAuthKind {
    if let Some(kind) = explicit.and_then(AnthropicAuthKind::from_metadata_value) {
        return kind;
    }

    let trimmed = token.trim();

    if trimmed.matches('.').count() >= 2 {
        return AnthropicAuthKind::Authorization;
    }

    if trimmed.starts_with("sk-ant-api") {
        return AnthropicAuthKind::ApiKey;
    }

    AnthropicAuthKind::ApiKey
}
