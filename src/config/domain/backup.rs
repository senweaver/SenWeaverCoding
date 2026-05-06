// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Backup and data retention configs, migrated out of `schema.rs`
//! as part of M8.  Both types remain `pub use`d from `schema.rs`
//! so downstream `crate::config::BackupConfig` imports are
//! unaffected.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BackupConfig {

    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_backup_max_keep")]
    pub max_keep: usize,

    #[serde(default = "default_backup_include_dirs")]
    pub include_dirs: Vec<String>,

    #[serde(default = "default_backup_destination_dir")]
    pub destination_dir: String,

    #[serde(default)]
    pub schedule_cron: Option<String>,

    #[serde(default)]
    pub schedule_timezone: Option<String>,

    #[serde(default = "default_true")]
    pub compress: bool,

    #[serde(default)]
    pub encrypt: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DataRetentionConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_retention_days")]
    pub retention_days: u64,

    #[serde(default)]
    pub dry_run: bool,

    #[serde(default)]
    pub categories: Vec<String>,
}

fn default_true() -> bool {
    true
}
fn default_backup_max_keep() -> usize {
    10
}
fn default_backup_include_dirs() -> Vec<String> {
    vec![
        "config".into(),
        "memory".into(),
        "audit".into(),
        "knowledge".into(),
    ]
}
fn default_backup_destination_dir() -> String {
    "state/backups".into()
}
fn default_retention_days() -> u64 {
    90
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_keep: default_backup_max_keep(),
            include_dirs: default_backup_include_dirs(),
            destination_dir: default_backup_destination_dir(),
            schedule_cron: None,
            schedule_timezone: None,
            compress: true,
            encrypt: false,
        }
    }
}

impl Default for DataRetentionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            retention_days: default_retention_days(),
            dry_run: false,
            categories: Vec::new(),
        }
    }
}

impl BackupConfig {

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if !self.enabled {
            return errors;
        }
        if self.max_keep == 0 {
            errors.push("backup.max_keep must be >= 1".into());
        }
        if self.include_dirs.is_empty() {
            errors.push("backup.include_dirs cannot be empty".into());
        }
        if self.destination_dir.trim().is_empty() {
            errors.push("backup.destination_dir must be non-empty".into());
        }
        if let Some(ref tz) = self.schedule_timezone {
            if tz.trim().is_empty() {
                errors.push("backup.schedule_timezone must be non-empty when set".into());
            }
        }
        errors
    }
}

impl DataRetentionConfig {

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.enabled && self.retention_days == 0 {
            errors.push("data_retention.retention_days must be >= 1 when enabled".into());
        }
        errors
    }
}
