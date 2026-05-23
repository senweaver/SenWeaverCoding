// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SecurityOpsExtras {

    #[serde(default = "default_true")]
    pub enforce_pid_ownership: bool,

    #[serde(default = "default_pid_start_drift_secs")]
    pub pid_start_drift_secs: u64,

    #[serde(default = "default_redact_log_level")]
    pub redact_log_level: String,

    #[serde(default = "default_audit_retention_days")]
    pub audit_retention_days: u32,

    #[serde(default)]
    pub require_request_id: bool,

    #[serde(default = "default_auth_failure_threshold")]
    pub auth_failure_threshold_per_minute: u32,
}

fn default_true() -> bool {
    true
}
fn default_pid_start_drift_secs() -> u64 {
    2
}
fn default_redact_log_level() -> String {
    "info".into()
}
fn default_audit_retention_days() -> u32 {
    90
}
fn default_auth_failure_threshold() -> u32 {
    10
}

impl Default for SecurityOpsExtras {
    fn default() -> Self {
        Self {
            enforce_pid_ownership: default_true(),
            pid_start_drift_secs: default_pid_start_drift_secs(),
            redact_log_level: default_redact_log_level(),
            audit_retention_days: default_audit_retention_days(),
            require_request_id: false,
            auth_failure_threshold_per_minute: default_auth_failure_threshold(),
        }
    }
}

impl SecurityOpsExtras {
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        let allowed = ["debug", "info", "warn"];
        if !allowed.contains(&self.redact_log_level.as_str()) {
            errors.push(format!(
                "security_ops.redact_log_level must be one of {allowed:?}, got '{}'",
                self.redact_log_level
            ));
        }
        if self.pid_start_drift_secs > 60 {
            errors.push(
                "security_ops.pid_start_drift_secs > 60 defeats the purpose of the check".into(),
            );
        }
        if self.audit_retention_days == 0 {

            errors.push(
                "security_ops.audit_retention_days = 0 means no automatic rotation — ensure external archival".into(),
            );
        }
        if self.auth_failure_threshold_per_minute == 0 {
            errors.push(
                "security_ops.auth_failure_threshold_per_minute = 0 disables rate limiting".into(),
            );
        }
        errors
    }
}
