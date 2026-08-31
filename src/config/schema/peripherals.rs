// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct PeripheralsConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub boards: Vec<PeripheralBoardConfig>,

    #[serde(default)]
    pub datasheet_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PeripheralBoardConfig {

    pub board: String,

    #[serde(default = "default_peripheral_transport")]
    pub transport: String,

    #[serde(default)]
    pub path: Option<String>,

    #[serde(default = "default_peripheral_baud")]
    pub baud: u32,
}

fn default_peripheral_transport() -> String {
    "serial".into()
}

fn default_peripheral_baud() -> u32 {
    115_200
}

impl Default for PeripheralBoardConfig {
    fn default() -> Self {
        Self {
            board: String::new(),
            transport: default_peripheral_transport(),
            path: None,
            baud: default_peripheral_baud(),
        }
    }
}
