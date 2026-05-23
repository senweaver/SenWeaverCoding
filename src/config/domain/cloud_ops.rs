// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CloudOpsConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_cloud_ops_cloud")]
    pub default_cloud: String,

    #[serde(default = "default_cloud_ops_supported_clouds")]
    pub supported_clouds: Vec<String>,

    #[serde(default = "default_cloud_ops_iac_tools")]
    pub iac_tools: Vec<String>,

    #[serde(default = "default_cloud_ops_cost_threshold")]
    pub cost_threshold_monthly_usd: f64,

    #[serde(default = "default_cloud_ops_waf")]
    pub well_architected_frameworks: Vec<String>,
}

impl Default for CloudOpsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_cloud: default_cloud_ops_cloud(),
            supported_clouds: default_cloud_ops_supported_clouds(),
            iac_tools: default_cloud_ops_iac_tools(),
            cost_threshold_monthly_usd: default_cloud_ops_cost_threshold(),
            well_architected_frameworks: default_cloud_ops_waf(),
        }
    }
}

impl CloudOpsConfig {
    pub fn validate(&self) -> Result<()> {
        if self.enabled {
            if self.default_cloud.trim().is_empty() {
                anyhow::bail!(
                    "cloud_ops.default_cloud must not be empty when cloud_ops is enabled"
                );
            }
            if self.supported_clouds.is_empty() {
                anyhow::bail!(
                    "cloud_ops.supported_clouds must not be empty when cloud_ops is enabled"
                );
            }
            for (i, cloud) in self.supported_clouds.iter().enumerate() {
                if cloud.trim().is_empty() {
                    anyhow::bail!("cloud_ops.supported_clouds[{i}] must not be empty");
                }
            }
            if !self.supported_clouds.contains(&self.default_cloud) {
                anyhow::bail!(
                    "cloud_ops.default_cloud '{}' is not in cloud_ops.supported_clouds {:?}",
                    self.default_cloud,
                    self.supported_clouds
                );
            }
            if self.cost_threshold_monthly_usd < 0.0 {
                anyhow::bail!(
                    "cloud_ops.cost_threshold_monthly_usd must be non-negative, got {}",
                    self.cost_threshold_monthly_usd
                );
            }
            if self.iac_tools.is_empty() {
                anyhow::bail!("cloud_ops.iac_tools must not be empty when cloud_ops is enabled");
            }
        }
        Ok(())
    }
}

pub(crate) fn default_cloud_ops_cloud() -> String {
    "aws".into()
}
pub(crate) fn default_cloud_ops_supported_clouds() -> Vec<String> {
    vec!["aws".into(), "azure".into(), "gcp".into()]
}
pub(crate) fn default_cloud_ops_iac_tools() -> Vec<String> {
    vec!["terraform".into()]
}
pub(crate) fn default_cloud_ops_cost_threshold() -> f64 {
    100.0
}
pub(crate) fn default_cloud_ops_waf() -> Vec<String> {
    vec!["aws-waf".into()]
}
