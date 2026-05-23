// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CronConfig {

    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_true")]
    pub catch_up_on_startup: bool,

    #[serde(default = "default_max_run_history")]
    pub max_run_history: u32,

    #[serde(default)]
    pub jobs: Vec<CronJobDecl>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CronJobDecl {

    pub id: String,

    #[serde(default)]
    pub name: Option<String>,

    #[serde(default = "default_job_type_decl")]
    pub job_type: String,

    pub schedule: CronScheduleDecl,

    #[serde(default)]
    pub command: Option<String>,

    #[serde(default)]
    pub prompt: Option<String>,

    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default)]
    pub model: Option<String>,

    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,

    #[serde(default)]
    pub session_target: Option<String>,

    #[serde(default)]
    pub delivery: Option<DeliveryConfigDecl>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum CronScheduleDecl {

    Cron {
        expr: String,
        #[serde(default)]
        tz: Option<String>,
    },

    Every { every_ms: u64 },

    At { at: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeliveryConfigDecl {

    #[serde(default = "default_delivery_mode")]
    pub mode: String,

    #[serde(default)]
    pub channel: Option<String>,

    #[serde(default)]
    pub to: Option<String>,

    #[serde(default = "default_true")]
    pub best_effort: bool,
}

fn default_job_type_decl() -> String {
    "shell".to_string()
}

fn default_delivery_mode() -> String {
    "none".to_string()
}

fn default_max_run_history() -> u32 {
    50
}

impl Default for CronConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            catch_up_on_startup: true,
            max_run_history: default_max_run_history(),
            jobs: Vec::new(),
        }
    }
}

