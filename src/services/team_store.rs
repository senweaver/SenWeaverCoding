// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamInfo {
    pub id: String,
    pub name: String,
    pub members: Vec<String>,
    pub leader: Option<String>,
    pub created_at: String,
}

pub type TeamRegistry = Arc<RwLock<HashMap<String, TeamInfo>>>;

pub fn global_team_registry() -> TeamRegistry {
    static GLOBAL: OnceLock<TeamRegistry> = OnceLock::new();
    GLOBAL
        .get_or_init(|| Arc::new(RwLock::new(HashMap::new())))
        .clone()
}
