// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const DEFAULT_TEMPERATURE: f64 = 0.7;

pub const DEFAULT_PROVIDER_TIMEOUT_SECS: u64 = 120;

fn default_temperature() -> f64 {
    DEFAULT_TEMPERATURE
}

fn default_provider_timeout_secs() -> u64 {
    DEFAULT_PROVIDER_TIMEOUT_SECS
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ClassificationRule {

    pub name: String,

    pub pattern: String,

    pub hint: String,
}

impl Default for ClassificationRule {
    fn default() -> Self {
        Self {
            name: String::new(),
            pattern: String::new(),
            hint: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct QueryClassificationConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub rules: Vec<ClassificationRule>,
}

impl QueryClassificationConfig {

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        for (i, rule) in self.rules.iter().enumerate() {
            if rule.pattern.is_empty() {
                errors.push(format!(
                    "query_classification.rules[{}].pattern must not be empty",
                    i
                ));
            }
            if rule.hint.is_empty() {
                errors.push(format!(
                    "query_classification.rules[{}].hint must not be empty",
                    i
                ));
            }
        }
        errors
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModelRouteConfig {

    pub hint: String,

    pub provider: String,

    pub model: String,

    #[serde(default)]
    pub api_key: Option<String>,

    #[serde(default)]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct SavedModel {

    pub id: String,

    pub name: String,

    pub provider: String,

    pub api_key: Option<String>,

    #[serde(default)]
    pub base_url: Option<String>,

    pub model: String,

    #[serde(default = "default_temperature")]
    pub temperature: f64,

    #[serde(default = "default_provider_timeout_secs")]
    pub timeout_secs: u64,
}

impl Default for SavedModel {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: String::new(),
            provider: "openrouter".to_string(),
            api_key: None,
            base_url: None,
            model: String::new(),
            temperature: default_temperature(),
            timeout_secs: default_provider_timeout_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EmbeddingRouteConfig {

    pub hint: String,

    pub provider: String,

    pub model: String,

    #[serde(default)]
    pub dimensions: Option<usize>,

    #[serde(default)]
    pub api_key: Option<String>,
}
