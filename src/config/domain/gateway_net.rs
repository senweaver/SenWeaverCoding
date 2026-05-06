// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Gateway networking sub-schema.
//!
//! This is the **first** domain-split config module (P6.1).  The canonical
//! `GatewayConfig` still lives in `schema.rs` for backward compatibility,
//! but new gateway fields should be added to `GatewayNetExtras` here and
//! aggregated at the top level once the full split is complete.
//!
//! `validate()` runs all built-in gateway invariants and returns a list
//! of human-readable errors, which the loader can present to the user
//! before applying the new config.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct GatewayNetExtras {

    #[serde(default)]
    pub force_loopback: bool,

    #[serde(default)]
    pub auth_override: std::collections::HashMap<String, String>,

    #[serde(default = "default_max_body_bytes")]
    pub max_body_bytes: usize,

    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,
}

fn default_max_body_bytes() -> usize {
    1_048_576
}

fn default_request_timeout_secs() -> u64 {
    30
}

impl GatewayNetExtras {

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        if self.request_timeout_secs == 0 {
            errors.push("gateway_net.request_timeout_secs must be > 0".into());
        }
        if self.request_timeout_secs > 600 {
            errors.push("gateway_net.request_timeout_secs > 600 is almost certainly wrong".into());
        }

        let allowed = ["anonymous", "local_only", "bearer", "mutual"];
        for (prefix, level) in &self.auth_override {
            if !allowed.contains(&level.as_str()) {
                errors.push(format!(
                    "gateway_net.auth_override['{prefix}'] = '{level}' — must be one of {allowed:?}"
                ));
            }
            if !prefix.starts_with('/') {
                errors.push(format!(
                    "gateway_net.auth_override prefix '{prefix}' must start with '/'"
                ));
            }
        }

        errors
    }
}
