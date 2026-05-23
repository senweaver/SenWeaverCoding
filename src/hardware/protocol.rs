// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ZcCommand {

    pub cmd: String,

    #[serde(default)]
    pub params: serde_json::Value,
}

impl ZcCommand {

    pub fn new(cmd: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            cmd: cmd.into(),
            params,
        }
    }

    pub fn simple(cmd: impl Into<String>) -> Self {
        Self {
            cmd: cmd.into(),
            params: serde_json::Value::Object(serde_json::Map::new()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ZcResponse {

    pub ok: bool,

    #[serde(default)]
    pub data: serde_json::Value,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ZcResponse {

    pub fn success(data: serde_json::Value) -> Self {
        Self {
            ok: true,
            data,
            error: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: serde_json::Value::Null,
            error: Some(message.into()),
        }
    }
}
