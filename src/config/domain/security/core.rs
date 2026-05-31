// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::schema::{
    AuditConfig, EstopConfig, NevisConfig, OtpConfig, ResourceLimitsConfig, SandboxConfig,
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WebAuthnConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_webauthn_rp_id")]
    pub rp_id: String,

    #[serde(default = "default_webauthn_rp_origin")]
    pub rp_origin: String,

    #[serde(default = "default_webauthn_rp_name")]
    pub rp_name: String,
}

impl Default for WebAuthnConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            rp_id: default_webauthn_rp_id(),
            rp_origin: default_webauthn_rp_origin(),
            rp_name: default_webauthn_rp_name(),
        }
    }
}

impl WebAuthnConfig {

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if !self.enabled {
            return errors;
        }
        if self.rp_id.trim().is_empty() {
            errors.push("security.webauthn.rp_id must be non-empty when enabled".into());
        }
        if self.rp_origin.trim().is_empty() {
            errors.push("security.webauthn.rp_origin must be non-empty when enabled".into());
        } else if !self.rp_origin.starts_with("http://") && !self.rp_origin.starts_with("https://")
        {
            errors.push(format!(
                "security.webauthn.rp_origin must be an HTTP(S) URL, got '{}'",
                self.rp_origin
            ));
        }
        if self.rp_name.trim().is_empty() {
            errors.push("security.webauthn.rp_name must be non-empty when enabled".into());
        }
        errors
    }
}

pub(crate) fn default_webauthn_rp_id() -> String {
    "localhost".into()
}
pub(crate) fn default_webauthn_rp_origin() -> String {
    "http://localhost:42617".into()
}
pub(crate) fn default_webauthn_rp_name() -> String {
    "SenWeaverCoding".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct SecurityConfig {

    #[serde(default)]
    pub sandbox: SandboxConfig,

    #[serde(default)]
    pub resources: ResourceLimitsConfig,

    #[serde(default)]
    pub audit: AuditConfig,

    #[serde(default)]
    pub otp: OtpConfig,

    #[serde(default)]
    pub estop: EstopConfig,

    #[serde(default)]
    pub nevis: NevisConfig,

    #[serde(default)]
    pub webauthn: WebAuthnConfig,
}
