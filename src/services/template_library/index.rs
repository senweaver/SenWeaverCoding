// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BaselineRecord {
    pub hash: String,
    #[serde(default)]
    pub edited_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BaselineIndex {
    #[serde(default)]
    pub baselines: BTreeMap<String, BaselineRecord>,
}

impl BaselineIndex {
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn baseline_hash(&self, rel: &str) -> Option<&str> {
        self.baselines.get(rel).map(|r| r.hash.as_str())
    }
}
