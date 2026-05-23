// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default, JsonSchema)]
pub enum HardwareTransport {

    #[default]
    None,

    Native,

    Serial,

    Probe,
}

impl std::fmt::Display for HardwareTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "none"),
            Self::Native => write!(f, "native"),
            Self::Serial => write!(f, "serial"),
            Self::Probe => write!(f, "probe"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HardwareConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub transport: HardwareTransport,

    #[serde(default)]
    pub serial_port: Option<String>,

    #[serde(default = "default_baud_rate")]
    pub baud_rate: u32,

    #[serde(default)]
    pub probe_target: Option<String>,

    #[serde(default)]
    pub workspace_datasheets: bool,
}

pub(crate) fn default_baud_rate() -> u32 {
    115_200
}

impl HardwareConfig {

    pub fn transport_mode(&self) -> HardwareTransport {
        self.transport.clone()
    }

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if !self.enabled {
            return errors;
        }
        match self.transport {
            HardwareTransport::Serial => {
                if self.serial_port.is_none() {
                    errors.push(
                        "hardware.transport=serial requires hardware.serial_port to be set".into(),
                    );
                }
                if self.baud_rate == 0 {
                    errors.push("hardware.baud_rate must be > 0".into());
                }
            }
            HardwareTransport::Probe => {
                if self.probe_target.is_none() {
                    errors.push(
                        "hardware.transport=probe requires hardware.probe_target to be set".into(),
                    );
                }
            }
            _ => {}
        }
        errors
    }
}

impl Default for HardwareConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            transport: HardwareTransport::None,
            serial_port: None,
            baud_rate: default_baud_rate(),
            probe_target: None,
            workspace_datasheets: false,
        }
    }
}
