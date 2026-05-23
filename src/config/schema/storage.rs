// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct StorageConfig {

    #[serde(default)]
    pub provider: StorageProviderSection,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct StorageProviderSection {

    #[serde(default)]
    pub config: StorageProviderConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StorageProviderConfig {

    #[serde(default)]
    pub provider: String,

    #[serde(
        default,
        alias = "dbURL",
        alias = "database_url",
        alias = "databaseUrl"
    )]
    pub db_url: Option<String>,

    #[serde(default = "default_storage_schema")]
    pub schema: String,

    #[serde(default = "default_storage_table")]
    pub table: String,

    #[serde(default)]
    pub connect_timeout_secs: Option<u64>,
}

fn default_storage_schema() -> String {
    "public".into()
}

fn default_storage_table() -> String {
    "memories".into()
}

impl Default for StorageProviderConfig {
    fn default() -> Self {
        Self {
            provider: String::new(),
            db_url: None,
            schema: default_storage_schema(),
            table: default_storage_table(),
            connect_timeout_secs: None,
        }
    }
}

